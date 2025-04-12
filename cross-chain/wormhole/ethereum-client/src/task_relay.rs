//! Cross-chain task orchestration engine with multi-protocol relay
//! Supports Solana/Ethereum/Cosmos chains with dynamic routing

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use wormhole_sdk::{
    vaa::Vaa,
    Address,
    Chain,
    Message,
    Serialize,
};
use ibc_proto::{
    cosmos::base::v1beta1::Coin,
    ibc::core::client::v1::Height,
};
use layer_zero::{
    Endpoint,
    Packet,
    UaConfig,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

// Custom error handling
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("Invalid source chain")]
    InvalidSourceChain,
    #[error("Unsupported destination chain")]
    UnsupportedChain,
    #[error("VAA verification failed")]
    VaaVerificationFailed,
    #[error("IBC channel not established")]
    IbcChannelError,
    #[error("Insufficient relay fee")]
    InsufficientFee,
    #[error("Payload size exceeded")]
    PayloadSizeExceeded,
    #[error("Invalid task nonce")]
    InvalidNonce,
    #[error("Gas estimation failed")]
    GasEstimationError,
    #[error("Relayer signature invalid")]
    SignatureError,
}

// Task state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Relaying,
    Completed,
    Failed,
}

// Cross-chain task metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayTask {
    pub source_chain: Chain,
    pub dest_chain: Chain,
    pub task_type: TaskType,
    pub payload: Vec<u8>,
    pub nonce: u64,
    pub timestamp: u64,
    pub retries: u8,
    pub state: TaskState,
    pub gas_estimate: u64,
    pub fee_payment: Option<Coin>,
}

// Protocol configuration
#[derive(Clone)]
pub struct RelayConfig {
    pub wormhole_bridge: Pubkey,
    pub ibc_channel: String,
    pub layerzero_endpoint: Endpoint,
    pub max_payload_size: usize,
    pub fee_denom: String,
    pub min_fee: u64,
    pub max_retries: u8,
}

// Core relay engine
pub struct TaskRelayer {
    config: RelayConfig,
    task_queue: Arc<Mutex<VecDeque<RelayTask>>>,
    state_cache: Arc<Mutex<HashMap<u64, TaskState>>>,
    chain_clients: HashMap<Chain, Box<dyn ChainClient>>,
    metrics: RelayMetrics,
}

impl TaskRelayer {
    pub fn new(config: RelayConfig) -> Self {
        Self {
            config,
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            state_cache: Arc::new(Mutex::new(HashMap::new())),
            chain_clients: initialize_chain_clients(),
            metrics: RelayMetrics::new(),
        }
    }

    // Main processing loop
    pub async fn run(&mut self) {
        loop {
            if let Some(task) = self.task_queue.lock().unwrap().pop_front() {
                self.process_task(task).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn process_task(&mut self, mut task: RelayTask) {
        self.metrics.inc_tasks_processed();
        
        // Validate task basics
        if let Err(e) = self.validate_task(&task) {
            self.handle_error(task, e).await;
            return;
        }

        // Select relay protocol
        let protocol = self.select_protocol(&task.dest_chain);
        
        // Execute relay
        let result = match protocol {
            RelayProtocol::Wormhole => self.relay_via_wormhole(&task).await,
            RelayProtocol::IBC => self.relay_via_ibc(&task).await,
            RelayProtocol::LayerZero => self.relay_via_layerzero(&task).await,
        };

        // Update state
        match result {
            Ok(_) => {
                task.state = TaskState::Completed;
                self.metrics.inc_tasks_success();
            }
            Err(e) => {
                task.retries += 1;
                task.state = TaskState::Failed;
                self.metrics.inc_tasks_failed();
                self.handle_retry(task, e).await;
            }
        }
    }

    // Protocol selection logic
    fn select_protocol(&self, chain: &Chain) -> RelayProtocol {
        match chain {
            Chain::Solana | Chain::Ethereum => RelayProtocol::Wormhole,
            Chain::Cosmos | Chain::Osmosis => RelayProtocol::IBC,
            Chain::Avalanche | Chain::Polygon => RelayProtocol::LayerZero,
            _ => RelayProtocol::Wormhole,
        }
    }

    // Wormhole message relay
    async fn relay_via_wormhole(&self, task: &RelayTask) -> Result<(), RelayError> {
        let vaa = self.generate_vaa(task).await?;
        let signature = self.sign_vaa(&vaa).await?;
        
        let client = self.chain_client(Chain::Solana)?;
        client.submit_vaa(vaa, signature).await
    }

    // IBC packet relay
    async fn relay_via_ibc(&self, task: &RelayTask) -> Result<(), RelayError> {
        let packet = ibc_packet(task)?;
        let height = Height {
            revision_number: 1,
            revision_height: self.latest_block().await?,
        };
        
        let client = self.chain_client(Chain::Cosmos)?;
        client.send_ibc_packet(packet, height).await
    }

    // LayerZero endpoint relay
    async fn relay_via_layerzero(&self, task: &RelayTask) -> Result<(), RelayError> {
        let ua_config = UaConfig::new()
            .with_gas_limit(task.gas_estimate)
            .with_ack_type(1);
            
        let packet = Packet::new(
            task.payload.clone(),
            self.config.layerzero_endpoint.clone(),
            ua_config,
        );
        
        let client = self.chain_client(Chain::Ethereum)?;
        client.send_layerzero_packet(packet).await
    }

    // VAA generation logic
    async fn generate_vaa(&self, task: &RelayTask) -> Result<Vaa, RelayError> {
        let emitter = Address::from(&self.config.wormhole_bridge.to_bytes());
        let sequence = self.next_sequence().await?;
        
        Vaa::new(
            wormhole_sdk::HEADER_VERSION,
            wormhole_sdk::GUARDIAN_SET_INDEX,
            sequence,
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            emitter,
            task.dest_chain.into(),
            task.payload.clone(),
        )
    }

    // State validation
    fn validate_task(&self, task: &RelayTask) -> Result<(), RelayError> {
        if task.payload.len() > self.config.max_payload_size {
            return Err(RelayError::PayloadSizeExceeded);
        }

        if let Some(fee) = &task.fee_payment {
            if fee.denom != self.config.fee_denom || fee.amount < self.config.min_fee.into() {
                return Err(RelayError::InsufficientFee);
            }
        }

        Ok(())
    }
}

// Chain client abstraction
#[async_trait]
pub trait ChainClient {
    async fn submit_vaa(&self, vaa: Vaa, signature: Vec<u8>) -> Result<(), RelayError>;
    async fn send_ibc_packet(&self, packet: Packet, height: Height) -> Result<(), RelayError>;
    async fn send_layerzero_packet(&self, packet: Packet) -> Result<(), RelayError>;
    async fn gas_estimate(&self, payload: &[u8]) -> Result<u64, RelayError>;
}

// Metrics tracking
struct RelayMetrics {
    tasks_processed: Counter,
    tasks_success: Counter,
    tasks_failed: Counter,
    latency: Histogram,
}

impl RelayMetrics {
    fn new() -> Self {
        Self {
            tasks_processed: Counter::new(),
            tasks_success: Counter::new(),
            tasks_failed: Counter::new(),
            latency: Histogram::with_buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0]),
        }
    }
}

// Protocol implementations
#[derive(Debug, Clone, Copy)]
enum RelayProtocol {
    Wormhole,
    IBC,
    LayerZero,
}

// Error handling utilities
impl From<RelayError> for ProgramError {
    fn from(e: RelayError) -> Self {
        ProgramError::Custom(e as u32 + 1000)
    }
}

// Entrypoint for Solana program
entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Starting task relay...");
    
    let config = RelayConfig::load(program_id)?;
    let mut relayer = TaskRelayer::new(config);
    
    let task = RelayTask::deserialize(instruction_data)?;
    relayer.queue_task(task)?;

    Ok(())
}

// Unit tests
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wormhole_relay() {
        let config = test_config();
        let mut relayer = TaskRelayer::new(config);
        
        let task = test_task(Chain::Solana, Chain::Ethereum);
        relayer.queue_task(task).unwrap();
        
        relayer.run().await;
        assert_eq!(relayer.metrics.tasks_success.count(), 1);
    }

    #[tokio::test]
    async fn test_fee_validation() {
        let config = test_config();
        let mut relayer = TaskRelayer::new(config);
        
        let mut task = test_task(Chain::Solana, Chain::Ethereum);
        task.fee_payment = Some(Coin {
            denom: "uatom".to_string(),
            amount: 50,
        });
        
        assert!(relayer.validate_task(&task).is_err());
    }
}
