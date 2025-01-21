pub mod amm_instruction;
pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;
pub mod utils;

use crate::instructions::*;
use anchor_lang::prelude::*;
use state::Config;

declare_id!("ApRXrsZcqKHzQFrdYYKcPhe66S5oHMwWqnC9DZVqiZFM");

#[program]
pub mod pumpfun {
    use super::*;

    //  called by admin to set global config
    //  need to check the signer is authority
    pub fn configure(ctx: Context<Configure>, new_config: Config) -> Result<()> {
        ctx.accounts.process(new_config, ctx.bumps.config)
    }

    //  Admin can hand over admin role
    pub fn nominate_authority(ctx: Context<NominateAuthority>, new_admin: Pubkey) -> Result<()> {
        ctx.accounts.process(new_admin)
    }

    //  Pending admin should accept the admin role
    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        ctx.accounts.process()
    }

    pub fn launch(
        ctx: Context<Launch>,

        // launch config
        decimals: u8,
        token_supply: u64,
        virtual_lamport_reserves: u64,

        //  metadata
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        ctx.accounts.process(
            decimals,
            token_supply,
            virtual_lamport_reserves,
            name,
            symbol,
            uri,
            ctx.bumps.global_vault,
        )
    }

    //  amount - swap amount
    //  direction - 0: buy, 1: sell
    pub fn swap(
        ctx: Context<Swap>,
        amount: u64,
        direction: u8,
        minimum_receive_amount: u64,
    ) -> Result<u64> {
        ctx.accounts.process(
            amount,
            direction,
            minimum_receive_amount,
            ctx.bumps.global_vault,
        )
    }

    //  amount - swap amount
    //  direction - 0: buy, 1: sell
    pub fn simulate_swap(ctx: Context<SimulateSwap>, amount: u64, direction: u8) -> Result<u64> {
        ctx.accounts.process(amount, direction)
    }

    //  admin can withdraw sol/token after the curve is completed
    //  backend receives a event when the curve is completed and call this instruction
    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        ctx.accounts.process(ctx.bumps.global_vault)
    }

    //  transfer fee to team wallet and prepare migration to raydium
    pub fn transfer_fee(ctx: Context<TransferFee>) -> Result<()> {
        ctx.accounts.process(ctx.bumps.global_vault)
    }

    pub fn add_wl(ctx: Context<AddWl>, new_creator: Pubkey)-> Result<()> {
        AddWl::handler(ctx, new_creator)
    }

    pub fn remove_wl(_ctx: Context<RemoveWl>) -> Result<()> {
        Ok(())
    }

    pub fn migrate(
        _ctx: Context<Migrate>,
        _nonce: u8
    ) -> Result<()>{
        Ok(())
    }
    //  backend receives a event when the curve is copmleted and run this instruction
    //  removes bonding curve and add liquidity to raydium

}
