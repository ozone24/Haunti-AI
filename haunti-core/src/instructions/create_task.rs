//! Instruction handler for creating AI training/inference tasks

use anchor_lang::{
    prelude::*,
    solana_program::{entrypoint::ProgramResult, system_instruction},
};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::program_memory::sol_memcmp;
use crate::{
    error::HauntiError,
    state::{ModelParams, TaskAccount, TaskState},
    utils::validate_model_hash,
};

// Account validation structure
#[derive(Accounts)]
#[instruction(model: ModelParams, reward: u64, time_limit: u64)]
pub struct CreateTask<'info> {
    #[account(
        init,
        payer = owner,
        space = TaskAccount::LEN,
        seeds = [b"task", owner.key().as_ref(), model.model_hash.as_ref()],
        bump
    )]
    pub task_account: Account<'info, TaskAccount>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
    
    // Optional: GPU resource provider account
    #[account(
        constraint = gpu_provider.map(|acc| acc.is_approved).unwrap_or(true),
        signer @ HauntiError::MissingProviderSignature
    )]
    pub gpu_provider: Option<Account<'info, GpuProvider>>,
}

// Instruction handler implementation
impl<'info> CreateTask<'info> {
    pub fn execute(
        &mut self,
        model: ModelParams,
        reward: u64,
        time_limit: u64,
        encrypted_data: Option<Vec<u8>>,
    ) -> ProgramResult {
        // Validate input parameters
        self.validate_inputs(&model, reward, time_limit)?;
        
        // Initialize task account
        let task = &mut self.task_account;
        task.owner = self.owner.key();
        task.model = model;
        task.reward = reward;
        task.time_limit = time_limit;
        task.state = TaskState::Pending;
        task.encrypted_input = encrypted_data.unwrap_or_default();
        task.created_at = Clock::get()?.unix_timestamp;
        
        // Deduct deposit from owner
        self.transfer_deposit(reward)?;
        
        // Emit creation event
        emit!(TaskCreated {
            owner: self.owner.key(),
            model_hash: task.model.model_hash.clone(),
            reward,
            timestamp: task.created_at
        });
        
        Ok(())
    }

    fn validate_inputs(
        &self,
        model: &ModelParams,
        reward: u64,
        time_limit: u64,
    ) -> Result<()> {
        // Model hash validation
        require!(
            validate_model_hash(&model.model_hash),
            HauntiError::InvalidModelHash
        );
        
        // Reward sanity check
        require!(reward >= MINIMUM_REWARD, HauntiError::RewardTooLow);
        require!(reward <= MAXIMUM_REWARD, HauntiError::RewardTooHigh);
        
        // Time constraints
        require!(
            time_limit >= MIN_TIME_LIMIT && time_limit <= MAX_TIME_LIMIT,
            HauntiError::InvalidTimeLimit
        );
        
        // GPU provider verification
        if let Some(provider) = &self.gpu_provider {
            require!(
                provider.supports_model_type(model.model_type),
                HauntiError::UnsupportedModelType
            );
        }
        
        Ok(())
    }

    fn transfer_deposit(&self, amount: u64) -> Result<()> {
        let transfer_ix = system_instruction::transfer(
            &self.owner.key(),
            &self.task_account.key(),
            amount,
        );
        
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_ix,
            &[
                self.owner.to_account_info(),
                self.task_account.to_account_info(),
                self.system_program.to_account_info(),
            ],
            &[],
        )?;
        
        Ok(())
    }
}

// Event logging
#[event]
pub struct TaskCreated {
    pub owner: Pubkey,
    pub model_hash: [u8; 32],
    pub reward: u64,
    pub timestamp: i64,
}

// Constants
const MINIMUM_REWARD: u64 = 100_000; // 0.0001 SOL
const MAXIMUM_REWARD: u64 = 100_000_000_000; // 100 SOL
const MIN_TIME_LIMIT: u64 = 300; // 5 minutes
const MAX_TIME_LIMIT: u64 = 2592000; // 30 days

// Security-critical memory cleanup
impl Drop for CreateTask<'_> {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.task_account.model.parameters.zeroize();
    }
}
