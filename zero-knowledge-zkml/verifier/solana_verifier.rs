//! On-chain verifier program for Haunti AI compute tasks

use anchor_lang::{
    prelude::*,
    solana_program::{
        program::invoke_signed,
        sysvar::instructions::load_instruction_at_checked,
    },
};
use anchor_spl::token::{self, Token, TokenAccount};
use haunti_errors::VerifierError;
use haunti_utils::{
    zk::verify_plonky3_proof,
    cpi_context::CrossProgramInvocationContext,
    serialization::deserialize_proof,
};

declare_id!("HaunVrfy111111111111111111111111111111111111");

#[program]
pub mod solana_verifier {
    use super::*;

    /// Verifies a ZK proof for AI compute tasks and triggers rewards
    /// Accounts:
    /// 0. [WRITE] verification_result: PDA to store verification status
    /// 1. [SIGNER] authority: Task submitter
    /// 2. [EXEC] compute_budget: CPI to request CU increase
    /// 3. [] task_account: Source task data
    /// 4. [] model_account: Verified model metadata
    /// 5. [] reward_vault: Token vault for staking rewards
    /// 6. [] system_program: System program
    pub fn verify_ai_proof(
        ctx: Context<VerifyAIProof>,
        proof_data: Vec<u8>,
        public_inputs: Vec<[u8; 32]>,
    ) -> Result<()> {
        // --- Phase 1: Security Checks ---
        // Validate proof data length (prevent DoS)
        require!(
            proof_data.len() <= 1024 * 128, // 128KB max
            VerifierError::InvalidProofDataLength
        );

        // CPI security: Ensure compute_budget is official program
        let compute_budget = &ctx.accounts.compute_budget;
        let compute_budget_info = load_instruction_at_checked(0, &ctx.accounts.to_account_infos())?;
        require!(
            compute_budget_info.program_id == solana_program::compute_budget::id(),
            VerifierError::UnauthorizedCpi
        );

        // --- Phase 2: Proof Verification ---
        let proof = deserialize_proof(&proof_data)?;
        let verification_result = verify_plonky3_proof(
            &proof,
            &public_inputs,
            &ctx.accounts.model_account.model_hash,
        )?;

        // --- Phase 3: State Update & Rewards ---
        let verification_account = &mut ctx.accounts.verification_result;
        verification_account.status = VerificationStatus::Verified;
        verification_account.slot = Clock::get()?.slot;
        verification_account.verifier = ctx.accounts.authority.key();

        // Transfer rewards from vault to submitter
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.reward_vault.to_account_info(),
                to: ctx.accounts.authority_token_account.to_account_info(),
                authority: ctx.accounts.reward_vault_authority.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, verification_result.reward_amount)?;

        // --- Phase 4: Compute Budget Management ---
        // Request additional CU for heavy verification logic
        let cu_ix = solana_program::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(200_000);
        invoke_signed(
            &cu_ix,
            &[
                ctx.accounts.compute_budget.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[],
        )?;

        Ok(())
    }

    /// Handles proof verification for FHE-encrypted results
    /// Accounts:
    /// 0. [WRITE] fhe_result_account: Encrypted result storage
    /// 1. [SIGNER] validator: Node operator
    pub fn verify_fhe_compute(ctx: Context<VerifyFHE>, ciphertext: Vec<u8>) -> Result<()> {
        // Implementation uses external FHE lib via CPI
        // ...
    }
}

// Accounts ========================

#[derive(Accounts)]
pub struct VerifyAIProof<'info> {
    #[account(mut, seeds = [b"verification"], bump)]
    pub verification_result: Account<'info, VerificationState>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(executable, constraint = compute_budget.key() == compute_budget::id())]
    pub compute_budget: AccountInfo<'info>,
    
    #[account(has_one = reward_vault)]
    pub task_account: Account<'info, TaskState>,
    
    #[account(constraint = model_account.owner == haunti_nft::id())]
    pub model_account: Account<'info, ModelState>,
    
    #[account(mut)]
    pub reward_vault: Account<'info, TokenAccount>,
    
    #[account(constraint = reward_vault_authority.key() == task_account.reward_authority)]
    pub reward_vault_authority: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[account]
#[derive(Default)]
pub struct VerificationState {
    pub status: VerificationStatus,
    pub slot: u64,
    pub verifier: Pubkey,
    pub reward_amount: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum VerificationStatus {
    Pending,
    Verified,
    Failed,
}

// Errors ==========================

#[error_code]
pub enum VerifierError {
    #[msg("Proof data exceeds maximum allowed size")]
    InvalidProofDataLength,
    #[msg("Unauthorized CPI invocation detected")]
    UnauthorizedCpi,
    #[msg("FHE ciphertext validation failed")]
    FheValidationFailure,
}
