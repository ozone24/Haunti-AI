//! Compute Network Coordinator Node - Core Runtime

#[macro_use]
extern crate prometheus;
use anchor_lang::prelude::*;
use anyhow::Context;
use clap::Parser;
use haunti_crypto::{fhe::FheRuntime, zk::PlonkProver};
use haunti_gpu::CudaAllocator;
use haunti_network::{
    consensus::ProofOfCompute,
    scheduler::{TaskScheduler, WorkerNode},
    storage::IpfsClient,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::RwLock,
    task::JoinSet,
};
use tracing::{info, instrument, Level};
use tracing_subscriber::{fmt, EnvFilter};

/// Global configuration for the compute network
#[derive(Debug, Clone, Parser)]
#[clap(version, about = "Haunti Compute Network Coordinator")]
struct Config {
    #[clap(long, env, default_value = "0.0.0.0:9090")]
    http_addr: SocketAddr,

    #[clap(long, env, default_value = "devnet")]
    solana_cluster: String,

    #[clap(long, env, default_value = "5")]
    heartbeat_interval_secs: u64,

    #[clap(long, env, default_value = "10")]
    max_concurrent_tasks: usize,

    #[clap(long, env)]
    gpu_enabled: bool,
}

/// Core coordinator state
struct Coordinator {
    scheduler: Arc<RwLock<TaskScheduler>>,
    solana_client: Arc<RpcClient>,
    ipfs: IpfsClient,
    fhe_runtime: Option<Arc<FheRuntime>>,
    zk_prover: Arc<PlonkProver>,
    metrics: MetricsRegistry,
    workers: Arc<RwLock<Vec<WorkerNode>>>,
}

impl Coordinator {
    #[instrument(skip_all)]
    async fn new(config: &Config) -> anyhow::Result<Self> {
        // Initialize metrics
        let metrics = MetricsRegistry::new()?;

        // Setup Solana RPC client
        let solana_client = Arc::new(RpcClient::new_with_commitment(
            config.solana_cluster.clone(),
            CommitmentConfig::confirmed(),
        ));

        // Initialize cryptographic runtimes
        let fhe_runtime = if config.gpu_enabled {
            Some(Arc::new(FheRuntime::new_gpu().await?))
        } else {
            None
        };
        let zk_prover = Arc::new(PlonkProver::new("circuits/")?);

        Ok(Self {
            scheduler: Arc::new(RwLock::new(TaskScheduler::new(
                config.max_concurrent_tasks,
            ))),
            solana_client,
            ipfs: IpfsClient::default(),
            fhe_runtime,
            zk_prover,
            metrics,
            workers: Arc::new(RwLock::new(Vec::new())),
        })
    }

    #[instrument(skip_all)]
    async fn run(self, config: Config) -> anyhow::Result<()> {
        let mut joinset = JoinSet::new();

        // Start HTTP API server
        joinset.spawn(self.start_http_server(config.http_addr));

        // Start worker heartbeat monitor
        joinset.spawn(self.monitor_workers(config.heartbeat_interval_secs));

        // Start task processing loop
        joinset.spawn(self.process_tasks());

        // Handle signals
        let mut term_signal = signal(SignalKind::terminate())?;
        let mut int_signal = signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = term_signal.recv() => info!("Received SIGTERM, shutting down"),
            _ = int_signal.recv() => info!("Received SIGINT, shutting down"),
            _ = joinset.join_next() => {},
        }

        Ok(())
    }

    #[instrument(skip(self))]
    async fn process_tasks(&self) -> anyhow::Result<()> {
        loop {
            let task = {
                let mut scheduler = self.scheduler.write().await;
                scheduler.next_task().await?
            };

            // Check if task requires GPU
            if task.requires_gpu && self.fhe_runtime.is_none() {
                warn!("Skipping GPU task in CPU-only mode");
                continue;
            }

            // Execute task with retries
            let result = tokio::time::timeout(
                Duration::from_secs(300),
                self.execute_task(task),
            )
            .await??;

            // Submit proof to Solana
            self.submit_proof(result).await?;
        }
    }

    #[instrument(skip(self, task))]
    async fn execute_task(&self, task: ComputeTask) -> anyhow::Result<ComputeProof> {
        // Fetch model & data from IPFS
        let model = self.ipfs.get_cid(&task.model_cid).await?;
        let data = self.ipfs.get_cid(&task.data_cid).await?;

        // Select execution backend
        let backend = if task.use_fhe {
            ExecutionBackend::Fhe(self.fhe_runtime.as_ref().unwrap().clone())
        } else {
            ExecutionBackend::Cpu
        };

        // Execute and generate proof
        let start = Instant::now();
        let (result, proof) = backend.execute(model, data).await?;
        let duration = start.elapsed();

        // Record metrics
        self.metrics
            .task_duration
            .with_label_values(&[&task.task_type])
            .observe(duration.as_secs_f64());

        Ok(ComputeProof { result, proof })
    }

    #[instrument(skip(self, proof))]
    async fn submit_proof(&self, proof: ComputeProof) -> anyhow::Result<()> {
        // Verify proof locally first
        let verified = self.zk_prover.verify(&proof.proof).await?;
        if !verified {
            anyhow::bail!("Invalid proof generated");
        }

        // Submit to Solana program
        let tx = self
            .solana_client
            .submit_compute_proof(proof)
            .await
            .context("Failed to submit proof")?;

        info!(tx = %tx, "Proof submitted successfully");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .init();

    // Parse config
    let config = Config::parse();

    // Set global allocator for GPU
    if config.gpu_enabled {
        #[global_allocator]
        static ALLOCATOR: CudaAllocator = CudaAllocator;
    }

    // Start coordinator
    let coordinator = Coordinator::new(&config).await?;
    coordinator.run(config).await?;

    Ok(())
}
