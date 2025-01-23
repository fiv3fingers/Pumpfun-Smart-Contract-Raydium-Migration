use crate::*;

use constants::CONFIG;

use crate::state::Whitelist;



#[derive(Accounts)]
#[instruction(new_creator: Pubkey)]
pub struct AddWl<'info> {
      #[account(
        mut,
        seeds = [CONFIG.as_bytes()],
        bump,
    )]
    global_config: Box<Account<'info, Config>>,

    #[account(
        init,
        payer = admin,
        space = 8 + 32,
        seeds = [Whitelist::SEED_PREFIX.as_bytes(), new_creator.key().as_ref()],
        bump
    )]
    pub whitelist: Account<'info, Whitelist>,
    
    #[account(
        mut, 
        // constraint = admin.key() == global_config.global_authority.key() @ PumpfunError::InvalidGlobalAuthority
    )]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

impl AddWl<'_> {
    pub fn handler(ctx: Context<AddWl>, new_creator: Pubkey) -> Result<()> {
        let whitelist = &mut ctx.accounts.whitelist;
        whitelist.creator = new_creator.key();
        Ok(())
    }
}
