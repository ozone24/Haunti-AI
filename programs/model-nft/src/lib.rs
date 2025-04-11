//! Model NFT program implementation (Metaplex-compatible)

use anchor_lang::{
    prelude::*,
    solana_program::{
        program::{invoke, invoke_signed},
        sysvar,
    },
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount},
};
use mpl_token_metadata::{
    instruction::{
        create_metadata_accounts_v3,
        update_metadata_accounts_v2,
    },
    state::{
        DataV2, Creator, Collection, Uses, 
        TokenStandard, UseMethod, CollectionDetails
    },
};

declare_id!("HaunM111111111111111111111111111111111111111");

#[program]
pub mod model_nft {
    use super::*;

    /// Initialize Model NFT Mint
    pub fn initialize_model_mint(
        ctx: Context<InitializeModelMint>,
        metadata: ModelMetadata,
        creators: Vec<Creator>,
        collection: Option<Collection>,
        uses: Option<Uses>,
    ) -> Result<()> {
        // Validate royalty basis points
        require!(
            metadata.seller_fee_basis_points <= 10_000,
            ModelNftError::InvalidRoyalties
        );

        // Create metadata account
        let accounts = mpl_token_metadata::accounts::CreateMetadataAccountsV3 {
            metadata: ctx.accounts.metadata.key(),
            mint: ctx.accounts.mint.key(),
            mint_authority: ctx.accounts.payer.key(),
            update_authority: ctx.accounts.payer.key(),
            payer: ctx.accounts.payer.key(),
            system_program: ctx.accounts.system_program.key(),
            rent: ctx.accounts.rent.key(),
        };

        let data = DataV2 {
            name: metadata.name,
            symbol: metadata.symbol,
            uri: metadata.uri,
            seller_fee_basis_points: metadata.seller_fee_basis_points,
            creators: Some(creators),
            collection: collection,
            uses: uses,
        };

        let ix = create_metadata_accounts_v3(
            mpl_token_metadata::ID,
            accounts.metadata,
            accounts.mint,
            accounts.mint_authority,
            accounts.payer,
            accounts.update_authority,
            data.name,
            data.symbol,
            data.uri,
            Some(vec![Creator {
                address: *accounts.payer,
                verified: false,
                share: 100,
            }]),
            data.seller_fee_basis_points,
            data.uses,
            None,
            TokenStandard::ProgrammableNonFungible,
            None,
            None,
            Some(CollectionDetails::V1 { size: 0 }),
        );

        invoke(
            &ix,
            &[
                ctx.accounts.metadata.to_account_info(),
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.rent.to_account_info(),
            ],
        )?;

        // Initialize model state
        let model_state = &mut ctx.accounts.model_state;
        model_state.mint = *ctx.accounts.mint.key;
        model_state.version = 1;
        model_state.model_root = metadata.model_root;
        model_state.encrypted_params_uri = metadata.encrypted_params_uri;
        model_state.zk_schema_uri = metadata.zk_schema_uri;

        emit!(ModelNftEvent::MintCreated {
            mint: *ctx.accounts.mint.key,
            timestamp: sysvar::clock::Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Update Model NFT Metadata (Authored by Update Authority)
    pub fn update_model_metadata(
        ctx: Context<UpdateModelMetadata>,
        new_metadata: ModelMetadata,
        new_creators: Option<Vec<Creator>>,
    ) -> Result<()> {
        require!(
            ctx.accounts.metadata.update_authority == *ctx.accounts.update_authority.key,
            ModelNftError::Unauthorized
        );

        let accounts = mpl_token_metadata::accounts::UpdateMetadataAccountsV2 {
            metadata: ctx.accounts.metadata.key(),
            update_authority: ctx.accounts.update_authority.key(),
        };

        let data = DataV2 {
            name: new_metadata.name,
            symbol: new_metadata.symbol,
            uri: new_metadata.uri,
            seller_fee_basis_points: new_metadata.seller_fee_basis_points,
            creators: new_creators,
            collection: None, // Update collection via separate instruction
            uses: None,       // Update uses via separate instruction
        };

        let ix = update_metadata_accounts_v2(
            mpl_token_metadata::ID,
            accounts.metadata,
            accounts.update_authority,
            Some(data.name),
            Some(data.symbol),
            Some(data.uri),
            data.creators,
            Some(data.seller_fee_basis_points),
            None, // Primary sale not changed
            data.uses,
            None, // Collection not updated here
        );

        invoke(
            &ix,
            &[
                ctx.accounts.metadata.to_account_info(),
                ctx.accounts.update_authority.to_account_info(),
            ],
        )?;

        // Update model state
        let model_state = &mut ctx.accounts.model_state;
        model_state.version += 1;
        model_state.model_root = new_metadata.model_root;
        model_state.encrypted_params_uri = new_metadata.encrypted_params_uri;
        model_state.zk_schema_uri = new_metadata.zk_schema_uri;

        emit!(ModelNftEvent::MetadataUpdated {
            mint: model_state.mint,
            version: model_state.version,
            timestamp: sysvar::clock::Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Mint Model NFT to a recipient (Requires Mint Authority)
    pub fn mint_to(
        ctx: Context<MintTo>,
        amount: u64,
        authorization_data: Option<AuthorizationData>,
    ) -> Result<()> {
        // PDA-based authorization
        let (pda, bump) = Pubkey::find_program_address(
            &[b"authority", ctx.accounts.mint.key().as_ref()], 
            ctx.program_id
        );
        require!(
            pda == *ctx.accounts.authority.key,
            ModelNftError::InvalidAuthority
        );

        // SPL Token mint_to instruction
        let ix = token::MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.associated_token.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            ix,
        ).with_signer(&[&[b"authority", ctx.accounts.mint.key().as_ref(), &[bump]]]);

        token::mint_to(cpi_ctx, amount)?;

        emit!(ModelNftEvent::Minted {
            mint: *ctx.accounts.mint.key,
            recipient: *ctx.accounts.recipient.key,
            amount,
            timestamp: sysvar::clock::Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeModelMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = payer,
        mint::freeze_authority = payer,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = payer,
        space = ModelState::LEN,
        seeds = [b"model_state", mint.key().as_ref()],
        bump,
    )]
    pub model_state: Account<'info, ModelState>,

    /// CHECK: Metaplex metadata account
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct UpdateModelMetadata<'info> {
    #[account(mut)]
    pub update_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"model_state", mint.key().as_ref()],
        bump,
    )]
    pub model_state: Account<'info, ModelState>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    /// CHECK: Metaplex metadata account
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct MintTo<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mut)]
    pub mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient,
    )]
    pub associated_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub recipient: SystemAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct ModelState {
    pub mint: Pubkey,
    pub version: u32,
    pub model_root: [u8; 32],
    pub encrypted_params_uri: String,
    pub zk_schema_uri: String,
    pub last_updated: i64,
}

impl ModelState {
    pub const LEN: usize = 32 + 4 + 32 + 4 + 100 + 4 + 100 + 8;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ModelMetadata {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub seller_fee_basis_points: u16,
    pub model_root: [u8; 32],
    pub encrypted_params_uri: String,
    pub zk_schema_uri: String,
}

#[event]
pub enum ModelNftEvent {
    MintCreated {
        mint: Pubkey,
        timestamp: i64,
    },
    MetadataUpdated {
        mint: Pubkey,
        version: u32,
        timestamp: i64,
    },
    Minted {
        mint: Pubkey,
        recipient: Pubkey,
        amount: u64,
        timestamp: i64,
    },
}

#[error_code]
pub enum ModelNftError {
    #[msg("Invalid royalty configuration (max 10000)")]
    InvalidRoyalties,
    #[msg("Unauthorized metadata update")]
    Unauthorized,
    #[msg("Invalid authority PDA")]
    InvalidAuthority,
    #[msg("Metadata URI exceeds max length")]
    UriTooLong,
    #[msg("Model root hash invalid")]
    InvalidModelRoot,
    #[msg("ZK schema verification failed")]
    ZkSchemaInvalid,
}
