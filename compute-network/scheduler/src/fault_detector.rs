//! Fault Detection & Recovery System with Byzantine Consensus
//! Integrated with Solana Validators and ZK Proof Audits

use anchor_lang::prelude::*;
use solana_program::clock::Clock;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock},
    time::interval,
};

#[derive(Clone, Debug, PartialEq, AnchorSerialize, AnchorDeserialize)]
pub enum FaultType {
    ComputeTimeout(u64),  // Task ID
    MemoryOverflow,        // GPU ID
    ZKProofMismatch,       // Proof CID
    DataAvailabilityError, // IPFS CID
    ByzantineBehavior,     // Node ID
}

#[derive(Clone, Debug)]
pub struct NodeHealth {
    pub last_heartbeat: Instant,
    pub task_success_rate: f32,
    pub resource_usage: ResourceMetrics,
    pub reputation_score: u8,
    pub staked_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct ResourceMetrics {
    pub gpu_util: f32,
    pub mem_util: f32,
    pub network_util: f32,
    pub disk_io: f32,
}

#[derive(Error, Debug)]
pub enum FaultError {
    #[error("Consensus failure: {0}")]
    ConsensusFailure(String),
    
    #[error("Recovery timeout: {0}")]
    RecoveryTimeout(String),
    
    #[error("Insufficient stake: {0}")]
    InsufficientStake(String),
}

pub struct FaultDetector {
    node_registry: Arc<RwLock<HashMap<String, NodeHealth>>>,
    pending_faults: Arc<Mutex<Vec<(FaultType, String)>>>,
    consensus_threshold: u8,
}

impl FaultDetector {
    pub fn new(consensus_ratio: f32) -> Self {
        Self {
            node_registry: Arc::new(RwLock::new(HashMap::new())),
            pending_faults: Arc::new(Mutex::new(Vec::new())),
            consensus_threshold: (consensus_ratio * 10.0) as u8,
        }
    }

    /// Core detection loop with adaptive intervals
    pub async fn start_monitoring(&self) {
        let mut interval = interval(Duration::from_secs(30));
        
        loop {
            interval.tick().await;
            self.check_heartbeats().await;
            self.audit_pending_tasks().await;
            self.verify_consensus().await;
        }
    }

    async fn check_heartbeats(&self) {
        let registry = self.node_registry.read().await;
        let mut faults = self.pending_faults.lock().await;
        
        for (node_id, health) in registry.iter() {
            if health.last_heartbeat.elapsed() > Duration::from_secs(120) {
                faults.push((FaultType::ByzantineBehavior, node_id.clone()));
            }
        }
    }

    async fn audit_pending_tasks(&self) {
        // Integration with Solana ledger & IPFS
        // Placeholder for actual audit logic
    }

    async fn verify_consensus(&self) {
        let mut faults = self.pending_faults.lock().await;
        let mut registry = self.node_registry.write().await;
        
        let mut fault_counts: HashMap<String, u8> = HashMap::new();
        
        for (fault, id) in faults.iter() {
            *fault_counts.entry(id.clone()).or_insert(0) += 1;
        }

        for (id, count) in fault_counts {
            if count >= self.consensus_threshold {
                self.apply_penalties(&id, &mut registry).await;
            }
        }
        
        faults.clear();
    }

    async fn apply_penalties(&self, node_id: &str, registry: &mut HashMap<String, NodeHealth>) {
        if let Some(health) = registry.get_mut(node_id) {
            // Slashing mechanism
            let penalty = (health.staked_tokens as f32 * 0.1) as u64;
            health.staked_tokens = health.staked_tokens.saturating_sub(penalty);
            
            // Reputation decay
            health.reputation_score = health.reputation_score.saturating_sub(10);
            
            // Auto-quarantine if below threshold
            if health.reputation_score < 20 {
                self.quarantine_node(node_id).await;
            }
        }
    }

    async fn quarantine_node(&self, node_id: &str) {
        // Integration with network layer
        // Placeholder for actual quarantine logic
    }

    /// Public API for external fault reporting
    pub async fn report_fault(&self, fault: FaultType, node_id: String) -> Result<(), FaultError> {
        let mut faults = self.pending_faults.lock().await;
        faults.push((fault, node_id));
        Ok(())
    }

    /// ZK-based validation of compute faults
    pub async fn validate_with_zkp(
        &self,
        proof: Vec<u8>,
        public_inputs: Vec<u8>
    ) -> Result<bool, FaultError> {
        // Integration with Plonky3 verifier
        // Placeholder for actual proof verification
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_heartbeat_failure() {
        let detector = FaultDetector::new(0.6);
        let node_id = "test_node".to_string();
        
        detector.node_registry.write().await.insert(
            node_id.clone(),
            NodeHealth {
                last_heartbeat: Instant::now() - Duration::from_secs(300),
                task_success_rate: 0.9,
                resource_usage: ResourceMetrics {
                    gpu_util: 0.3,
                    mem_util: 0.4,
                    network_util: 0.2,
                    disk_io: 0.1,
                },
                reputation_score: 50,
                staked_tokens: 1000,
            }
        );

        detector.check_heartbeats().await;
        let faults = detector.pending_faults.lock().await;
        assert!(faults.len() > 0);
    }

    #[tokio::test]
    async fn test_consensus_penalty() {
        let detector = FaultDetector::new(0.6);
        let node_id = "bad_actor".to_string();
        
        detector.node_registry.write().await.insert(
            node_id.clone(),
            NodeHealth {
                last_heartbeat: Instant::now(),
                task_success_rate: 0.5,
                resource_usage: ResourceMetrics {
                    gpu_util: 0.8,
                    mem_util: 0.9,
                    network_util: 0.7,
                    disk_io: 0.6,
                },
                reputation_score: 30,
                staked_tokens: 500,
            }
        );

        for _ in 0..7 {
            detector.report_fault(
                FaultType::ByzantineBehavior,
                node_id.clone()
            ).await.unwrap();
        }

        detector.verify_consensus().await;
        let registry = detector.node_registry.read().await;
        let health = registry.get(&node_id).unwrap();
        
        assert_eq!(health.staked_tokens, 450); // 10% penalty
        assert_eq!(health.reputation_score, 20);
    }
}
