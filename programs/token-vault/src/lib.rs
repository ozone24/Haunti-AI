//! Token Vault Program: Staking, Reward Distribution & Governance

use anchor_lang::{
    prelude::*,
    solana_program::{
        clock,
        program::{invoke, invoke_signed},
        system_instruction,
    },
};
use anchor_spl::{
    token::{self, Mint, Token, TokenAccount, Transfer},
    associated_token::AssociatedToken,
};
use std::convert::TryInto;

declare_id!("HAUNTVAU1111111111111111111111111111111111");

#[program]
pub mod token_vault {
    use super::*;

    /// Initialize a new staking pool
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_type: PoolType,
        reward_rate: u64,
        lockup_period: i64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.version = 1;
        pool.pool_type = pool_type;
        pool.reward_rate = reward_rate;
        pool.lockup_period = lockup_period;
        pool.total_staked = 0;
        pool.reward_reserve = 0;
        pool.bump = *ctx.bumps.get("pool").unwrap();
        pool.last_update = clock::Clock::get()?.unix_timestamp;
        
        emit!(PoolEvent::PoolInitialized {
            pool: pool.key(),
            timestamp: pool.last_update,
        });
        
        Ok(())
    }

    /// Stake tokens into the pool
    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let user = &mut ctx.accounts.user_stake;
        
        // Transfer tokens to vault
        let transfer_ix = Transfer {
            from: ctx.accounts.user_token.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_ix,
        );
        token::transfer(cpi_ctx, amount)?;

        // Update stake records
        user.amount += amount;
        user.last_staked = clock::Clock::get()?.unix_timestamp;
        pool.total_staked += amount;

        emit!(PoolEvent::Staked {
            user: user.key(),
            amount,
            timestamp: user.last_staked,
        });
        
        Ok(())
    }

    /// Unstake tokens with optional penalty
    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let user = &mut ctx.accounts.user_stake;
        let now = clock::Clock::get()?.unix_timestamp;
        
        require!(
            now >= user.last_staked + pool.lockup_period,
            VaultError::LockupActive
        );
        
        // Calculate rewards first
        let rewards = calculate_rewards(user, pool, now)?;
        if rewards > 0 {
            distribute_rewards(ctx.accounts, rewards)?;
        }

        // Transfer tokens back
        let transfer_ix = Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.user_token.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        
        let seeds = &[b"pool", &[pool.bump]];
        let signer = &[&seeds[..]];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_ix,
            signer,
        );
        token::transfer(cpi_ctx, amount)?;

        // Update records
        user.amount -= amount;
        pool.total_staked -= amount;

        emit!(PoolEvent::Unstaked {
            user: user.key(),
            amount,
            timestamp: now,
        });
        
        Ok(())
    }

    /// Claim accumulated rewards
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let user = &mut ctx.accounts.user_stake;
        let now = clock::Clock::get()?.unix_timestamp;
        
        let rewards = calculate_rewards(user, pool, now)?;
        require!(rewards > 0, VaultError::NoRewardsAvailable);
        
        distribute_rewards(ctx.accounts, rewards)?;
        
        user.last_reward = now;
        pool.reward_reserve -= rewards;
        
        emit!(PoolEvent::RewardClaimed {
            user: user.key(),
            amount: rewards,
            timestamp: now,
        });
        
        Ok(())
    }

    /// Governance: Create a new proposal
    pub fn create_proposal(
        ctx: Context<CreateProposal>,
        proposal_type: ProposalType,
        amount: Option<u64>,
        recipient: Option<Pubkey>,
    ) -> Result<()> {
        let proposal = &mut ctx.accounts.proposal;
        proposal.proposer = *ctx.accounts.owner.key;
        proposal.proposal_type = proposal_type;
        proposal.amount = amount;
        proposal.recipient = recipient;
        proposal.votes_for = 0;
        proposal.votes_against = 0;
        proposal.created_at = clock::Clock::get()?.unix_timestamp;
        proposal.status = ProposalStatus::Active;
        
        emit!(GovernanceEvent::ProposalCreated {
            proposal: proposal.key(),
            proposer: proposal.proposer,
            timestamp: proposal.created_at,
        });
        
        Ok(())
    }

    /// Governance: Vote on a proposal
    pub fn vote(ctx: Context<Vote>, approve: bool) -> Result<()> {
        let proposal = &mut ctx.accounts.proposal;
        let stake = &ctx.accounts.user_stake;
        
        require!(
            proposal.status == ProposalStatus::Active,
            VaultError::ProposalNotActive
        );
        require!(
            stake.amount >= MIN_VOTING_STAKE,
            VaultError::InsufficientVotingPower
        );
        
        if approve {
            proposal.votes_for += stake.amount;
        } else {
            proposal.votes_against += stake.amount;
        }
        
        emit!(GovernanceEvent::VoteCast {
            proposal: proposal.key(),
            voter: stake.key(),
            amount: stake.amount,
            approve,
            timestamp: clock::Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = authority,
        space = PoolState::LEN,
        seeds = [b"pool", pool_type.to_string().as_bytes()],
        bump,
    )]
    pub pool: Account<'info, PoolState>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        init,
        payer = authority,
        token::mint = mint,
        token::authority = pool,
        seeds = [b"vault", pool.key().as_ref()],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,
    
    pub mint: Account<'info, Mint>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub pool: Account<'info, PoolState>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = owner,
    )]
    pub user_token: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = owner,
        space = UserStake::LEN,
        seeds = [b"stake", pool.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub user_stake: Account<'info, UserStake>,
    
    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    
    pub mint: Account<'info, Mint>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    // Similar to Stake with additional time checks
}

#[account]
pub struct PoolState {
    pub version: u8,
    pub pool_type: PoolType,
    pub reward_rate: u64,
    pub lockup_period: i64,
    pub total_staked: u64,
    pub reward_reserve: u64,
    pub bump: u8,
    pub last_update: i64,
}

#[account]
pub struct UserStake {
    pub amount: u64,
    pub last_staked: i64,
    pub last_reward: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum PoolType {
    GPUProvider,
    Validator,
    Trainer,
    Governance,
}

#[error_code]
pub enum VaultError {
    #[msg("Lockup period not expired")]
    LockupActive,
    #[msg("Insufficient staked amount")]
    InsufficientStake,
    #[msg("No rewards available")]
    NoRewardsAvailable,
    #[msg("Proposal no longer active")]
    ProposalNotActive,
    #[msg("Minimum voting stake not met")]
    InsufficientVotingPower,
    #[msg("Invalid reward distribution")]
    InvalidRewardCalc,
}

#[event]
pub enum PoolEvent {
    PoolInitialized {
        pool: Pubkey,
        timestamp: i64,
    },
    Staked {
        user: Pubkey,
        amount: u64,
        timestamp: i64,
    },
    Unstaked {
        user: Pubkey,
        amount: u64,
        timestamp: i64,
    },
    RewardClaimed {
        user: Pubkey,
        amount: u64,
        timestamp: i64,
    },
}

// Helper functions
fn calculate_rewards(user: &UserStake, pool: &PoolState, now: i64) -> Result<u64> {
    let duration = now - user.last_reward;
    if duration <= 0 || pool.reward_rate == 0 {
        return Ok(0);
    }
    
    let reward = user.amount
        .checked_mul(pool.reward_rate)
        .and_then(|r| r.checked_mul(duration.try_into().unwrap()))
        .ok_or(VaultError::InvalidRewardCalc)?;
    
    Ok(reward / 1_000_000) // Normalize by precision factor
}

fn distribute_rewards(ctx: &mut ClaimRewards, amount: u64) -> Result<()> {
    let transfer_ix = Transfer {
        from: ctx.reward_vault.to_account_info(),
        to: ctx.user_token.to_account_info(),
        authority: ctx.pool.to_account_info(),
    };
    
    let seeds = &[b"pool", &[ctx.pool.bump]];
    let signer = &[&seeds[..]];
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.token_program.to_account_info(),
        transfer_ix,
        signer,
    );
    token::transfer(cpi_ctx, amount)
}
