//! Instruction handler for minting AI models as NFTs with on-chain metadata

use anchor_lang::{
    prelude::*,
    solana_program::{
        program::{invoke, invoke_signed},
        sysvar::instructions,
    },
};
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{
        create_metadata_accounts_v3, CreateMetadataAccountsV3, Metadata,
        MetadataAccount, MasterEditionAccount,
    },
    token::{mint_to, Mint, MintTo, Token, TokenAccount},
};
use mpl_token_metadata::{
    instruction::create_master_edition_v3,
    state::{Collection, Creator, DataV2, TokenStandard},
};
use crate::{
    error::HauntiError,
    state::{ModelNFT, ModelType},
    utils::{self, compute_model_hash},
    constants::METADATA_SEED,
};

#[derive(Accounts)]
#[instruction(
    model_type: ModelType, 
    params_hash: [u8; 32],
    encrypted_params: Vec<u8>
)]
pub struct MintModel<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = ModelNFT::LEN,
        seeds = [b"model", payer.key().as_ref(), &params_hash],
        bump
    )]
    pub model_nft: Account<'info, ModelNFT>,

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
        associated_token::mint = mint,
        associated_token::authority = payer,
    )]
    pub model_token_account: Account<'info, TokenAccount>,

    /// Metaplex Metadata PDA
    #[account(
        mut,
        address = MetadataAccount::find_pda(&mint.key()).0
    )]
    pub metadata_account: Account<'info, MetadataAccount>,

    #[account(
        mut,
        address = MasterEditionAccount::find_pda(&mint.key()).0
    )]
    pub master_edition_account: Account<'info, MasterEditionAccount>,

    #[account(address = Token::id())]
    pub token_program: Program<'info, Token>,
    #[account(address = mpl_token_metadata::ID)]
    pub metadata_program: Program<'info, mpl_token_metadata::Metadata>,
    #[account(address = instructions::ID)]
    pub sysvar_instructions: Account<'info, instructions::Instructions>,
    #[account(address = associated_token::ID)]
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> MintModel<'info> {
    pub fn execute(
        &mut self,
        model_type: ModelType,
        params_hash: [u8; 32],
        encrypted_params: Vec<u8>,
        name: String,
        symbol: String,
        uri: String,
        creators: Vec<Creator>,
        royalty_basis_points: u16,
    ) -> Result<()> {
        // Validate model uniqueness
        self.validate_unique_model(&params_hash)?;

        // 1. Initialize ModelNFT account
        self.initialize_model_nft(
            model_type,
            params_hash,
            encrypted_params,
        )?;

        // 2. Mint NFT token
        self.mint_token()?;

        // 3. Create metadata
        self.create_metadata(
            name,
            symbol,
            uri,
            creators,
            royalty_basis_points,
        )?;

        // 4. Create master edition
        self.create_master_edition()?;

        emit!(ModelMinted {
            mint: self.mint.key(),
            model_type,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    fn validate_unique_model(&self, params_hash: &[u8; 32]) -> Result<()> {
        let existing_account = ModelNFT::find_model(
            self.payer.key(),
            *params_hash,
        )?;
        
        require!(
            existing_account.is_none(),
            HauntiError::DuplicateModel
        );

        Ok(())
    }

    fn initialize_model_nft(
        &mut self,
        model_type: ModelType,
        params_hash: [u8; 32],
        encrypted_params: Vec<u8>,
    ) -> Result<()> {
        let model_hash = compute_model_hash(&encrypted_params)?;

        self.model_nft.set_inner(ModelNFT {
            model_type,
            params_hash,
            encrypted_params,
            model_hash,
            mint: self.mint.key(),
            authority: self.payer.key(),
            bump: self.bumps["model_nft"],
            created_at: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    fn mint_token(&self) -> Result<()> {
        let cpi_accounts = MintTo {
            mint: self.mint.to_account_info(),
            to: self.model_token_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            self.token_program.to_account_info(),
            cpi_accounts,
        );

        // Mint 1 token (NFT)
        mint_to(cpi_ctx, 1)?;

        Ok(())
    }

    fn create_metadata(
        &self,
        name: String,
        symbol: String,
        uri: String,
        mut creators: Vec<Creator>,
        royalty_basis_points: u16,
    ) -> Result<()> {
        // Add default creator if empty
        if creators.is_empty() {
            creators.push(Creator {
                address: self.payer.key(),
                verified: false,
                share: 100,
            });
        }

        let data = DataV2 {
            name,
            symbol,
            uri,
            seller_fee_basis_points: royalty_basis_points,
            creators: Some(creators),
            collection: None,
            uses: None,
        };

        let accounts = CreateMetadataAccountsV3 {
            metadata: self.metadata_account.to_account_info(),
            mint: self.mint.to_account_info(),
            mint_authority: self.payer.to_account_info(),
            update_authority: self.payer.to_account_info(),
            payer: self.payer.to_account_info(),
            system_program: self.system_program.to_account_info(),
            rent: self.rent.to_account_info(),
        };

        let ix = create_metadata_accounts_v3(
            self.metadata_program.key(),
            accounts,
            data,
            false,  // Is mutable
            true,   // Update authority is signer
            None,   // Collection details
        );

        invoke(
            &ix,
            &[
                self.metadata_account.to_account_info(),
                self.mint.to_account_info(),
                self.payer.to_account_info(),
                self.system_program.to_account_info(),
                self.rent.to_account_info(),
            ],
        )?;

        Ok(())
    }

    fn create_master_edition(&self) -> Result<()> {
        let accounts = mpl_token_metadata::accounts::CreateMasterEditionV3 {
            edition: self.master_edition_account.to_account_info(),
            mint: self.mint.to_account_info(),
            update_authority: self.payer.to_account_info(),
            metadata: self.metadata_account.to_account_info(),
            payer: self.payer.to_account_info(),
            system_program: self.system_program.to_account_info(),
            rent: Some(self.rent.to_account_info()),
        };

        let ix = create_master_edition_v3(
            self.metadata_program.key(),
            accounts,
            None, // Max supply (0 for unlimited)
        );

        invoke(
            &ix,
            &[
                self.master_edition_account.to_account_info(),
                self.mint.to_account_info(),
                self.payer.to_account_info(),
                self.metadata_account.to_account_info(),
                self.system_program.to_account_info(),
                self.rent.to_account_info(),
            ],
        )?;

        Ok(())
    }
}

#[event]
pub struct ModelMinted {
    pub mint: Pubkey,
    pub model_type: ModelType,
    pub timestamp: i64,
}

// Security: Zeroize encrypted parameters
impl Drop for ModelNFT {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.encrypted_params.zeroize();
        self.model_hash.zeroize();
    }
}
