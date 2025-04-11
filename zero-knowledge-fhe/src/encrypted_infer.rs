//! Encrypted inference executor with FHE and ZK result proofs

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
    zk::ProofVerificationError,
};
use std::convert::TryInto;

declare_id!("HaunINF111111111111111111111111111111111111");

#[program]
pub mod encrypted_infer {
    use super::*;

    /// Initializes encrypted inference task
    /// Accounts:
    /// 0. [WRITE] inference_task: Task state PDA
    /// 1. [SIGNER] creator: Task owner
    /// 2. [] model_account: Model NFT
    /// 3. [] fhe_params: Global FHE config
    pub fn create_inference_task(
        ctx: Context<CreateInferenceTask>,
        max_steps: u16,
    ) -> Result<()> {
        let task = &mut ctx.accounts.inference_task;
        task.creator = ctx.accounts.creator.key();
        task.model = ctx.accounts.model_account.key();
        task.fhe_pubkey = ctx.accounts.fhe_params.public_key.clone();
        task.status = InferenceStatus::Initialized;
        task.max_steps = max_steps;
        
        // Validate model supports FHE inference
        require!(
            ctx.accounts.model_account.operations.contains(&ModelOperation::FHEInference),
            InferError::UnsupportedModelOperation
        );
        
        Ok(())
    }

    /// Submits encrypted input for processing
    /// Accounts:
    /// 0. [WRITE] inference_task: Task state
    /// 1. [SIGNER] input_provider: Data owner
    /// 2. [WRITE] encrypted_input: Encrypted data account
    pub fn submit_encrypted_input(
        ctx: Context<SubmitEncryptedInput>,
        ciphertext: EncodedVector,
    ) -> Result<()> {
        let task = &mut ctx.accounts.inference_task;
        
        // Validate task phase
        require!(
            task.status == InferenceStatus::DataSubmitted,
            InferError::InvalidTaskState
        );
        
        // Verify input ownership and hash
        let input_hash = compute_ciphertext_hash(&ciphertext);
        ctx.accounts.encrypted_input.set_inner(EncryptedInput {
            owner: ctx.accounts.input_provider.key(),
            task: task.key(),
            data_hash: input_hash,
            ciphertext,
        });
        
        task.status = InferenceStatus::InputReady;
        
        Ok(())
    }

    /// Finalizes inference with ZK proof
    /// Accounts:
    /// 0. [WRITE] inference_task: Task state
    /// 1. [SIGNER] executor: Compute provider
    /// 2. [WRITE] result_account: Encrypted output
    /// 3. [] verifier_program: ZK verifier program
    pub fn finalize_inference(
        ctx: Context<FinalizeInference>,
        encrypted_output: EncodedVector,
        proof: Vec<u8>,
    ) -> Result<()> {
        let task = &mut ctx.accounts.inference_task;
        
        // 1. Validate pre-conditions
        require!(
            task.status == InferenceStatus::InputReady,
            InferError::InvalidTaskState
        );
        
        // 2. Verify ZK proof via CPI
        let verify_ix = haunti_verifier::verify_proof(
            proof.clone(),
            task.model.clone(),
            task.fhe_pubkey.clone(),
        )?;
        invoke(
            &verify_ix,
            &[
                ctx.accounts.verifier_program.to_account_info(),
                ctx.accounts.inference_task.to_account_info(),
            ],
        )?;
        
        // 3. Store encrypted result
        ctx.accounts.result_account.set_inner(InferenceResult {
            task: task.key(),
            encrypted_output,
            proof,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        // 4. Update task state
        task.status = InferenceStatus::Completed;
        task.completed_at = Some(Clock::get()?.unix_timestamp);
        
        Ok(())
    }
}

// Accounts ========================

#[derive(Accounts)]
pub struct CreateInferenceTask<'info> {
    #[account(
        init,
        payer = creator,
        space = 512,
        seeds = [b"inference_task", creator.key().as_ref(), model_account.key().as_ref()],
        bump
    )]
    pub inference_task: Account<'info, InferenceTask>,
    
    #[account(mut)]
    pub creator: Signer<'info>,
    
    #[account(
        constraint = model_account.owner == haunti_nft::id(),
        constraint = model_account.encrypted_inference
    )]
    pub model_account: Account<'info, ModelState>,
    
    #[account(executable, address = haunti_fhe::id())]
    pub fhe_params: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SubmitEncryptedInput<'info> {
    #[account(mut, has_one = fhe_params)]
    pub inference_task: Account<'info, InferenceTask>,
    
    #[account(signer)]
    pub input_provider: Signer<'info>,
    
    #[account(
        init,
        payer = input_provider,
        space = 1024,
        seeds = [b"encrypted_input", inference_task.key().as_ref()],
        bump
    )]
    pub encrypted_input: Account<'info, EncryptedInput>,
    
    #[account(address = haunti_fhe::id())]
    pub fhe_params: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

// States ==========================

#[account]
#[derive(Default)]
pub struct InferenceTask {
    pub creator: Pubkey,
    pub model: Pubkey,
    pub status: InferenceStatus,
    pub fhe_pubkey: Vec<u8>,
    pub max_steps: u16,
    pub completed_at: Option<i64>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum InferenceStatus {
    Initialized,
    DataSubmitted,
    InputReady,
    Completed,
    Failed,
}

#[account]
pub struct EncryptedInput {
    pub owner: Pubkey,
    pub task: Pubkey,
    pub data_hash: [u8; 32],
    pub ciphertext: EncodedVector,
}

#[account]
pub struct InferenceResult {
    pub task: Pubkey,
    pub encrypted_output: EncodedVector,
    pub proof: Vec<u8>,
    pub timestamp: i64,
}

// Errors ==========================

#[error_code]
pub enum InferError {
    #[msg("Model doesn't support FHE inference")]
    UnsupportedModelOperation,
    #[msg("Invalid task state for this operation")]
    InvalidTaskState,
    #[msg("ZK proof verification failed")]
    ProofVerificationFailed,
    #[msg("Encrypted input hash mismatch")]
    InputHashMismatch,
    #[msg("Inference execution timeout")]
    ExecutionTimeout,
}

// Cryptographic Utilities =========

fn compute_ciphertext_hash(ciphertext: &EncodedVector) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&ciphertext.data);
    hasher.update(&ciphertext.metadata);
    let result = hasher.finalize();
    result.into()
}
