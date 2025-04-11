//! FHE-accelerated training benchmarks with GPU/CUDA integration

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use concrete::prelude::*;
use concrete::{ConfigBuilder, CudaEngine, Variance};
use haunti_crypto::fhe_ops::FheNetwork;
use std::time::Duration;

const FHE_PARAMS: &str = "
    lattice_dimension: 2048
    log2_poly_size: 14
    variance: 0.0000001
    secret_key_dist: ternary
";

#[derive(Clone)]
struct BenchConfig {
    sample_size: usize,
    feature_dim: usize,
    num_classes: usize,
    batch_size: usize,
    use_gpu: bool,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            sample_size: 1000,
            feature_dim: 128,
            num_classes: 10,
            batch_size: 32,
            use_gpu: cfg!(feature = "cuda"),
        }
    }
}

fn fhe_init(cfg: &BenchConfig) -> (CudaEngine, FheNetwork) {
    let config = ConfigBuilder::from_str(FHE_PARAMS)
        .use_gpu(cfg.use_gpu)
        .build();
    
    let engine = CudaEngine::new(config).unwrap();
    let network = FheNetwork::new(
        cfg.feature_dim,
        cfg.num_classes,
        cfg.batch_size,
        engine.key_pair().clone()
    );
    
    (engine, network)
}

fn encrypt_dataset(engine: &CudaEngine, cfg: &BenchConfig) -> Vec<Vec<CudaCiphertext>> {
    (0..cfg.sample_size)
        .map(|_| {
            let data: Vec<f64> = (0..cfg.feature_dim)
                .map(|_| rand::random::<f64>())
                .collect();
            engine.encrypt(&data).unwrap()
        })
        .collect()
}

fn training_benchmark(c: &mut Criterion, cfg: BenchConfig) {
    let (engine, mut network) = fhe_init(&cfg);
    let dataset = encrypt_dataset(&engine, &cfg);
    
    let mut group = c.benchmark_group("FHE Training");
    group.sample_size(10)
         .measurement_time(Duration::from_secs(30))
         .warm_up_time(Duration::from_secs(5));
    
    group.bench_function("full_epoch", |b| {
        b.iter(|| {
            for batch in dataset.chunks(cfg.batch_size) {
                let encrypted_batch = batch.iter()
                    .map(|ct| ct.to_owned())
                    .collect();
                
                let grads = network.forward_backward(
                    black_box(encrypted_batch),
                    black_box(0.01) // learning rate
                ).unwrap();
                
                engine.synchronize(); // GPU sync
                black_box(grads);
            }
        })
    });
    
    group.bench_function("encrypt_batch", |b| {
        let data: Vec<f64> = (0..cfg.feature_dim)
            .map(|_| rand::random())
            .collect();
        
        b.iter(|| {
            let ct = engine.encrypt(black_box(&data)).unwrap();
            black_box(ct);
        })
    });
}

fn bench_config() -> Criterion {
    Criterion::default()
        .with_plots()
        .configure_from_args()
}

criterion_group!{
    name = fhe_benches;
    config = bench_config();
    targets = 
        training_benchmark,
}

criterion_main!(fhe_benches);
