//! Task state machine and account definitions

use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock;
use borsh::{BorshDeserialize, BorshSerialize};
use std::convert::TryFrom;

/// Task lifecycle states
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub enum TaskStatus {
    /// Task created but not yet assigned
    Pending,
    /// Resources allocated, computation ongoing
    Running {
        worker: Pubkey,
        started_at: i64,
        last_heartbeat: i64,
    },
    /// Successfully completed with proof
    Completed {
        result_hash: [u8; 32],
        completed_at: i64,
    },
    /// Failed with error code
    Failed {
        error_code: u32,
        failed_at: i64,
    },
    /// Cancelled by owner
    Cancelled {
        cancelled_at: i64,
    },
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Core task account storing execution metadata
#[account]
#[derive(Default)]
pub struct TaskState {
    /// Bump seed for PDA
    pub bump: u8,
    /// Task creation unix timestamp
    pub created_at: i64,
    /// Task creator authority
    pub owner: Pubkey,
    /// Current status
    pub status: TaskStatus,
    /// Hash of encrypted input data
    pub input_hash: [u8; 32],
    /// Hash of expected model version
    pub model_hash: [u8; 32],
    /// Allocated compute units (CU)
    pub allocated_cu: u64,
    /// Remaining compute units (CU)
    pub remaining_cu: u64,
    /// Proof verification timestamp
    pub verified_at: Option<i64>,
    /// Associated Model NFT
    pub model_mint: Option<Pubkey>,
    /// Version counter for optimistic concurrency
    pub version: u64,
}

impl TaskState {
    /// Account space calculation
    pub const LEN: usize = 8 + // discriminator
        1 +  // bump
        8 +  // created_at
        32 + // owner
        TaskStatus::LEN + 
        32 + // input_hash
        32 + // model_hash
        8 +  // allocated_cu
        8 +  // remaining_cu
        1 + 8 + // verified_at (option)
        1 + 32 + // model_mint (option)
        8; // version

    /// Transition task to running state
    pub fn start(
        &mut self,
        worker: Pubkey,
    ) -> Result<()> {
        require!(
            matches!(self.status, TaskStatus::Pending),
            TaskError::InvalidStateTransition
        );
        
        let clock = clock::Clock::get()?;
        self.status = TaskStatus::Running {
            worker,
            started_at: clock.unix_timestamp,
            last_heartbeat: clock.unix_timestamp,
        };
        self.version = self.version.wrapping_add(1);
        
        Ok(())
    }

    /// Update progress of running task
    pub fn update_progress(
        &mut self,
        remaining_cu: u64,
    ) -> Result<()> {
        match &mut self.status {
            TaskStatus::Running {
                ref mut last_heartbeat,
                ..
            } => {
                let clock = clock::Clock::get()?;
                *last_heartbeat = clock.unix_timestamp;
                self.remaining_cu = remaining_cu;
                self.version = self.version.wrapping_add(1);
                Ok(())
            }
            _ => Err(TaskError::InvalidStateTransition.into()),
        }
    }

    /// Complete task with final proof
    pub fn complete(
        &mut self,
        result_hash: [u8; 32],
    ) -> Result<()> {
        require!(
            matches!(self.status, TaskStatus::Running { .. }),
            TaskError::InvalidStateTransition
        );

        let clock = clock::Clock::get()?;
        self.status = TaskStatus::Completed {
            result_hash,
            completed_at: clock.unix_timestamp,
        };
        self.verified_at = Some(clock.unix_timestamp);
        self.version = self.version.wrapping_add(1);

        Ok(())
    }

    /// Mark task as failed
    pub fn fail(
        &mut self,
        error_code: u32,
    ) -> Result<()> {
        require!(
            matches!(self.status, TaskStatus::Running { .. }),
            TaskError::InvalidStateTransition
        );

        let clock = clock::Clock::get()?;
        self.status = TaskStatus::Failed {
            error_code,
            failed_at: clock.unix_timestamp,
        };
        self.version = self.version.wrapping_add(1);

        Ok(())
    }

    /// Cancel pending task
    pub fn cancel(&mut self) -> Result<()> {
        require!(
            matches!(self.status, TaskStatus::Pending),
            TaskError::InvalidStateTransition
        );

        let clock = clock::Clock::get()?;
        self.status = TaskStatus::Cancelled {
            cancelled_at: clock.unix_timestamp,
        };
        self.version = self.version.wrapping_add(1);

        Ok(())
    }

    /// Validate authority for state transitions
    pub fn validate_authority(&self, authority: &Pubkey) -> Result<()> {
        match self.status {
            TaskStatus::Running { ref worker, .. } => {
                require!(
                    worker == authority,
                    TaskError::Unauthorized
                )
            }
            _ => require!(
                self.owner == *authority,
                TaskError::Unauthorized
            ),
        }
        Ok(())
    }
}

impl TaskStatus {
    /// Calculate max serialized size
    pub const LEN: usize = 1 + // variant tag
        32 + 8 + 8; // Running state fields (worker + timestamps)
}

/// Task state change events
#[event]
pub struct TaskStatusChanged {
    pub task: Pubkey,
    pub old_status: TaskStatus,
    pub new_status: TaskStatus,
    pub version: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum TaskError {
    #[msg("Invalid state transition")]
    InvalidStateTransition,
    #[msg("Unauthorized operation")]
    Unauthorized,
    #[msg("Heartbeat timeout")]
    HeartbeatTimeout,
    #[msg("Compute unit exhausted")]
    ComputeUnitExhausted,
    #[msg("Model hash mismatch")]
    ModelHashMismatch,
}
