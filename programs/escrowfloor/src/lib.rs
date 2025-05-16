use anchor_lang::prelude::*;

declare_id!("4gjmWmuanYNZTsU1vXnUSUsphL9BYBNSkh6UoU5ym9i4");

#[program]
pub mod escrowfloor {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
