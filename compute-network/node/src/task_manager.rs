//! Distributed task management system with priority scheduling and resource orchestration

use std::{
    collections::{BinaryHeap, HashMap},
    sync::Arc,
    time::{Duration, SystemTime},
};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use tokio::{
    sync::{Mutex, RwLock},
    time::interval,
};
use thiserror::Error;

/// Priority levels for compute tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Task execution requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub gpu_type: Option<String>,
    pub gpu_count: u8,
    pub memory_gb: u8,
    pub storage_gb: u8,
    pub timeout_secs: u32,
}

/// Task status lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Scheduled,
    Running,
    Completed,
    Failed(String),
    TimedOut,
}

/// Compute task metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTask {
    pub task_id: String,
    pub owner: Pubkey,
    pub priority: TaskPriority,
    pub requirements: ResourceRequirements,
    pub state: TaskState,
    pub created_at: u64,
    pub updated_at: u64,
    pub task_type: TaskType,
    pub model_cid: String,
    pub data_cid: String,
}

/// Types of AI tasks supported
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskType {
    Training {
        epochs: u32,
        batch_size: u32,
    },
    Inference {
        input_cid: String,
    },
    FederatedLearning {
        participant_count: u32,
    },
}

#[derive(Debug, Clone)]
struct GpuResource {
    device_id: String,
    memory_allocated: u64,
    total_memory: u64,
    supported_ops: Vec<String>,
}

#[derive(Debug)]
struct ResourcePool {
    gpu_devices: Vec<GpuResource>,
    available_memory_gb: u64,
    total_memory_gb: u64,
}

/// Central task management system
pub struct TaskManager {
    rpc_client: Arc<RpcClient>,
    pending_queue: Arc<Mutex<BinaryHeap<Arc<ComputeTask>>>>,
    running_tasks: Arc<RwLock<HashMap<String, Arc<ComputeTask>>>>,
    resource_pool: Arc<RwLock<ResourcePool>>,
}

#[derive(Error, Debug)]
pub enum TaskManagerError {
    #[error("Insufficient resources")]
    InsufficientResources,
    #[error("Task not found")]
    TaskNotFound,
    #[error("Invalid state transition")]
    InvalidStateTransition,
    #[error("RPC error: {0}")]
    RpcError(#[from] solana_client::client_error::ClientError),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl TaskManager {
    pub fn new(rpc_url: &str) -> Self {
        let client = RpcClient::new_with_commitment(
            rpc_url.to_string(), 
            CommitmentConfig::confirmed()
        );
        
        TaskManager {
            rpc_client: Arc::new(client),
            pending_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            resource_pool: Arc::new(RwLock::new(ResourcePool {
                gpu_devices: vec![],
                available_memory_gb: 0,
                total_memory_gb: 0,
            })),
        }
    }

    /// Add new task to the management system
    pub async fn add_task(&self, task: ComputeTask) -> Result<(), TaskManagerError> {
        let mut queue = self.pending_queue.lock().await;
        queue.push(Arc::new(task));
        Ok(())
    }

    /// Core scheduling logic
    pub async fn schedule_tasks(&self) -> Result<(), TaskManagerError> {
        let mut interval = interval(Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            let mut queue = self.pending_queue.lock().await;
            let mut resources = self.resource_pool.write().await;
            let mut running = self.running_tasks.write().await;

            while let Some(task) = queue.pop() {
                if self.can_allocate(&task.requirements, &resources).await {
                    if let Err(e) = self.start_task(task.clone(), &mut resources).await {
                        log::error!("Failed to start task {}: {}", task.task_id, e);
                        continue;
                    }
                    running.insert(task.task_id.clone(), task.clone());
                } else {
                    queue.push(task);
                    break;
                }
            }
        }
    }

    async fn can_allocate(
        &self,
        requirements: &ResourceRequirements,
        pool: &ResourcePool
    ) -> bool {
        // Check GPU requirements
        if requirements.gpu_count > 0 {
            let available_gpus = pool.gpu_devices.iter()
                .filter(|gpu| {
                    gpu.memory_allocated + (requirements.memory_gb as u64 * 1024 * 1024 * 1024)
                        <= gpu.total_memory
                })
                .count();

            if available_gpus < requirements.gpu_count as usize {
                return false;
            }
        }

        // Check memory requirements
        if (requirements.memory_gb as u64) > pool.available_memory_gb {
            return false;
        }

        true
    }

    async fn start_task(
        &self,
        task: Arc<ComputeTask>,
        resources: &mut ResourcePool
    ) -> Result<(), TaskManagerError> {
        // Allocate GPU resources
        if task.requirements.gpu_count > 0 {
            let mut allocated = 0;
            for gpu in &mut resources.gpu_devices {
                if allocated >= task.requirements.gpu_count {
                    break;
                }
                
                let required_memory = task.requirements.memory_gb as u64 * 1024 * 1024 * 1024;
                if gpu.memory_allocated + required_memory <= gpu.total_memory {
                    gpu.memory_allocated += required_memory;
                    allocated += 1;
                }
            }
            
            if allocated < task.requirements.gpu_count {
                return Err(TaskManagerError::InsufficientResources);
            }
        }

        // Allocate general memory
        resources.available_memory_gb -= task.requirements.memory_gb as u64;

        // Update task state
        let mut task = (*task).clone();
        task.state = TaskState::Running;
        task.updated_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // TODO: Submit to execution engine
        Ok(())
    }

    pub async fn complete_task(
        &self,
        task_id: &str,
        result: &str
    ) -> Result<(), TaskManagerError> {
        let mut running = self.running_tasks.write().await;
        let mut resources = self.resource_pool.write().await;

        let task = running.get(task_id)
            .ok_or(TaskManagerError::TaskNotFound)?;

        // Free resources
        self.release_resources(&task.requirements, &mut resources).await;

        // Update task state
        let mut updated_task = (*task).clone();
        updated_task.state = TaskState::Completed;
        updated_task.updated_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Submit result to chain
        self.submit_result(task, result).await?;

        running.remove(task_id);
        Ok(())
    }

    async fn release_resources(
        &self,
        requirements: &ResourceRequirements,
        pool: &mut ResourcePool
    ) {
        // Release GPU memory
        if requirements.gpu_count > 0 {
            let mut released = 0;
            let required_memory = requirements.memory_gb as u64 * 1024 * 1024 * 1024;
            
            for gpu in &mut pool.gpu_devices {
                if released >= requirements.gpu_count {
                    break;
                }
                
                if gpu.memory_allocated >= required_memory {
                    gpu.memory_allocated -= required_memory;
                    released += 1;
                }
            }
        }

        // Release general memory
        pool.available_memory_gb += requirements.memory_gb as u64;
    }

    async fn submit_result(
        &self,
        task: &ComputeTask,
        result: &str
    ) -> Result<(), TaskManagerError> {
        // Construct transaction
        let instruction = haunti_chain::instruction::submit_result(
            task.owner,
            task.task_id.clone(),
            result.to_string(),
        )?;

        let mut tx = solana_sdk::transaction::Transaction::new_with_payer(
            &[instruction],
            Some(&task.owner),
        );

        // Sign and submit
        let recent_blockhash = self.rpc_client.get_latest_blockhash().await?;
        tx.try_sign(&[/* TODO: Add signer */], recent_blockhash)?;
        self.rpc_client.send_and_confirm_transaction(&tx).await?;

        Ok(())
    }

    pub async fn monitor_timeouts(&self) {
        let mut interval = interval(Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut running = self.running_tasks.write().await;
            let mut to_remove = vec![];

            for (task_id, task) in running.iter() {
                let elapsed = now - task.updated_at;
                if elapsed > task.requirements.timeout_secs as u64 {
                    log::warn!("Task {} timed out", task_id);
                    to_remove.push(task_id.clone());
                }
            }

            for task_id in to_remove {
                if let Some(task) = running.remove(&task_id) {
                    let mut resources = self.resource_pool.write().await;
                    self.release_resources(&task.requirements, &mut resources).await;
                }
            }
        }
    }
}

// Ord implementation for task prioritization
impl Ord for ComputeTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.created_at.cmp(&other.created_at).reverse())
    }
}

impl PartialOrd for ComputeTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ComputeTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for ComputeTask {}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signer::keypair::Keypair;

    #[tokio::test]
    async fn test_task_lifecycle() {
        let manager = TaskManager::new("http://test:8899");
        let owner = Keypair::new().pubkey();
        
        let task = ComputeTask {
            task_id: "test-1".to_string(),
            owner,
            priority: TaskPriority::High,
            requirements: ResourceRequirements {
                gpu_type: Some("V100".to_string()),
                gpu_count: 1,
                memory_gb: 16,
                storage_gb: 50,
                timeout_secs: 3600,
            },
            state: TaskState::Pending,
            created_at: 0,
            updated_at: 0,
            task_type: TaskType::Inference {
                input_cid: "Qm...".to_string(),
            },
            model_cid: "Qm...".to_string(),
            data_cid: "Qm...".to_string(),
        };

        // Test adding task
        manager.add_task(task).await.unwrap();
        assert_eq!(manager.pending_queue.lock().await.len(), 1);

        // TODO: Add more test cases
    }
}
