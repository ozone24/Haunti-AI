//! Cross-chain message verification via Wormhole VAA attestation

use anchor_lang::{
    prelude::*,
    solana_program::{program::invoke, sysvar::instructions},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount},
};
use wormhole_sdk::{
    vaa::{Body, Header, Signature},
    Address, Chain, GuardianSet,
};

// Verified message storage
#[account]
pub struct VerifiedMessage {
    pub source_chain: Chain,
    pub source_address: [u8; 32],
    pub payload: Vec<u8>,
    pub timestamp: i64,
    pub status: VerificationStatus,
    pub guardian_set_index: u32,
    pub vaa_hash: [u8; 32],
}

// Verification context
#[derive(Accounts)]
#[instruction(vaa_hash: [u8; 32])]
pub struct VerifyMessage<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    
    #[account(
        init,
        payer = payer,
        space = 8 + VerifiedMessage::LEN,
        seeds = [b"verified_msg", &vaa_hash],
        bump
    )]
    pub verified_message: Account<'info, VerifiedMessage>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = payer
    )]
    pub fee_account: Account<'info, TokenAccount>,
    
    #[account(address = wormhole::ID)]
    pub wormhole_program: Program<'info, wormhole::program::Wormhole>,
    
    #[account(
        seeds = [b"GuardianSet", guardian_set_index.to_le_bytes().as_ref()],
        bump,
        constraint = guardian_set.expiration_time > Clock::get()?.unix_timestamp
    )]
    pub guardian_set: Account<'info, GuardianSet>,
    
    #[account(
        seeds = [b"Message", &emitter_address, &sequence.to_le_bytes()],
        bump,
        constraint = message_account.vaa_hash == vaa_hash
    )]
    pub message_account: Account<'info, MessageAccount>,
    
    pub mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

impl VerifyMessage<'_> {
    pub fn verify_vaa(
        ctx: Context<VerifyMessage>,
        header: Header,
        signatures: Vec<Signature>,
        body: Body,
        guardian_set_index: u32,
    ) -> Result<()> {
        let vaa_hash = sha256::digest(&ctx.accounts.message_account.vaa_hash);
        
        // 1. Validate VAA integrity
        Self::validate_vaa_structure(&header, &body, &signatures)?;
        
        // 2. Check guardian signatures
        Self::verify_signatures(
            &header,
            &body,
            &signatures,
            &ctx.accounts.guardian_set,
        )?;
        
        // 3. Validate chain targeting
        require_eq!(body.chain_id, Chain::Solana, ErrorCode::InvalidTargetChain);
        
        // 4. Store verified message
        let verified = &mut ctx.accounts.verified_message;
        verified.source_chain = header.emitter_chain;
        verified.source_address = header.emitter_address;
        verified.payload = body.payload.clone();
        verified.timestamp = header.timestamp as i64;
        verified.status = VerificationStatus::Verified;
        verified.guardian_set_index = guardian_set_index;
        verified.vaa_hash = vaa_hash.into();
        
        // 5. Process fee payment
        let fee = calculate_verification_fee(body.payload.len())?;
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.fee_account.to_account_info(),
                    to: ctx.accounts.wormhole_program.to_account_info(),
                    authority: ctx.accounts.payer.to_account_info(),
                },
            ),
            fee,
        )?;

        Ok(())
    }

    fn validate_vaa_structure(
        header: &Header,
        body: &Body,
        signatures: &[Signature],
    ) -> Result<()> {
        require_gt!(signatures.len(), 0, ErrorCode::NoSignatures);
        require!(
            header.guardian_set_index == body.guardian_set_index,
            ErrorCode::GuardianSetMismatch
        );
        Ok(())
    }

    fn verify_signatures(
        header: &Header,
        body: &Body,
        signatures: &[Signature],
        guardian_set: &GuardianSet,
    ) -> Result<()> {
        let message = construct_signing_message(header, body)?;
        let mut weight_accum = 0;
        
        for sig in signatures {
            let guardian = guardian_set.keys.get(sig.index as usize)
                .ok_or(ErrorCode::InvalidGuardianIndex)?;
            
            let pubkey = ed25519_dalek::PublicKey::from_bytes(&guardian.key)
                .map_err(|_| ErrorCode::InvalidGuardianKey)?;
            
            let sig = ed25519_dalek::Signature::from_bytes(&sig.signature)
                .map_err(|_| ErrorCode::InvalidSignatureFormat)?;
            
            pubkey.verify_strict(&message, &sig)
                .map_err(|_| ErrorCode::SignatureVerificationFailed)?;
            
            weight_accum += guardian.weight;
        }
        
        require!(
            weight_accum >= guardian_set.min_threshold,
            ErrorCode::InsufficientGuardianWeight
        );
        
        Ok(())
    }
}

// Fee calculation based on payload size
fn calculate_verification_fee(payload_size: usize) -> Result<u64> {
    let base_fee = 100_000; // 0.1 USDC
    let size_fee = (payload_size / 256) as u64 * 10_000;
    Ok(base_fee + size_fee)
}

// Error handling
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid target chain specified")]
    InvalidTargetChain,
    #[msg("No guardian signatures provided")]
    NoSignatures,
    #[msg("Guardian set index mismatch")]
    GuardianSetMismatch,
    #[msg("Invalid guardian index")]
    InvalidGuardianIndex,
    #[msg("Malformed guardian public key")]
    InvalidGuardianKey,
    #[msg("Invalid signature format")]
    InvalidSignatureFormat,
    #[msg("Signature verification failed")]
    SignatureVerificationFailed,
    #[msg("Insufficient guardian weight")]
    InsufficientGuardianWeight,
    #[msg("Message payload too large")]
    PayloadTooLarge,
    #[msg("VAA timestamp too old")]
    TimestampExpired,
    #[msg("Duplicate message verification")]
    DuplicateMessage,
}

// Constants and config
impl VerifiedMessage {
    pub const LEN: usize = 32 + 32 + 4 + 8 + 1 + 4 + 32;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum VerificationStatus {
    Pending,
    Verified,
    Rejected,
}

// Wormhole integration
fn construct_signing_message(header: &Header, body: &Body) -> Result<Vec<u8>> {
    let mut msg = Vec::with_capacity(512);
    header.write(&mut msg).map_err(|_| ErrorCode::SerializationError)?;
    body.write(&mut msg).map_err(|_| ErrorCode::SerializationError)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wormhole_sdk::vaa::test_utils::*;

    #[test]
    fn test_successful_verification() {
        // Test valid VAA verification
    }

    #[test]
    fn test_insufficient_signatures() {
        // Test guardian weight threshold failure
    }

    #[test]
    fn test_expired_guardian_set() {
        // Test guardian set expiration validation
    }
}
