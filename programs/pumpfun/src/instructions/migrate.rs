use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token},
};

use crate::{amm_instruction, constants::{CONFIG, GLOBAL, TOKEN_LAUNCH}, errors::PumpfunError, state::{Config, LaunchPhase, TokenLaunch, TokenLaunchAccount}};

pub fn migrate(
    ctx: Context<Migrate>,
    nonce: u8
) -> Result<()> {
    let token_launch = &mut ctx.accounts.token_launch;
    
    //  check launch phase is completed
    token_launch.launch_phase.assert_eq(&LaunchPhase::Completed)?;


    let fee_lamport = token_launch.reserve_lamport / 10000 * ctx.accounts.global_config.platform_migration_fee as u64;

    //  0.32 - market create, 0.4 - pool create, 0.01 - tx
    let init_pc_amount = token_launch.reserve_lamport - fee_lamport - token_launch.init_lamport - 730_000_000;

    let coin_amount = (init_pc_amount as u128 * token_launch.reserve_token as u128 / token_launch.reserve_lamport as u128) as u64; 
    let fee_token = token_launch.reserve_token - coin_amount;

    //  transfer 0.33 SOL to signer for market id creation fee + tx fee
    token_launch.transfer_sol_from_pool(
        &ctx.accounts.global_vault,
        &ctx.accounts.user_wallet,
        330_000_000, 
        &ctx.accounts.system_program,
        ctx.bumps.global_vault
    )?;

    //  transfer fee lamport and fee token to admin wallet
    token_launch.transfer_sol_from_pool(
        &ctx.accounts.global_vault,
        &ctx.accounts.admin,
        fee_lamport,
        &ctx.accounts.system_program,
        ctx.bumps.global_vault
    )?;
    token_launch.transfer_token_from_pool(
        &ctx.accounts.global_token_account,
        &ctx.accounts.admin_token_account,
        fee_token,
        &ctx.accounts.token_program,
        &ctx.accounts.global_vault,
        ctx.bumps.global_vault
    )?;


    let seeds = &[
        GLOBAL.as_bytes(), 
        &[ctx.bumps.global_vault]
    ];
    let signed_seeds = &[&seeds[..]];


    //  Running raydium amm initialize2
    let initialize_ix = amm_instruction::initialize2(
        ctx.accounts.amm_program.key,
        ctx.accounts.amm.key,
        ctx.accounts.amm_authority.key,
        ctx.accounts.amm_open_orders.key,
        ctx.accounts.lp_mint.key,
        &ctx.accounts.coin_mint.key(),
        &ctx.accounts.pc_mint.key(),
        ctx.accounts.coin_vault.key,
        ctx.accounts.pc_vault.key,
        ctx.accounts.target_orders.key,
        ctx.accounts.amm_config.key,
        ctx.accounts.admin.key,
        ctx.accounts.market_program.key,
        ctx.accounts.market.key,
        //  change this to PDA address
        ctx.accounts.global_vault.key,
        ctx.accounts.global_token_account.key,
        ctx.accounts.user_token_pc.key,
        &ctx.accounts.user_token_lp.key(),
        nonce,
        Clock::get()?.unix_timestamp as u64,
        init_pc_amount,
        coin_amount,
    )?;
    let account_infos = [
        ctx.accounts.amm_program.clone(),
        ctx.accounts.amm.clone(),
        ctx.accounts.amm_authority.clone(),
        ctx.accounts.amm_open_orders.clone(),
        ctx.accounts.lp_mint.clone(),
        ctx.accounts.coin_mint.to_account_info().clone(),
        ctx.accounts.pc_mint.to_account_info().clone(),
        ctx.accounts.coin_vault.clone(),
        ctx.accounts.pc_vault.clone(),
        ctx.accounts.target_orders.clone(),
        ctx.accounts.amm_config.clone(),
        ctx.accounts.admin.clone(),
        ctx.accounts.market_program.clone(),
        ctx.accounts.market.clone(),
        ctx.accounts.global_vault.clone(),
        ctx.accounts.global_token_account.clone(),
        ctx.accounts.user_token_pc.clone(),
        ctx.accounts.user_token_lp.clone(),
        ctx.accounts.token_program.to_account_info().clone(),
        ctx.accounts.system_program.to_account_info().clone(),
        ctx.accounts
            .associated_token_program
            .to_account_info()
            .clone(),
        ctx.accounts.sysvar_rent.to_account_info().clone(),
    ];
    invoke_signed(&initialize_ix, &account_infos, signed_seeds)?;

    msg!("Reserve:: Token: {:?}  Sol: {:?}", token_launch.reserve_token, token_launch.reserve_lamport);
    msg!("Raydium Input:: Token: {:?}  Sol: {:?}", coin_amount, init_pc_amount);
    
    //  update reserves
    token_launch.update_reserves(&*ctx.accounts.global_config, 0, 0)?;

    Ok(())
}

#[derive(Accounts)]
pub struct Migrate<'info> {

    /// CHECK: Safe
    #[account(
        mut,
        constraint = global_config.authority == *admin.key @PumpfunError::IncorrectAuthority
    )]
    pub admin: AccountInfo<'info>,

    #[account(
        seeds = [CONFIG.as_bytes()],
        bump,
    )]
    global_config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [TOKEN_LAUNCH.as_bytes(), &coin_mint.key().to_bytes()],
        bump
    )]
    token_launch: Box<Account<'info, TokenLaunch>>,

    /// CHECK
    #[account(
        mut,
        seeds = [GLOBAL.as_bytes()],
        bump,
    )]
    pub global_vault: AccountInfo<'info>,

    /// CHECK: Safe
    pub amm_program: AccountInfo<'info>,
    /// CHECK: Safe. The spl token program
    pub token_program: Program<'info, Token>,
    /// CHECK: Safe. The associated token program
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// CHECK: Safe. System program
    pub system_program: Program<'info, System>,
    /// CHECK: Safe. Rent program
    pub sysvar_rent: Sysvar<'info, Rent>,
    /// CHECK: Safe.
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"amm_associated_seed"],
        bump,
        seeds::program = amm_program.key
    )]
    pub amm: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        seeds = [b"amm authority"],
        bump,
        seeds::program = amm_program.key
    )]
    pub amm_authority: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"open_order_associated_seed"],
        bump,
        seeds::program = amm_program.key
    )]
    pub amm_open_orders: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"lp_mint_associated_seed"
        ],
        bump,
        seeds::program = amm_program.key
    )]
    pub lp_mint: AccountInfo<'info>,

    #[account(mut)]
    pub coin_mint: Box<Account<'info, Mint>>,
    /// CHECK: Safe. Pc mint account
    #[account(mut)]
    pub pc_mint: Box<Account<'info, Mint>>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"coin_vault_associated_seed"
        ],
        bump,
        seeds::program = amm_program.key
    )]
    pub coin_vault: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"pc_vault_associated_seed"
        ],
        bump,
        seeds::program = amm_program.key
    )]
    pub pc_vault: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [
            amm_program.key.as_ref(),
            market.key.as_ref(),
            b"target_associated_seed"
        ],
        bump,
        seeds::program = amm_program.key
    )]
    pub target_orders: AccountInfo<'info>,
    /// CHECK: Safe
    #[account(
        mut,
        seeds = [b"amm_config_account_seed"],
        bump,
        seeds::program = amm_program.key
    )]
    pub amm_config: AccountInfo<'info>,

    /// CHECK: Safe. OpenBook program.
    pub market_program: AccountInfo<'info>,
    /// CHECK: Safe. OpenBook market. OpenBook program is the owner.
    #[account(
       mut
    )]
    pub market: AccountInfo<'info>,
    /// CHECK: Safe. The user wallet create the pool
    #[account(mut)]
    pub user_wallet: Signer<'info>,

    /// CHECK: verified in transfer instruction
    #[account(
        mut,
        seeds = [
            global_vault.key().as_ref(),
            anchor_spl::token::spl_token::ID.as_ref(),
            coin_mint.key().as_ref(),
        ],
        bump,
        seeds::program = anchor_spl::associated_token::ID
    )]
    global_token_account: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [
            admin.key().as_ref(),
            anchor_spl::token::spl_token::ID.as_ref(),
            coin_mint.key().as_ref(),
        ],
        bump,
        seeds::program = anchor_spl::associated_token::ID
    )]
    admin_token_account: AccountInfo<'info>,

    /// CHECK: Safe. The user pc token
    #[account(
        mut,
    )]
    pub user_token_pc: AccountInfo<'info>,

    /// CHECK: Safe. The user lp token
    #[account(
        mut,
    )]
    pub user_token_lp: AccountInfo<'info>,
}
