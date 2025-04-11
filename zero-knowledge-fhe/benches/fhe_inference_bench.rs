//! FHE-accelerated inference benchmarks with multi-GPU support

use criterion::{black_box, criterion_group, criterion_main, AxisScale, Criterion, PlotConfiguration, SamplingMode};
use concrete::{
    prelude::*,
    CudaEngine,
    CudaStream,
    CudaConfig,
    security::SecurityLevel,
};
use concrete_nn::FheConvNet;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Security parameters meeting 128-bit security
const FHE_CONFIG: &str = "
lattice_dimension: 8192
log2_poly_size: 17
variance: 0.0000000001
secret_key_dist: gaussian
";

#[derive(Debug, Clone)]
struct InferenceConfig {
    input_dims: (usize, usize, usize), // (channels, height, width)
    model_layers: Vec<(usize, usize)>, // (num_filters, kernel_size)
    batch_sizes: Vec<usize>,
    use_multi_gpu: bool,
    precision_bits: u32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            input_dims: (3, 224, 224),
            model_layers: vec![(64, 3), (128, 3), (256, 3)],
            batch_sizes: vec![1, 8, 32],
            use_multi_gpu: false,
            precision_bits: 16,
        }
    }
}

struct BenchContext {
    engine: Arc<CudaEngine>,
    model: FheConvNet,
    encrypted_inputs: Vec<CudaCiphertext>,
}

fn initialize_bench(ctx_cfg: &InferenceConfig) -> BenchContext {
    let config = CudaConfig::from_str(FHE_CONFIG)
        .use_multi_gpu(ctx_cfg.use_multi_gpu)
        .set_precision(ctx_cfg.precision_bits)
        .build();
    
    let engine = Arc::new(CudaEngine::new(config).unwrap());
    let mut model = FheConvNet::new(ctx_cfg.input_dims);
    
    for (filters, kernel) in &ctx_cfg.model_layers {
        model.add_conv_layer(*filters, *kernel, 1, 1); // stride=1, padding=1
    }
    
    // Generate encrypted test inputs
    let mut rng = rand::thread_rng();
    let encrypted_inputs = (0..ctx_cfg.batch_sizes.iter().max().unwrap())
        .map(|_| {
            let input: Vec<f64> = (0..ctx_cfg.input_dims.0 * ctx_cfg.input_dims.1 * ctx_cfg.input_dims.2)
                .map(|_| rng.gen_range(-1.0..1.0))
                .collect();
            engine.encrypt(&input).unwrap()
        })
        .collect();
    
    BenchContext {
        engine,
        model,
        encrypted_inputs,
    }
}

fn inference_benchmark(c: &mut Criterion, cfg: InferenceConfig) {
    let ctx = initialize_bench(&cfg);
    let mut group = c.benchmark_group("FHE Inference");
    
    group.plot_config(PlotConfiguration::default()
        .summary_scale(AxisScale::Logarithmic)
    );
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(5));
    group.measurement_time(Duration::from_secs(30));

    for &batch_size in &cfg.batch_sizes {
        group.bench_with_input(
            format!("batch_{}", batch_size),
            &batch_size,
            |b, &bs| {
                let inputs = &ctx.encrypted_inputs[0..bs];
                
                // Warmup and synchronization
                ctx.model.forward(&inputs[0]);
                ctx.engine.synchronize();
                
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;
                    
                    for _ in 0..iters {
                        let start = Instant::now();
                        
                        for input in inputs {
                            let output = ctx.model.forward(input).unwrap();
                            black_box(output);
                        }
                        
                        ctx.engine.synchronize();
                        total_duration += start.elapsed();
                    }
                    
                    total_duration / (inputs.len() as u32 * iters)
                })
            },
        );
    }
}

fn encryption_benchmark(c: &mut Criterion, cfg: &InferenceConfig) {
    let ctx = initialize_bench(cfg);
    let input_size = cfg.input_dims.0 * cfg.input_dims.1 * cfg.input_dims.2;
    let mut rng = rand::thread_rng();
    
    let mut group = c.benchmark_group("FHE Encryption");
    group.plot_config(PlotConfiguration::default());
    
    group.bench_function("encrypt_input", |b| {
        let data: Vec<f64> = (0..input_size)
            .map(|_| rng.gen_range(-1.0..1.0))
            .collect();
        
        b.iter(|| {
            let ct = ctx.engine.encrypt(black_box(&data)).unwrap();
            black_box(ct);
        })
    });
    
    group.bench_function("decrypt_output", |b| {
        let output = ctx.model.forward(&ctx.encrypted_inputs[0]).unwrap();
        
        b.iter(|| {
            let pt = ctx.engine.decrypt(black_box(&output)).unwrap();
            black_box(pt);
        })
    });
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        .with_plots()
        .configure_from_args()
}

criterion_group!{
    name = benches;
    config = configure_criterion();
    targets = 
        inference_benchmark,
        encryption_benchmark,
}

criterion_main!(benches);
