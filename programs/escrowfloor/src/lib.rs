use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;

declare_id!("4gjmWmuanYNZTsU1vXnUSUsphL9BYBNSkh6UoU5ym9i4");

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
        let escrow = &mut ctx.accounts.escrow;
        let trader = &ctx.accounts.trader;

        escrow.trader_a = trader.key();
        escrow.collection_id = collection_id;
        escrow.predicted_floor = predicted_floor;
        escrow.expiry_timestamp = expiry_timestamp;
        escrow.margin_amount = margin_amount;
        escrow.initialized = true;
        escrow.settled = false;

        // Transfer margin from trader to escrow account
        let cpi_accounts = anchor_lang::system_program::Transfer {
            from: trader.to_account_info(),
            to: escrow.to_account_info(),
        };

        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        anchor_lang::system_program::transfer(cpi_ctx, margin_amount)?;

        Ok(())
    }

    pub fn accept_escrow(ctx: Context<AcceptEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        let trader = &ctx.accounts.trader;

        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.initialized, EscrowError::NotInitialized);
        require!(Clock::get()?.unix_timestamp < escrow.expiry_timestamp, EscrowError::Expired);

        // Transfer margin from trader B to escrow account
        let cpi_accounts = anchor_lang::system_program::Transfer {
            from: trader.to_account_info(),
            to: escrow.to_account_info(),
        };

        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        anchor_lang::system_program::transfer(cpi_ctx, escrow.margin_amount)?;
        
        escrow.trader_b = Some(trader.key());
        
        Ok(())
    }

    pub fn settle_escrow(
        ctx: Context<SettleEscrow>,
        current_floor_price: u64,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        
        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.initialized, EscrowError::NotInitialized);
        require!(escrow.trader_b.is_some(), EscrowError::NoSecondTrader);
        require!(Clock::get()?.unix_timestamp >= escrow.expiry_timestamp, EscrowError::NotExpiredYet);

        // Determine winner based on predicted floor vs actual floor
        let winner = if (current_floor_price as i64 - escrow.predicted_floor as i64).abs() <= 100 {
            // If prediction is within 100 lamports, trader A wins
            escrow.trader_a
        } else if current_floor_price > escrow.predicted_floor {
            // If actual floor is higher than predicted, trader B wins
            escrow.trader_b.unwrap()
        } else {
            // If actual floor is lower than predicted, trader A wins
            escrow.trader_a
        };

        // Calculate total amount (both margins)
        let total_amount = escrow.margin_amount.checked_mul(2).unwrap();

        // Transfer total amount to winner
        **escrow.to_account_info().try_borrow_mut_lamports()? = 0;
        **ctx.accounts.winner.try_borrow_mut_lamports()? += total_amount;

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
    
    pub system_program: Program<'info, System>,
}

#[account]
pub struct EscrowState {
    pub trader_a: Pubkey,
    pub trader_b: Option<Pubkey>,
    pub collection_id: String,
    pub predicted_floor: u64,
    pub expiry_timestamp: i64,
    pub margin_amount: u64,
    pub initialized: bool,
    pub settled: bool,
}

impl EscrowState {
    pub const LEN: usize = 8 + // discriminator
        32 + // trader_a
        33 + // trader_b (Option<Pubkey>)
        36 + // collection_id (max 32 chars + 4 bytes for length)
        8 + // predicted_floor
        8 + // expiry_timestamp
        8 + // margin_amount
        1 + // initialized
        1; // settled
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
