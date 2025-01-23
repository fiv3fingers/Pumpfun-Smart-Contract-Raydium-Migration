use crate::*;

use constants::CONFIG;
use crate::{
    state::Whitelist,
    errors::*,
};

#[derive(Accounts)]
pub struct RemoveWl<'info> {
    #[account(
        mut,
        seeds = [CONFIG.as_bytes()],
        bump,
    )]
    global_config: Box<Account<'info, Config>>,

    #[account(
        mut,
        close = admin,
        seeds = [Whitelist::SEED_PREFIX.as_bytes(), whitelist.creator.key().as_ref()],
        bump
    )]
    pub whitelist: Account<'info, Whitelist>,

    #[account(
        mut, 
        constraint = admin.key() == global_config.global_authority.key() @ PumpfunError::InvalidGlobalAuthority
    )]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}
