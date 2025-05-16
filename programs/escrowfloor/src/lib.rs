use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;
use tensor_swap::program::TensorSwap;
use tensor_swap::state::Pool;

declare_id!("4gjmWmuanYNZTsU1vXnUSUsphL9BYBNSkh6UoU5ym9i4");

// Constants for profit calculation
pub const PROFIT_PERCENTAGE: u64 = 20; // 20% profit for correct prediction
pub const TENSOR_SWAP_PROGRAM_ID: Pubkey = tensor_swap::ID;

// Tensor Pool Seeds
pub const POOL_SEED: &[u8] = b"pool";

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
        let tensor_oracle = &ctx.accounts.tensor_oracle;

        // Verify this is a valid Tensor collection
        require!(tensor_oracle.is_initialized(), EscrowError::InvalidTensorOracle);
        require!(tensor_oracle.verify_collection(&collection_id), EscrowError::InvalidCollection);

        let trader = &ctx.accounts.trader;

        escrow.trader_a = trader.key();
        escrow.collection_id = collection_id;
        escrow.predicted_floor = predicted_floor;
        escrow.expiry_timestamp = expiry_timestamp;
        escrow.margin_amount = margin_amount;
        escrow.initialized = true;
        escrow.settled = false;

        // Calculate potential profit
        let profit = (margin_amount * PROFIT_PERCENTAGE) / 100;
        escrow.profit_amount = profit;

        // Transfer margin from trader to escrow account
        let cpi_accounts = anchor_lang::system_program::Transfer {
            from: trader.to_account_info(),

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
        let escrow = &mut ctx.accounts.escrow;
        let trader = &ctx.accounts.trader;

        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.initialized, EscrowError::NotInitialized);
        require!(Clock::get()?.unix_timestamp < escrow.expiry_timestamp, EscrowError::Expired);

        // Transfer margin + potential profit from trader B to escrow account
        let total_amount = escrow.margin_amount + escrow.profit_amount;
        
        let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.trader.key(),
            &escrow.key(),
            total_amount,
        );

        anchor_lang::solana_program::program::invoke(
            &transfer_instruction,
            &[
                ctx.accounts.trader.to_account_info(),
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        
        escrow.trader_b = Some(trader.key());
        
        Ok(())
    }

    pub fn settle_escrow(ctx: Context<SettleEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        let tensor_oracle = &ctx.accounts.tensor_oracle;
        
        require!(!escrow.settled, EscrowError::AlreadySettled);
        require!(escrow.initialized, EscrowError::NotInitialized);
        require!(escrow.trader_b.is_some(), EscrowError::NoSecondTrader);
        require!(Clock::get()?.unix_timestamp >= escrow.expiry_timestamp, EscrowError::NotExpiredYet);

        // Get current floor price from Tensor oracle
        let current_floor_price = tensor_oracle.get_floor_price(&escrow.collection_id)?;

        // Determine winner based on predicted floor vs actual floor
        let (winner, gets_profit) = if (current_floor_price as i64 - escrow.predicted_floor as i64).abs() <= 100 {
            // If prediction is within 100 lamports, trader A wins with profit
            (escrow.trader_a, true)
        } else if current_floor_price > escrow.predicted_floor {
            // If actual floor is higher than predicted, trader B wins with profit
            (escrow.trader_b.unwrap(), true)
        } else {
            // If actual floor is lower than predicted, trader A wins with profit
            (escrow.trader_a, true)
        };

        // Calculate total amount (both margins + profit if winner predicted correctly)
        let base_amount = escrow.margin_amount.checked_mul(2).unwrap();
        let total_amount = if gets_profit {
            base_amount + escrow.profit_amount
        } else {
            base_amount
        };

        // Transfer total amount to winner
        let transfer_instruction = anchor_lang::solana_program::system_instruction::transfer(
            &escrow.key(),
            &winner,
            total_amount,
        );

        anchor_lang::solana_program::program::invoke_signed(
            &transfer_instruction,
            &[
                ctx.accounts.escrow.to_account_info(),
                ctx.accounts.winner.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[&[b"escrow", escrow.trader_a.as_ref(), &[*ctx.bumps.get("escrow").unwrap()]]],
        )?;

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
    
    /// CHECK: Tensor oracle program account
    #[account(address = TENSOR_SWAP_PROGRAM_ID.parse::<Pubkey>().unwrap())]
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
    
    /// CHECK: Tensor oracle program account
    #[account(address = TENSOR_SWAP_PROGRAM_ID.parse::<Pubkey>().unwrap())]
    pub tensor_oracle: AccountInfo<'info>,
    
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
    pub profit_amount: u64,
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
        8 + // profit_amount
        1 + // initialized
        1; // settled
}

/// Custom trait for Tensor oracle interactions
pub trait TensorOracle {
    fn is_initialized(&self) -> bool;
    fn verify_collection(&self, collection_id: &str) -> bool;
    fn get_floor_price(&self, collection_id: &str) -> Result<u64>;
}

impl TensorOracle for AccountInfo<'_> {
    fn is_initialized(&self) -> bool {
        // Verify this is the Tensor Swap program
        self.key() == &TENSOR_SWAP_PROGRAM_ID
    }

    fn verify_collection(&self, collection_id: &str) -> bool {
        // Get pool address for collection
        let (pool_address, _bump) = Pubkey::find_program_address(
            &[POOL_SEED, collection_id.as_bytes()],
            &TENSOR_SWAP_PROGRAM_ID,
        );

        // Try to load pool data
        let pool_data = self.try_borrow_data().ok();
        if let Some(data) = pool_data {
            // Deserialize pool data using Tensor's Pool type
            if let Ok(pool) = Pool::try_deserialize(&mut &data[..]) {
                return pool.is_active();
            }
        }
        false
    }

    fn get_floor_price(&self, collection_id: &str) -> Result<u64> {
        // Get pool address
        let (pool_address, _bump) = Pubkey::find_program_address(
            &[POOL_SEED, collection_id.as_bytes()],
            &TENSOR_SWAP_PROGRAM_ID,
        );

        // CPI to Tensor Swap to get floor price
        let cpi_program = self.to_account_info();
        let cpi_accounts = tensor_swap::cpi::accounts::GetFloorPrice {
            pool: pool_address,
        };

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        let floor_price = tensor_swap::cpi::get_floor_price(cpi_ctx)?;

        Ok(floor_price)
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
    #[msg("Invalid Tensor oracle account")]
    InvalidTensorOracle,
    #[msg("Invalid collection ID")]
    InvalidCollection,
}
