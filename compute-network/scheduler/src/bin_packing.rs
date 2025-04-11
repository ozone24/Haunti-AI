//! Multi-dimensional bin packing algorithm for GPU resource scheduling

use std::collections::{BinaryHeap, HashMap};
use std::cmp::{Ordering, Reverse};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct GpuResource {
    pub id: String,
    pub total_memory: u64,
    pub used_memory: u64,
    pub cuda_cores: u32,
    pub memory_bandwidth: u32,
    pub fp32_perf: f32,
    pub fp16_support: bool,
    pub current_utilization: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputeTask {
    pub task_id: String,
    pub required_memory: u64,
    pub min_cuda_cores: u32,
    pub bandwidth_threshold: u32,
    pub fp16_required: bool,
    pub priority: u8,
}

#[derive(Error, Debug)]
pub enum BinPackError {
    #[error("Insufficient resource for task {0}: {1}")]
    InsufficientResource(String, String),
    
    #[error("Resource conflict: {0}")]
    ResourceConflict(String),
    
    #[error("Scheduler overloaded: {0}")]
    SchedulerOverload(String),
}

#[derive(PartialEq, PartialOrd)]
struct GpuFitnessScore(f32);

impl Eq for GpuFitnessScore {}

impl Ord for GpuFitnessScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

pub trait PackingStrategy {
    fn schedule(
        &self,
        task: &ComputeTask,
        gpu_pool: &mut HashMap<String, GpuResource>,
    ) -> Result<String, BinPackError>;
}

pub struct MultiDimFirstFit;
pub struct BestFitWithScoring;
pub struct HybridEvolutionary;

impl PackingStrategy for MultiDimFirstFit {
    fn schedule(
        &self,
        task: &ComputeTask,
        gpu_pool: &mut HashMap<String, GpuResource>,
    ) -> Result<String, BinPackError> {
        for gpu in gpu_pool.values_mut() {
            if meets_task_requirements(gpu, task) {
                allocate_resources(gpu, task)?;
                return Ok(gpu.id.clone());
            }
        }
        Err(BinPackError::InsufficientResource(
            task.task_id.clone(),
            "No suitable GPU found".into(),
        ))
    }
}

impl PackingStrategy for BestFitWithScoring {
    fn schedule(
        &self,
        task: &ComputeTask,
        gpu_pool: &mut HashMap<String, GpuResource>,
    ) -> Result<String, BinPackError> {
        let mut heap = BinaryHeap::new();
        
        for gpu in gpu_pool.values() {
            if meets_task_requirements(gpu, task) {
                let score = calculate_fitness_score(gpu, task);
                heap.push((Reverse(GpuFitnessScore(score)), gpu.id.clone()));
            }
        }
        
        if let Some((_, gpu_id)) = heap.pop() {
            let gpu = gpu_pool.get_mut(&gpu_id).unwrap();
            allocate_resources(gpu, task)?;
            Ok(gpu_id)
        } else {
            Err(BinPackError::InsufficientResource(
                task.task_id.clone(),
                "No suitable GPU found".into(),
            ))
        }
    }
}

fn meets_task_requirements(gpu: &GpuResource, task: &ComputeTask) -> bool {
    let memory_available = gpu.total_memory - gpu.used_memory;
    let cores_available = gpu.cuda_cores as f32 * (1.0 - gpu.current_utilization);
    
    memory_available >= task.required_memory &&
    cores_available >= task.min_cuda_cores as f32 &&
    gpu.memory_bandwidth >= task.bandwidth_threshold &&
    (!task.fp16_required || gpu.fp16_support)
}

fn calculate_fitness_score(gpu: &GpuResource, task: &ComputeTask) -> f32 {
    let memory_ratio = (gpu.used_memory + task.required_memory) as f32 / gpu.total_memory as f32;
    let core_utilization = (task.min_cuda_cores as f32 / gpu.cuda_cores as f32) * 0.7;
    let bandwidth_utilization = (task.bandwidth_threshold as f32 / gpu.memory_bandwidth as f32) * 0.3;
    
    // Lower score is better
    1.0 / (0.5 * memory_ratio + 0.3 * core_utilization + 0.2 * bandwidth_utilization)
}

fn allocate_resources(gpu: &mut GpuResource, task: &ComputeTask) -> Result<(), BinPackError> {
    if gpu.used_memory + task.required_memory > gpu.total_memory {
        return Err(BinPackError::ResourceConflict(
            format!("Memory exceeded on GPU {}", gpu.id)
        ));
    }
    
    let new_util = gpu.current_utilization + 
        (task.min_cuda_cores as f32 / gpu.cuda_cores as f32);
        
    if new_util > 1.0 {
        return Err(BinPackError::SchedulerOverload(
            format!("GPU {} utilization over 100%", gpu.id)
        ));
    }
    
    gpu.used_memory += task.required_memory;
    gpu.current_utilization = new_util;
    Ok(())
}

pub struct ResourceScheduler {
    strategies: HashMap<&'static str, Box<dyn PackingStrategy>>,
    current_strategy: &'static str,
    gpu_pool: HashMap<String, GpuResource>,
}

impl ResourceScheduler {
    pub fn new(gpus: Vec<GpuResource>) -> Self {
        let mut strategies = HashMap::new();
        strategies.insert("first_fit", Box::new(MultiDimFirstFit) as Box<dyn PackingStrategy>);
        strategies.insert("best_fit", Box::new(BestFitWithScoring));
        
        ResourceScheduler {
            strategies,
            current_strategy: "best_fit",
            gpu_pool: gpus.into_iter().map(|g| (g.id.clone(), g)).collect(),
        }
    }
    
    pub fn schedule_task(
        &mut self,
        task: ComputeTask,
    ) -> Result<String, BinPackError> {
        let strategy = self.strategies
            .get(self.current_strategy)
            .ok_or_else(|| BinPackError::ResourceConflict("Invalid strategy".into()))?;
        
        strategy.schedule(&task, &mut self.gpu_pool)
    }
    
    pub fn add_gpu(&mut self, gpu: GpuResource) {
        self.gpu_pool.insert(gpu.id.clone(), gpu);
    }
    
    pub fn remove_gpu(&mut self, gpu_id: &str) -> Result<(), BinPackError> {
        self.gpu_pool.remove(gpu_id)
            .ok_or_else(|| BinPackError::ResourceConflict("GPU not found".into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_gpu(id: &str) -> GpuResource {
        GpuResource {
            id: id.into(),
            total_memory: 32_768, // 32GB
            used_memory: 0,
            cuda_cores: 10_240,
            memory_bandwidth: 936, // GB/s
            fp32_perf: 30.1, // TFLOPS
            fp16_support: true,
            current_utilization: 0.0,
        }
    }

    #[test]
    fn test_best_fit_allocation() {
        let mut gpus = vec![
            create_test_gpu("gpu1"),
            create_test_gpu("gpu2"),
        ];
        
        gpus[0].used_memory = 16_384; // 16GB used
        
        let mut scheduler = ResourceScheduler::new(gpus);
        let task = ComputeTask {
            task_id: "task1".into(),
            required_memory: 8_192, // 8GB
            min_cuda_cores: 2048,
            bandwidth_threshold: 500,
            fp16_required: true,
            priority: 1,
        };
        
        let result = scheduler.schedule_task(task);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "gpu2");
    }

    #[test]
    fn test_insufficient_memory() {
        let mut scheduler = ResourceScheduler::new(vec![create_test_gpu("gpu1")]);
        let task = ComputeTask {
            task_id: "task1".into(),
            required_memory: 40_960, // 40GB
            min_cuda_cores: 1024,
            bandwidth_threshold: 500,
            fp16_required: false,
            priority: 1,
        };
        
        let result = scheduler.schedule_task(task);
        assert!(matches!(result, Err(BinPackError::InsufficientResource(_, _))));
    }
}
