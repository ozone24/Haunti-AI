//! Model state management with version control and cryptographic proofs

use anchor_lang::{
    prelude::*,
    solana_program::{program_pack::IsInitialized, sysvar},
};
use borsh::{BorshDeserialize, BorshSerialize};
use std::convert::TryFrom;

/// Model lifecycle states
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub enum ModelStatus {
    /// New model initialized, not yet active
    PendingTraining,
    /// Active and available for inference
    Active {
        last_inference: Option<i64>,
        inference_count: u64,
    },
    /// Archived and immutable
    Archived,
    /// Deprecated with migration target
    Deprecated {
        successor: Option<Pubkey>,
    },
}

impl Default for ModelStatus {
    fn default() -> Self {
        Self::PendingTraining
    }
}

/// Model metadata account (PDA-based)
#[account]
#[derive(Default)]
pub struct ModelState {
    /// Bump seed for PDA
    pub bump: u8,
    /// Model owner authority
    pub owner: Pubkey,
    /// Current status
    pub status: ModelStatus,
    /// Model version (monotonically increasing)
    pub version: u32,
    /// Merkle root of model parameters
    pub model_root: [u8; 32],
    /// Hash of training dataset
    pub dataset_hash: [u8; 32],
    /// Encryption parameters (FHE scheme config)
    pub fhe_params: Vec<u8>,
    /// ZK proof system parameters
    pub zk_params: Vec<u8>,
    /// IPFS CID for encrypted parameters
    pub storage_cid: String,
    /// Last update timestamp
    pub updated_at: i64,
    /// Version counter for optimistic locking
    pub revision: u64,
}

impl ModelState {
    /// Account space calculation (adjustable via realloc)
    pub const BASE_LEN: usize = 8 + // discriminator
        1 +  // bump
        32 + // owner
        ModelStatus::LEN +
        4 +  // version
        32 + // model_root
        32 + // dataset_hash
        8 +  // updated_at
        8;   // revision

    /// Initialize new model with cryptographic proofs
    pub fn initialize(
        &mut self,
        owner: Pubkey,
        model_root: [u8; 32],
        dataset_hash: [u8; 32],
        fhe_params: Vec<u8>,
        zk_params: Vec<u8>,
        storage_cid: String,
    ) -> Result<()> {
        require!(!self.is_initialized(), ModelError::AlreadyInitialized);
        require_eq!(self.version, 0, ModelError::VersionMismatch);
        
        let clock = sysvar::clock::Clock::get()?;
        self.owner = owner;
        self.model_root = model_root;
        self.dataset_hash = dataset_hash;
        self.fhe_params = fhe_params;
        self.zk_params = zk_params;
        self.storage_cid = storage_cid;
        self.updated_at = clock.unix_timestamp;
        self.status = ModelStatus::PendingTraining;
        self.revision = 1;

        Ok(())
    }

    /// Update model parameters with version control
    pub fn update_model(
        &mut self,
        new_root: [u8; 32],
        new_cid: String,
        signature: &[u8],
    ) -> Result<()> {
        require!(
            matches!(self.status, ModelStatus::Active { .. }),
            ModelError::InvalidState
        );
        self.verify_owner_signature(new_root, signature)?;

        let clock = sysvar::clock::Clock::get()?;
        self.model_root = new_root;
        self.storage_cid = new_cid;
        self.version = self.version.wrapping_add(1);
        self.updated_at = clock.unix_timestamp;
        self.revision = self.revision.wrapping_add(1);

        emit!(ModelUpdated {
            model: self.key(),
            version: self.version,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Transition model to active state
    pub fn activate(&mut self) -> Result<()> {
        require!(
            matches!(self.status, ModelStatus::PendingTraining),
            ModelError::InvalidStateTransition
        );

        self.status = ModelStatus::Active {
            last_inference: None,
            inference_count: 0,
        };
        self.revision = self.revision.wrapping_add(1);

        Ok(())
    }

    /// Record inference usage
    pub fn record_inference(&mut self) -> Result<()> {
        if let ModelStatus::Active {
            ref mut last_inference,
            ref mut inference_count,
        } = self.status
        {
            let clock = sysvar::clock::Clock::get()?;
            *last_inference = Some(clock.unix_timestamp);
            *inference_count = inference_count.saturating_add(1);
            self.revision = self.revision.wrapping_add(1);
            Ok(())
        } else {
            Err(ModelError::InvalidStateTransition.into())
        }
    }

    /// Verify cryptographic ownership proof
    fn verify_owner_signature(
        &self,
        message: [u8; 32],
        signature: &[u8],
    ) -> Result<()> {
        use solana_program::ed25519_program;
        
        let owner_pubkey = ed25519_program::get_processed_signer_key(
            &self.owner.to_bytes()
        )?;
        
        ed25519_program::check_signature(
            signature,
            &message,
            &owner_pubkey,
        )?;

        Ok(())
    }
}

impl IsInitialized for ModelState {
    fn is_initialized(&self) -> bool {
        self.revision > 0
    }
}

impl ModelStatus {
    /// Calculate max serialized size
    pub const LEN: usize = 1 + // variant tag
        1 + 8 + 8; // Active state fields (Option<i64> + u64)
}

/// Model metadata update event
#[event]
pub struct ModelUpdated {
    pub model: Pubkey,
    pub version: u32,
    pub timestamp: i64,
}

#[error_code]
pub enum ModelError {
    #[msg("Model already initialized")]
    AlreadyInitialized,
    #[msg("Invalid cryptographic proof")]
    InvalidProof,
    #[msg("Unauthorized operation")]
    Unauthorized,
    #[msg("Invalid model state for operation")]
    InvalidState,
    #[msg("Invalid state transition")]
    InvalidStateTransition,
    #[msg("Model version mismatch")]
    VersionMismatch,
    #[msg("Storage CID too long")]
    CidTooLong,
    #[msg("FHE parameters invalid")]
    FheParamsInvalid,
    #[msg("ZK parameters invalid")]
    ZkParamsInvalid,
}
