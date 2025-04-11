//! Core module for Haunti AI on Solana

#![deny(
    warnings,
    missing_docs,
    unused_import_braces,
    unused_qualifications,
    rust_2018_idioms
)]
#![cfg_attr(not(feature = "std"), no_std)]
#![feature(generic_const_exprs)]

use anchor_lang::prelude::*;
use solana_program::entrypoint;

mod compute;
mod encryption;
mod errors;
mod instructions;
mod state;
mod zkml;

// Re-export core functionalities
pub use compute::GPUComputation;
pub use encryption::FHEOperator;
pub use errors::HauntiError;
pub use state::{ModelParams, TaskAccount};
pub use zkml::{ZKProof, ZKVerifier};

declare_id!("HAUNTiCore1111111111111111111111111111111111111");

/// Main program module handling AI task lifecycle
#[program]
pub mod haunti_core {
    use super::*;

    /// Initialize a new AI training task
    pub fn initialize_task(
        ctx: Context<InitializeTask>,
        model_params: ModelParams,
        reward: u64,
    ) -> Result<()> {
        require!(reward > 0, HauntiError::InvalidReward);
        
        let task_account = &mut ctx.accounts.task_account;
        task_account.model = model_params;
        task_account.reward = reward;
        task_account.owner = *ctx.accounts.owner.key;
        task_account.state = TaskState::Pending;

        Ok(())
    }

    /// Submit completed computation with ZK proof
    pub fn submit_computation(
        ctx: Context<SubmitComputation>,
        proof: ZKProof,
        encrypted_output: Vec<u8>,
    ) -> Result<()> {
        let task = &mut ctx.accounts.task_account;
        require!(task.state == TaskState::Pending, HauntiError::TaskNotActive);
        
        // Verify ZK proof
        let verifier = ZKVerifier::new(&task.model)?;
        verifier.verify_proof(&proof)?;

        // Store encrypted result
        let fhe = FHEOperator::from_key(&task.model.fhe_public_key)?;
        task.encrypted_output = fhe.encrypt_result(encrypted_output)?;
        task.state = TaskState::Completed;

        Ok(())
    }

    // Additional handlers for:
    // - Task cancellation
    // - Reward distribution
    // - Model updates
}

/// Account validation structures
#[derive(Accounts)]
pub struct InitializeTask<'info> {
    #[account(init, payer = owner, space = TaskAccount::LEN)]
    pub task_account: Account<'info, TaskAccount>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SubmitComputation<'info> {
    #[account(mut, has_one = owner)]
    pub task_account: Account<'info, TaskAccount>,
    pub owner: Signer<'info>,
}

// GPU-accelerated computation implementation
#[cfg(feature = "gpu-acceleration")]
impl GPUComputation {
    /// Execute model training/inference on CUDA cores
    pub fn cuda_execute(
        &self,
        model: &ModelParams,
        inputs: &[f32],
    ) -> Result<Vec<f32>> {
        // CUDA kernel integration
        unsafe {
            let mut device_output = DeviceBuffer::zeros(model.output_size);
            launch_kernel!(
                model.kernel_template,
                model.blocks,
                model.threads,
                inputs.as_ptr(),
                device_output.as_mut_ptr(),
                model.params.as_ptr()
            )?;
            Ok(device_output.copy_to_host())
        }
    }
}

// Zero-knowledge proof system integration
impl ZKVerifier {
    /// Verify proof against public inputs
    pub fn verify_proof(&self, proof: &ZKProof) -> Result<()> {
        use plonky3::verifier::verify_plonk_proof;
        
        let public_inputs = self.model.get_public_inputs();
        verify_plonk_proof(
            &self.verifier_key,
            proof,
            &public_inputs,
        ).map_err(|e| HauntiError::ProofVerificationFailed.into())
    }
}

// FHE operations using tfhe-rs
impl FHEOperator {
    /// Encrypt computation result with FHE
    pub fn encrypt_result(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        use fhe_rs::prelude::*;
        
        let ciphertext = self.key.encrypt_bytes(&data)
            .map_err(|_| HauntiError::EncryptionFailed)?;
        Ok(ciphertext.to_bytes())
    }
}
