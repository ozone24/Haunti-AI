//! Cross-chain message passing implementation using Wormhole protocol
//! Supports multi-chain message verification and state attestation

use anchor_lang::{
    prelude::*,
    solana_program::{program::invoke, system_instruction},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount},
};
use wormhole_sdk::{
    vaa::{Body, Header},
    Address, Chain, Message,
};

// Message payload structure
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CrossChainMessage {
    pub source_chain: Chain,
    pub target_chain: Chain,
    pub payload: Vec<u8>,
    pub nonce: u32,
    pub timestamp: i64,
    pub status: MessageStatus,
}

// Account structure for storing messages
#[account]
pub struct MessageAccount {
    pub message: CrossChainMessage,
    pub emitter: Pubkey,
    pub sequence: u64,
    pub vaa_hash: [u8; 32],
    pub guardian_set_index: u32,
}

// Instruction context
#[derive(Accounts)]
pub struct SendMessage<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + MessageAccount::LEN,
        seeds = [b"message", emitter.key().as_ref(), &sequence.to_le_bytes()],
        bump
    )]
    pub message_account: Account<'info, MessageAccount>,
    pub emitter: Signer<'info>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = emitter
    )]
    pub fee_account: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    #[account(address = wormhole::ID)]
    pub wormhole_program: Program<'info, wormhole::program::Wormhole>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

impl SendMessage<'_> {
    pub fn send_message(
        ctx: Context<SendMessage>,
        target_chain: Chain,
        payload: Vec<u8>,
        nonce: u32,
    ) -> Result<()> {
        let message = CrossChainMessage {
            source_chain: Chain::Solana,
            target_chain,
            payload,
            nonce,
            timestamp: Clock::get()?.unix_timestamp,
            status: MessageStatus::Pending,
        };

        // Validate payload size
        if message.payload.len() > 1024 {
            return Err(ErrorCode::MessageTooLarge.into());
        }

        // Deduct fee payment
        let fee = calculate_fee(target_chain)?;
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.fee_account.to_account_info(),
                    to: ctx.accounts.wormhole_program.to_account_info(),
                    authority: ctx.accounts.emitter.to_account_info(),
                },
            ),
            fee,
        )?;

        // Generate VAA (Verified Action Approval)
        let vaa = construct_vaa(
            &ctx.accounts.emitter,
            target_chain,
            &message,
            ctx.accounts.message_account.sequence,
        )?;

        // Store message state
        let message_account = &mut ctx.accounts.message_account;
        message_account.message = message;
        message_account.emitter = ctx.accounts.emitter.key();
        message_account.sequence = message_account.sequence.checked_add(1).unwrap();
        message_account.vaa_hash = sha256::digest(&vaa).into();
        message_account.guardian_set_index = get_current_guardian_set_index()?;

        // Emit wormhole message
        invoke(
            &wormhole::instruction::post_vaa(
                ctx.accounts.wormhole_program.key(),
                vaa.header,
                vaa.body,
            )?,
            &[
                ctx.accounts.message_account.to_account_info(),
                ctx.accounts.wormhole_program.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        Ok(())
    }
}

// Wormhole VAA construction
fn construct_vaa(
    emitter: &Pubkey,
    target_chain: Chain,
    message: &CrossChainMessage,
    sequence: u64,
) -> Result<(Header, Body)> {
    let header = Header {
        version: 1,
        guardian_set_index: get_current_guardian_set_index()?,
        timestamp: message.timestamp as u32,
        nonce: message.nonce,
        emitter_chain: Chain::Solana,
        emitter_address: emitter.to_bytes(),
        sequence,
        consistency_level: 0,
    };

    let body = Body {
        chain_id: target_chain,
        contract: Address::from([0u8; 32]), // Target contract address
        payload: message.payload.clone(),
    };

    Ok((header, body))
}

// Fee calculation based on target chain
fn calculate_fee(target_chain: Chain) -> Result<u64> {
    match target_chain {
        Chain::Ethereum => Ok(1000000),    // 1 USDC
        Chain::Bsc => Ok(500000),         // 0.5 USDC
        Chain::Polygon => Ok(300000),     // 0.3 USDC
        _ => Err(ErrorCode::UnsupportedChain.into()),
    }
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("Message payload exceeds 1KB limit")]
    MessageTooLarge,
    #[msg("Target chain not supported")]
    UnsupportedChain,
    #[msg("Invalid fee payment")]
    FeePaymentFailed,
    #[msg("VAA construction failed")]
    VaaError,
    #[msg("Sequence number overflow")]
    SequenceOverflow,
}

// Constants
impl MessageAccount {
    pub const LEN: usize = 32 + 8 + 4 + 4 + 32 + 8 + 4;
}

// Wormhole SDK integration
mod wormhole {
    use super::*;
    pub const ID: Pubkey = Pubkey::new_from_array([ ... ]);
    
    pub mod program {
        use super::*;
        pub struct Wormhole;
        impl anchor_lang::Id for Wormhole {
            fn id() -> Pubkey {
                ID
            }
        }
    }
    
    pub fn instruction::post_vaa(
        program_id: Pubkey,
        header: Header,
        body: Body,
    ) -> Result<Instruction> {
        // Implementation depends on Wormhole program details
    }
}
