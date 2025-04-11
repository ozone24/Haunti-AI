//! Instruction handler for submitting computation proofs with ZK verification

use anchor_lang::{
    prelude::*,
    solana_program::{program::invoke, system_instruction},
};
use plonky3::{
    field::goldilocks_field::GoldilocksField,
    plonk::proof::Proof,
    verifier::VerifierKey,
};
use fhe_rs::prelude::*;
use crate::{
    error::HauntiError,
    state::{TaskAccount, TaskState, ModelParams},
    utils::{verify_merkle_path, decrypt_reward},
    zk::ProofVerificationCircuit,
    fhe::FHEOperator,
};

#[derive(Accounts)]
#[instruction(proof: Vec<u8>, encrypted_output: Vec<u8>)]
pub struct SubmitProof<'info> {
    #[account(
        mut,
        has_one = owner @ HauntiError::OwnerMismatch,
        constraint = task_account.state == TaskState::Pending 
            @ HauntiError::TaskNotActive
    )]
    pub task_account: Account<'info, TaskAccount>,

    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        address = task_account.model.verifier_key,
        constraint = verifier_key.validate()?
    )]
    pub verifier_key: Account<'info, VerifierKey<GoldilocksField>>,

    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
}

impl<'info> SubmitProof<'info> {
    pub fn execute(
        &mut self,
        proof: Vec<u8>,
        encrypted_output: Vec<u8>,
    ) -> ProgramResult {
        // Deserialize proof
        let proof = Proof::<GoldilocksField>::deserialize(&proof)
            .map_err(|_| HauntiError::InvalidProofFormat)?;

        // Step 1: Verify ZK Proof
        self.verify_zk_proof(&proof)?;

        // Step 2: Encrypt and store result
        self.process_encrypted_output(encrypted_output)?;

        // Step 3: Update task state
        self.task_account.state = TaskState::Completed;
        self.task_account.completed_at = Clock::get()?.unix_timestamp;

        // Step 4: Distribute rewards
        self.transfer_rewards()?;

        emit!(ProofSubmitted {
            task: self.task_account.key(),
            owner: self.owner.key(),
            timestamp: self.task_account.completed_at,
        });

        Ok(())
    }

    fn verify_zk_proof(
        &self,
        proof: &Proof<GoldilocksField>,
    ) -> Result<()> {
        let public_inputs = self.task_account.model.get_public_inputs()?;
        
        ProofVerificationCircuit::verify(
            &self.verifier_key,
            proof,
            &public_inputs,
            &self.task_account.model.constraints,
        )
        .map_err(|e| {
            msg!("ZK verification failed: {:?}", e);
            HauntiError::ProofVerificationFailed
        })?;

        Ok(())
    }

    fn process_encrypted_output(
        &mut self,
        encrypted_output: Vec<u8>,
    ) -> Result<()> {
        let fhe = FHEOperator::from_public_key(
            &self.task_account.model.fhe_public_key
        )?;
        
        // Validate encryption format
        fhe.validate_ciphertext(&encrypted_output)
            .map_err(|_| HauntiError::InvalidCiphertext)?;

        // Store encrypted result
        self.task_account.encrypted_output = encrypted_output;

        // Generate storage proof
        let merkle_root = self.task_account.calculate_storage_root()?;
        self.task_account.storage_proof = Some(merkle_root);

        Ok(())
    }

    fn transfer_rewards(&self) -> Result<()> {
        let reward = decrypt_reward(
            &self.task_account.encrypted_reward,
            &self.owner.key(),
        )?;

        let reward_lamports = reward
            .checked_div(LAMPORTS_PER_SOL)
            .ok_or(HauntiError::ArithmeticOverflow)?;

        invoke(
            &system_instruction::transfer(
                &self.task_account.key(),
                &self.owner.key(),
                reward_lamports,
            ),
            &[
                self.task_account.to_account_info(),
                self.owner.to_account_info(),
                self.system_program.to_account_info(),
            ],
        )?;

        Ok(())
    }
}

#[event]
pub struct ProofSubmitted {
    pub task: Pubkey,
    pub owner: Pubkey,
    pub timestamp: i64,
}

// Security-critical: Zeroize sensitive data
impl Drop for SubmitProof<'_> {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.task_account.model.parameters.zeroize();
    }
}

// ZK Proof Verification Circuit
impl ProofVerificationCircuit<GoldilocksField> {
    pub fn verify(
        verifier_key: &VerifierKey<GoldilocksField>,
        proof: &Proof<GoldilocksField>,
        public_inputs: &[GoldilocksField],
        constraints: &[GoldilocksField],
    ) -> Result<()> {
        let mut verifier = plonky3::verifier::Verifier::new(verifier_key.clone());
        
        verifier.verify_proof(
            proof,
            public_inputs,
            constraints,
        )
        .map_err(|e| {
            msg!("Plonky3 verification error: {:?}", e);
            HauntiError::ProofVerificationFailed
        })
    }
}
