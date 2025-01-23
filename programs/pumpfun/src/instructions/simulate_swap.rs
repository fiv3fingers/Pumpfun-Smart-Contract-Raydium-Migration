use anchor_lang::prelude::*;
use anchor_spl::token::Mint;
use crate::{
    constants::{BONDING_CURVE, CONFIG}, state::{BondingCurve, Config, BondingCurveAccount}
};

#[derive(Accounts)]
pub struct SimulateSwap<'info> {
    #[account(
        seeds = [CONFIG.as_bytes()],
        bump,
    )]
    global_config: Box<Account<'info, Config>>,

    #[account(
        seeds = [BONDING_CURVE.as_bytes(), &token_mint.key().to_bytes()], 
        bump
    )]
    bonding_curve: Account<'info, BondingCurve>,

    pub token_mint: Box<Account<'info, Mint>>,
}

impl <'info> SimulateSwap<'info> {
pub fn process(&mut self, amount: u64, direction: u8) -> Result<u64> {

    let amount_out = self.bonding_curve.simulate_swap(
        &*self.global_config,
        self.token_mint.as_ref(),
        amount,
        direction,
    )?;
    
    Ok(amount_out)
}

}
