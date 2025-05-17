use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;

declare_id!("4gjmWmuanYNZTsU1vXnUSUsphL9BYBNSkh6UoU5ym9i4");

// Constants for profit calculation
pub const PROFIT_PERCENTAGE: u64 = 20; // 20% profit for correct prediction

#[program]
pub mod escrowfloor {
    use super::*;

    pub fn initialize_escrow(
        ctx: Context<InitializeEscrow>,
        collection_id: String,
        predicted_floor: u64,
        expiry_timestamp: i64,
        margin_amount: u64,
    ) -> Result<()> {
        let escrow_key = ctx.accounts.escrow.key();
        let escrow = &mut ctx.accounts.escrow;

        // For testing, we'll skip collection verification
        // In production, this would verify against Tensor's API

        escrow.trader = ctx.accounts.trader.key();
        escrow.collection_id = collection_id;
        escrow.predicted_floor = predicted_floor;
        escrow.expiry_timestamp = expiry_timestamp;
        escrow.margin_amount = margin_amount;
        escrow.is_initialized = true;

        // Transfer margin amount from trader to escrow account
        let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.trader.key(),
            &escrow_key,
            margin_amount,
        );

        anchor_lang::solana_program::program::invoke(
            &transfer_instruction,
            &[
                ctx.accounts.trader.to_account_info(),
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        Ok(())
    }

    pub fn accept_escrow(ctx: Context<AcceptEscrow>) -> Result<()> {
        let trader = &ctx.accounts.trader;
        let escrow = &ctx.accounts.escrow;

        // Verify escrow state
        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.is_initialized, EscrowError::NotInitialized);
        require!(Clock::get()?.unix_timestamp < escrow.expiry_timestamp, EscrowError::Expired);

        // Transfer margin amount from trader to escrow account
        let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
            &trader.key(),
            &escrow.key(),
            escrow.margin_amount,
        );

        anchor_lang::solana_program::program::invoke(
            &transfer_instruction,
            &[
                trader.to_account_info(),
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        
        // Update escrow state after transfer
        let escrow = &mut ctx.accounts.escrow;
        escrow.counterparty = Some(trader.key());
        
        Ok(())
    }

    pub fn settle_escrow(ctx: Context<SettleEscrow>) -> Result<()> {
        let escrow = &ctx.accounts.escrow;
        let tensor_oracle = &ctx.accounts.tensor_oracle;
        
        // Verify escrow state
        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.is_initialized, EscrowError::NotInitialized);
        require!(escrow.counterparty.is_some(), EscrowError::NoSecondTrader);
        require!(Clock::get()?.unix_timestamp >= escrow.expiry_timestamp, EscrowError::NotExpiredYet);

        // Get current floor price from Tensor oracle
        let current_floor_price = tensor_oracle.get_floor_price(&escrow.collection_id)?;

        // Determine winner based on predicted floor vs actual floor
        let winner_key = if (escrow.predicted_floor as i64 - current_floor_price as i64).abs() <= 100 {
            // Trader wins if prediction is within 100 lamports
            escrow.trader
        } else {
            // Counterparty wins
            escrow.counterparty.unwrap()
        };

        // Calculate total amount to transfer
        let total_amount = escrow.margin_amount * 2;

        // Get bump from derive macro
        let bump = ctx.bumps.escrow;

        // Transfer funds to winner
        let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
            &escrow.key(),
            &winner_key,
            total_amount,
        );

        anchor_lang::solana_program::program::invoke_signed(
            &transfer_instruction,
            &[
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.winner.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[&[b"escrow", escrow.trader.as_ref(), &[bump]]],
        )?;

        // Update escrow state after transfer
        let escrow = &mut ctx.accounts.escrow;
        escrow.settled = true;
        
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeEscrow<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    
    #[account(
        init,
        payer = trader,
        space = EscrowState::LEN,
        seeds = [b"escrow", trader.key().as_ref()],
        bump
    )]
    pub escrow: Account<'info, EscrowState>,
    
    /// CHECK: This is Tensor's oracle account for floor price
    pub tensor_oracle: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AcceptEscrow<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    
    #[account(mut)]
    pub escrow: Account<'info, EscrowState>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SettleEscrow<'info> {
    /// CHECK: Winner account to receive funds
    #[account(mut)]
    pub winner: AccountInfo<'info>,
    
    #[account(mut)]
    pub escrow: Account<'info, EscrowState>,
    
    /// CHECK: This is Tensor's oracle account for floor price
    pub tensor_oracle: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[account]
pub struct EscrowState {
    pub trader: Pubkey,
    pub counterparty: Option<Pubkey>,
    pub collection_id: String,
    pub predicted_floor: u64,
    pub expiry_timestamp: i64,
    pub margin_amount: u64,
    pub is_initialized: bool,
    pub settled: bool,
}

impl EscrowState {
    pub const LEN: usize = 8 + // discriminator
        32 + // trader
        33 + // counterparty (Option<Pubkey>)
        36 + // collection_id (max 32 chars + 4 bytes for length)
        8 + // predicted_floor
        8 + // expiry_timestamp
        8 + // margin_amount
        1 + // is_initialized
        1; // settled
}

/// Custom trait for Tensor oracle interactions
pub trait TensorOracle {
    fn get_floor_price(&self, collection_id: &str) -> Result<u64>;
}

impl TensorOracle for AccountInfo<'_> {
    fn get_floor_price(&self, _collection_id: &str) -> Result<u64> {
        // For testing, we'll return a mock floor price
        // In production, this would make an HTTP call to Tensor's API
        Ok(10 * anchor_lang::solana_program::native_token::LAMPORTS_PER_SOL)
    }
}

#[error_code]
pub enum EscrowError {
    #[msg("Escrow is already settled")]
    AlreadySettled,
    #[msg("Escrow is not initialized")]
    NotInitialized,
    #[msg("Escrow has expired")]
    Expired,
    #[msg("Escrow has not expired yet")]
    NotExpiredYet,
    #[msg("No second trader has accepted the escrow")]
    NoSecondTrader,
}
