//! Encrypted training program using FHE for privacy-preserving AI

use anchor_lang::{
    prelude::*,
    solana_program::{
        program::{invoke, invoke_signed},
        sysvar::instructions,
    },
};
use anchor_spl::token::{self, Token, TokenAccount};
use haunti_utils::{
    fhe::{FheCiphertext, FhePublicKey, FheContext},
    serialization::EncodedVector,
};
use std::convert::TryInto;

declare_id!("HaunFHE111111111111111111111111111111111111");

#[program]
pub mod encrypted_trainer {
    use super::*;

    /// Initializes a new encrypted training task
    /// Accounts:
    /// 0. [WRITE] training_task: PDA for task state
    /// 1. [SIGNER] creator: Task owner
    /// 2. [] model_account: Base model NFT
    /// 3. [] fhe_params: Global FHE parameters
    pub fn create_encrypted_task(
        ctx: Context<CreateEncryptedTask>,
        epochs: u32,
        batch_size: u16,
    ) -> Result<()> {
        let task = &mut ctx.accounts.training_task;
        task.creator = ctx.accounts.creator.key();
        task.model = ctx.accounts.model_account.key();
        task.fhe_pubkey = ctx.accounts.fhe_params.public_key.clone();
        task.status = TrainingStatus::Initialized;
        task.epochs_completed = 0;
        
        // Validate FHE compatibility
        require!(
            ctx.accounts.model_account.fhe_supported,
            TrainerError::FheNotSupported
        );
        
        Ok(())
    }

    /// Processes encrypted training data batch
    /// Accounts:
    /// 0. [WRITE] training_task: Task state
    /// 1. [SIGNER] data_provider: Data owner
    /// 2. [WRITE] encrypted_data: Encrypted dataset account
    pub fn process_encrypted_batch(
        ctx: Context<ProcessEncryptedBatch>,
        ciphertexts: Vec<EncodedVector>,
    ) -> Result<()> {
        let task = &mut ctx.accounts.training_task;
        
        // 1. Validate task phase
        require!(
            task.status == TrainingStatus::Training,
            TrainerError::InvalidTaskState
        );
        
        // 2. Verify encrypted data ownership
        let data_hash = compute_ciphertext_hash(&ciphertexts);
        require!(
            ctx.accounts.encrypted_data.data_hash == data_hash,
            TrainerError::DataHashMismatch
        );
        
        // 3. Execute FHE operations (simplified)
        let updated_weights = fhe_linear_layer_forward(
            &task.current_weights,
            &ciphertexts,
            &ctx.accounts.fhe_params
        )?;
        
        // 4. Update task state
        task.current_weights = updated_weights;
        task.batches_processed += 1;
        
        Ok(())
    }

    /// Finalizes training and generates ZK proof
    /// Accounts:
    /// 0. [WRITE] training_task: Task state
    /// 1. [SIGNER] creator: Task owner
    /// 2. [WRITE] trained_model: Output model account
    pub fn finalize_training(ctx: Context<FinalizeTraining>) -> Result<()> {
        let task = &mut ctx.accounts.training_task;
        
        // 1. Validate completion criteria
        require!(
            task.epochs_completed >= task.epochs,
            TrainerError::TrainingIncomplete
        );
        
        // 2. Generate training proof
        let proof = generate_training_proof(
            &task.initial_weights,
            &task.current_weights,
            &task.fhe_pubkey
        )?;
        
        // 3. Initialize trained model
        let trained_model = &mut ctx.accounts.trained_model;
        trained_model.weights = task.current_weights.clone();
        trained_model.proof = proof;
        trained_model.training_task = task.key();
        
        // 4. Update task status
        task.status = TrainingStatus::Completed;
        
        Ok(())
    }
}

// Accounts ========================

#[derive(Accounts)]
pub struct CreateEncryptedTask<'info> {
    #[account(
        init,
        payer = creator,
        space = 512,
        seeds = [b"encrypted_task", creator.key().as_ref(), model_account.key().as_ref()],
        bump
    )]
    pub training_task: Account<'info, EncryptedTrainingTask>,
    
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        constraint = model_account.owner == haunti_nft::id(),
        constraint = model_account.encrypted_training
    )]
    pub model_account: Account<'info, ModelState>,
    
    #[account(executable, address = haunti_fhe::id())]
    pub fhe_params: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ProcessEncryptedBatch<'info> {
    #[account(mut, has_one = fhe_params)]
    pub training_task: Account<'info, EncryptedTrainingTask>,
    
    #[account(
        signer,
        constraint = encrypted_data.owner == data_provider.key()
    )]
    pub data_provider: Signer<'info>,
    
    #[account(
        mut,
        constraint = encrypted_data.training_task == training_task.key()
    )]
    pub encrypted_data: Account<'info, EncryptedDataSet>,
    
    #[account(address = haunti_fhe::id())]
    pub fhe_params: AccountInfo<'info>,
}

// States ==========================

#[account]
#[derive(Default)]
pub struct EncryptedTrainingTask {
    pub creator: Pubkey,
    pub model: Pubkey,
    pub status: TrainingStatus,
    pub fhe_pubkey: Vec<u8>,
    pub current_weights: Vec<u8>,
    pub initial_weights: Vec<u8>,
    pub epochs: u32,
    pub epochs_completed: u32,
    pub batches_processed: u32,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum TrainingStatus {
    Initialized,
    Training,
    Completed,
    Failed,
}

#[account]
pub struct EncryptedDataSet {
    pub owner: Pubkey,
    pub training_task: Pubkey,
    pub data_hash: [u8; 32],
    pub ciphertexts: Vec<EncodedVector>,
}

// Errors ==========================

#[error_code]
pub enum TrainerError {
    #[msg("FHE operations not supported by this model")]
    FheNotSupported,
    #[msg("Invalid training task state for this operation")]
    InvalidTaskState,
    #[msg("Training proof generation failed")]
    ProofGenerationFailed,
    #[msg("Encrypted data hash mismatch")]
    DataHashMismatch,
    #[msg("Minimum epochs not completed")]
    TrainingIncomplete,
}

// FHE Operations =================

fn fhe_linear_layer_forward(
    weights: &[u8],
    inputs: &[EncodedVector],
    params: &AccountInfo,
) -> Result<Vec<u8>> {
    // Implementation would call FHE processor via CPI
    // This is a simplified placeholder
    Ok(weights.to_vec())
}

fn generate_training_proof(
    initial_weights: &[u8],
    final_weights: &[u8],
    pubkey: &[u8],
) -> Result<Vec<u8>> {
    // ZK proof generation via cross-program invocation
    // ...
    Ok(vec![])
}
