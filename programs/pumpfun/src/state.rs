use crate::constants::LAMPORT_DECIMALS;
use crate::constants::GLOBAL;
use crate::errors::*;
use crate::events::CompleteEvent;
use crate::utils::*;
use anchor_lang::system_program;
use anchor_lang::{prelude::*, AnchorDeserialize, AnchorSerialize};
use anchor_spl::token;
use anchor_spl::token::Mint;
use anchor_spl::token::Token;
use core::fmt::Debug;
use std::ops::Div;
use std::ops::Mul;
use std::ops::Sub;


#[account]
#[derive(InitSpace, Debug, Default)]
pub struct Whitelist {
    pub creator: Pubkey,
}

impl Whitelist {
    pub const SEED_PREFIX: &'static str = "wl-seed";
}

#[account]
pub struct Config {
    pub authority: Pubkey,
    //  use this for 2 step ownership transfer
    pub pending_authority: Pubkey,

    pub team_wallet: Pubkey,

    pub init_bonding_curve: f64, // bonding curve init percentage. The remaining amount is sent to team wallet for distribution to agent

    pub platform_buy_fee: f64, //  platform fee percentage
    pub platform_sell_fee: f64,
    pub platform_migration_fee: f64,

    pub curve_limit: u64, //  lamports to complete te bonding curve

    pub lamport_amount_config: AmountConfig<u64>,
    pub token_supply_config: AmountConfig<u64>,
    pub token_decimals_config: AmountConfig<u8>,

    pub initialized: bool,
    pub global_authority: Pubkey,    // can update settings

    pub whitelist_enabled: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum AmountConfig<T: PartialEq + PartialOrd + Debug> {
    Range { min: Option<T>, max: Option<T> },
    Enum(Vec<T>),
}

impl<T: PartialEq + PartialOrd + Debug> AmountConfig<T> {
    pub fn validate(&self, value: &T) -> Result<()> {
        match self {
            Self::Range { min, max } => {
                if let Some(min) = min {
                    if value < min {
                        msg!("value {value:?} too small, expected at least {min:?}");
                        return Err(ValueTooSmall.into());
                    }
                }
                if let Some(max) = max {
                    if value > max {
                        msg!("value {value:?} too large, expected at most {max:?}");
                        return Err(ValueTooLarge.into());
                    }
                }

                Ok(())
            }
            Self::Enum(options) => {
                if options.contains(value) {
                    Ok(())
                } else {
                    msg!("invalid value {value:?}, expected one of: {options:?}");
                    Err(ValueInvalid.into())
                }
            }
        }
    }
}

#[account]
pub struct BondingCurve {
    pub token_mint: Pubkey,
    pub creator: Pubkey,

    pub init_lamport: u64,

    pub reserve_lamport: u64,
    pub reserve_token: u64,

    pub is_completed: bool,
}
pub trait BondingCurveAccount<'info> {
    // Updates the token reserves in the liquidity pool
    fn update_reserves(
        &mut self,
        global_config: &Account<'info, Config>,
        reserve_one: u64,
        reserve_two: u64,
    ) -> Result<bool>;

    fn swap(
        &mut self,
        global_config: &Account<'info, Config>,
        token_mint: &Account<'info, Mint>,
        global_ata: &mut AccountInfo<'info>,
        user_ata: &mut AccountInfo<'info>,
        source: &mut AccountInfo<'info>,
        team_wallet: &mut AccountInfo<'info>,
        team_wallet_ata: &mut AccountInfo<'info>,
        amount: u64,
        direction: u8,
        minimum_receive_amount: u64,

        user: &Signer<'info>,
        signer: &[&[&[u8]]],

        token_program: &Program<'info, Token>,
        system_program: &Program<'info, System>,
    ) -> Result<u64>;

    fn simulate_swap(
        &self,
        global_config: &Account<'info, Config>,
        token_mint: &Account<'info, Mint>,
        amount: u64,
        direction: u8,
    ) -> Result<u64>;

    fn cal_amount_out(
        &self,
        amount: u64,
        token_one_decimals: u8,
        direction: u8,
        platform_sell_fee: f64,
        platform_buy_fee: f64,
    ) -> Result<(u64, u64)>;
}

impl<'info> BondingCurveAccount<'info> for Account<'info, BondingCurve> {
    fn update_reserves(
        &mut self,
        global_config: &Account<'info, Config>,
        reserve_token: u64,
        reserve_lamport: u64,
    ) -> Result<bool> {
        self.reserve_token = reserve_token;
        self.reserve_lamport = reserve_lamport;

        if reserve_lamport >= global_config.curve_limit {
            msg!("curve is completed");
            self.is_completed = true;
            return Ok(true);
        }

        Ok(false)
    }

    fn swap(
        &mut self,
        global_config: &Account<'info, Config>,

        token_mint: &Account<'info, Mint>,
        global_ata: &mut AccountInfo<'info>,
        user_ata: &mut AccountInfo<'info>,

        source: &mut AccountInfo<'info>,
        team_wallet: &mut AccountInfo<'info>,
        team_wallet_ata: &mut AccountInfo<'info>,

        amount: u64,
        direction: u8,
        minimum_receive_amount: u64,

        user: &Signer<'info>,
        signer: &[&[&[u8]]],

        token_program: &Program<'info, Token>,
        system_program: &Program<'info, System>,
    ) -> Result<u64> {
        if amount <= 0 {
            return err!(PumpfunError::InvalidAmount);
        }

        // if side = buy, amount to swap = min(amount, remaining reserve)
        let amount = if direction == 1 {
            amount
        } else {
            amount.min(global_config.curve_limit - self.reserve_lamport)
        };

        msg!("Mint: {:?} ", token_mint.key());
        msg!("Swap: {:?} {:?} {:?}", user.key(), direction, amount);

        // xy = k => Constant product formula
        // (x + dx)(y - dy) = k
        // y - dy = k / (x + dx)
        // y - dy = xy / (x + dx)
        // dy = y - (xy / (x + dx))
        // dy = yx + ydx - xy / (x + dx)
        // formula => dy = ydx / (x + dx)

        let (adjusted_amount, amount_out) = self.cal_amount_out(
            amount,
            token_mint.decimals,
            direction,
            global_config.platform_sell_fee,
            global_config.platform_buy_fee,
        )?;

        if amount_out < minimum_receive_amount {
            return Err(PumpfunError::ReturnAmountTooSmall.into());
        }

        if direction == 1 {
            let new_reserves_one = self
                .reserve_token
                .checked_add(amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let new_reserves_two = self
                .reserve_lamport
                .checked_sub(amount_out)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            self.update_reserves(global_config, new_reserves_one, new_reserves_two)?;

            msg! {"Reserves: {:?} {:?}", new_reserves_one, new_reserves_two};

            token_transfer_user(
                user_ata.clone(),
                &user,
                global_ata.clone(),
                &token_program,
                adjusted_amount,
            )?;

            sol_transfer_with_signer(
                source.clone(),
                user.to_account_info(),
                &system_program,
                signer,
                amount_out,
            )?;

            //  transfer fee to team wallet
            let fee_amount = amount - adjusted_amount;

            msg! {"fee: {:?}", fee_amount}

            token_transfer_user(
                user_ata.clone(),
                &user,
                team_wallet_ata.clone(),
                &token_program,
                fee_amount,
            )?;
        } else {
            let new_reserves_one = self
                .reserve_token
                .checked_sub(amount_out)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let new_reserves_two = self
                .reserve_lamport
                .checked_add(amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let is_completed =
                self.update_reserves(global_config, new_reserves_one, new_reserves_two)?;

            if is_completed == true {
                emit!(CompleteEvent {
                    user: user.key(),
                    mint: token_mint.key(),
                    bonding_curve: self.key()
                });
            }

            msg! {"Reserves: {:?} {:?}", new_reserves_one, new_reserves_two};

            token_transfer_with_signer(
                global_ata.clone(),
                source.clone(),
                user_ata.clone(),
                &token_program,
                signer,
                amount_out,
            )?;

            sol_transfer_from_user(&user, source.clone(), &system_program, amount)?;

            //  transfer fee to team wallet
            let fee_amount = amount - adjusted_amount;
            msg! {"fee: {:?}", fee_amount}

            sol_transfer_from_user(&user, team_wallet.clone(), &system_program, fee_amount)?;
        }
        Ok(amount_out)
    }

    fn simulate_swap(
        &self,
        global_config: &Account<'info, Config>,
        token_mint: &Account<'info, Mint>,
        amount: u64,
        direction: u8,
    ) -> Result<u64> {
        if amount <= 0 {
            return err!(PumpfunError::InvalidAmount);
        }

        Ok(self
            .cal_amount_out(
                amount,
                token_mint.decimals,
                direction,
                global_config.platform_sell_fee,
                global_config.platform_buy_fee,
            )?
            .1)
    }

    fn cal_amount_out(
        &self,
        amount: u64,
        token_one_decimals: u8,
        direction: u8,
        platform_sell_fee: f64,
        platform_buy_fee: f64,
    ) -> Result<(u64, u64)> {
        // xy = k => Constant product formula
        // (x + dx)(y - dy) = k
        // y - dy = k / (x + dx)
        // y - dy = xy / (x + dx)
        // dy = y - (xy / (x + dx))
        // dy = (yx + ydx - xy) / (x + dx)
        // formula => dy = ydx / (x + dx)

        let fee_percent = if direction == 1 {
            platform_sell_fee
        } else {
            platform_buy_fee
        };

        let adjusted_amount_in_float = convert_to_float(amount, token_one_decimals)
            .div(100_f64)
            .mul(100_f64.sub(fee_percent));

        let adjusted_amount = convert_from_float(adjusted_amount_in_float, token_one_decimals);

        let amount_out: u64;

        // sell
        if direction == 1 {
            // sell, token for sel
            // x + dx token
            let denominator_sum = self
                .reserve_token
                .checked_add(adjusted_amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            // (x + dx) / dx
            let div_amt = convert_to_float(denominator_sum, token_one_decimals)
                .div(convert_to_float(adjusted_amount, token_one_decimals));

            // dy = y / ((x + dx) / dx)
            // dx = ydx / (x + dx)
            let amount_out_in_float =
                convert_to_float(self.reserve_lamport, LAMPORT_DECIMALS).div(div_amt);

            amount_out = convert_from_float(amount_out_in_float, LAMPORT_DECIMALS);
        } else {
            // buy, sol for token
            // y + dy sol
            let denominator_sum = self
                .reserve_lamport
                .checked_add(adjusted_amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            // (y + dy) / dy
            let div_amt = convert_to_float(denominator_sum, LAMPORT_DECIMALS)
                .div(convert_to_float(adjusted_amount, LAMPORT_DECIMALS));

            // dx = x / ((y + dy) / dy)
            // dx = xdy / (y + dy)
            let amount_out_in_float =
                convert_to_float(self.reserve_token, token_one_decimals).div(div_amt);

            amount_out = convert_from_float(amount_out_in_float, token_one_decimals);
        }
        Ok((adjusted_amount, amount_out))
    }
}


#[account]
pub struct TokenLaunch {
    pub token: Pubkey,
    pub creator: Pubkey,

    pub init_lamport: u64,

    pub reserve_lamport: u64,
    pub reserve_token: u64,

    pub start_timestamp: i64,
    pub presale_time: u64,

    pub launch_phase: LaunchPhase,
}

impl TokenLaunch {
    pub const ACCOUNT_LEN: usize = 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum LaunchPhase {
    Presale,
    ProcessingPresale,
    OpenSale,
    Completed,
}

impl LaunchPhase {
    pub fn assert_eq(&self, phase: &Self) -> Result<()> {
        if self != phase {
            println!("launch must be in phase {phase:?}, got {self:?}");
            Err(PumpfunError::IncorrectLaunchPhase.into())
        } else {
            Ok(())
        }
    }
}

pub trait TokenLaunchAccount<'info> {
    // Updates the token reserves in the liquidity pool
    fn update_reserves(&mut self, global_config: &Account<'info, Config>, reserve_one: u64, reserve_two: u64) -> Result<()>;

    fn swap(
        &mut self,
        global_config: &Account<'info, Config>,
        token_one_accounts: (
            &mut Account<'info, Mint>,
            &mut AccountInfo<'info>,
            &mut AccountInfo<'info>,
        ),
        token_two_accounts: (&mut AccountInfo<'info>, &mut AccountInfo<'info>),
        amount: u64,
        style: u8,
        bump: u8,
        authority: &Signer<'info>,
        token_program: &Program<'info, Token>,
        system_program: &Program<'info, System>,
    ) -> Result<()>;

    fn snipe(
        &mut self,
        global_config: &Account<'info, Config>,
        token_one_accounts: (
            &mut Account<'info, Mint>,
            &mut AccountInfo<'info>,
            &mut AccountInfo<'info>,
        ),
        token_two_accounts: (&mut AccountInfo<'info>, &mut AccountInfo<'info>),
        amount: u64,
        // signers: &[&[&[u8]]; 1],
        bump: u8,
        token_program: &Program<'info, Token>,
    ) -> Result<()>;

    fn transfer_token_from_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        token_program: &Program<'info, Token>,
        authority: &AccountInfo<'info>,
        bump: u8,
    ) -> Result<()>;

    fn transfer_token_to_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        authority: &Signer<'info>,
        token_program: &Program<'info, Token>,
    ) -> Result<()>;

    fn transfer_sol_to_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        system_program: &Program<'info, System>,
    ) -> Result<()>;

    fn transfer_sol_from_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        system_program: &Program<'info, System>,
        bump: u8,
    ) -> Result<()>;

    fn transfer_sol_with_signer(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        system_program: &Program<'info, System>,
        signers: &[&[&[u8]]; 1],
        amount: u64,
    ) -> Result<()>;
}

impl<'info> TokenLaunchAccount<'info> for Account<'info, TokenLaunch> {
    fn update_reserves(&mut self, global_config: &Account<'info, Config>, reserve_token: u64, reserve_lamport: u64) -> Result<()> {
        self.reserve_token = reserve_token;
        self.reserve_lamport = reserve_lamport;

        if reserve_lamport >= global_config.curve_limit {
            msg!("curve is completed");
            self.launch_phase = LaunchPhase::Completed;
        }

        Ok(())
    }

    fn swap(
        &mut self,
        global_config: &Account<'info, Config>,
        token_one_accounts: (
            &mut Account<'info, Mint>,
            &mut AccountInfo<'info>,
            &mut AccountInfo<'info>,
        ),
        token_two_accounts: (&mut AccountInfo<'info>, &mut AccountInfo<'info>),
        amount: u64,
        style: u8,
        bump: u8,
        authority: &Signer<'info>,
        token_program: &Program<'info, Token>,
        system_program: &Program<'info, System>,
    ) -> Result<()> {
        if amount <= 0 {
            return err!(PumpfunError::InvalidAmount);
        }
        msg!("Mint: {:?} ", token_one_accounts.0.key());
        msg!("Swap: {:?} {:?} {:?}", authority.key(), style, amount);

        // xy = k => Constant product formula
        // (x + dx)(y - dy) = k
        // y - dy = k / (x + dx)
        // y - dy = xy / (x + dx)
        // dy = y - (xy / (x + dx))
        // dy = yx + ydx - xy / (x + dx)
        // formula => dy = ydx / (x + dx)

        let fees = if style == 1 {
            global_config.platform_sell_fee
        } else {
            global_config.platform_buy_fee
        };

        let adjusted_amount_in_float = convert_to_float(amount, token_one_accounts.0.decimals)
            .div(100_f64)
            .mul(100_f64.sub(fees));

        let adjusted_amount =
            convert_from_float(adjusted_amount_in_float, token_one_accounts.0.decimals);

        if style == 1 {
            let denominator_sum = self
                .reserve_token
                .checked_add(adjusted_amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let div_amt = convert_to_float(denominator_sum, token_one_accounts.0.decimals).div(
                convert_to_float(adjusted_amount, token_one_accounts.0.decimals),
            );

            let amount_out_in_float = convert_to_float(self.reserve_lamport, 9 as u8).div(div_amt);

            let amount_out = convert_from_float(amount_out_in_float, 9 as u8);

            let new_reserves_one = self
                .reserve_token
                .checked_add(amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let new_reserves_two = self
                .reserve_lamport
                .checked_sub(amount_out)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            self.update_reserves(global_config, new_reserves_one, new_reserves_two)?;
            msg! {"Reserves: {:?} {:?}", new_reserves_one, new_reserves_two}
            self.transfer_token_to_pool(
                token_one_accounts.2,
                token_one_accounts.1,
                amount,
                authority,
                token_program,
            )?;

            self.transfer_sol_from_pool(
                token_two_accounts.0,
                token_two_accounts.1,
                amount_out,
                system_program,
                bump,
            )?;
        } else {
            let denominator_sum = self
                .reserve_lamport
                .checked_add(adjusted_amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let div_amt = convert_to_float(denominator_sum, token_one_accounts.0.decimals).div(
                convert_to_float(adjusted_amount, token_one_accounts.0.decimals),
            );

            let amount_out_in_float = convert_to_float(self.reserve_token, 9 as u8).div(div_amt);

            let amount_out = convert_from_float(amount_out_in_float, 9 as u8);

            let new_reserves_one = self
                .reserve_token
                .checked_sub(amount_out)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            let new_reserves_two = self
                .reserve_lamport
                .checked_add(amount)
                .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

            self.update_reserves(global_config, new_reserves_one, new_reserves_two)?;

            msg! {"Reserves: {:?} {:?}", new_reserves_one, new_reserves_two}
            self.transfer_token_from_pool(
                token_one_accounts.1,
                token_one_accounts.2,
                amount_out,
                token_program,
                token_two_accounts.0,
                bump,
            )?;

            self.transfer_sol_to_pool(
                token_two_accounts.1,
                token_two_accounts.0,
                amount,
                system_program,
            )?;
        }
        Ok(())
    }

    fn snipe(
        &mut self,
        global_config: &Account<'info, Config>,
        token_one_accounts: (
            &mut Account<'info, Mint>,
            &mut AccountInfo<'info>,
            &mut AccountInfo<'info>,
        ),
        token_two_accounts: (&mut AccountInfo<'info>, &mut AccountInfo<'info>),
        amount: u64,
        // signers: &[&[&[u8]]; 1],
        bump: u8,
        token_program: &Program<'info, Token>,
    ) -> Result<()> {
        if amount <= 0 {
            return err!(PumpfunError::InvalidAmount);
        }

        // xy = k => Constant product formula
        // (x + dx)(y - dy) = k
        // y - dy = k / (x + dx)
        // y - dy = xy / (x + dx)
        // dy = y - (xy / (x + dx))
        // dy = yx + ydx - xy / (x + dx)
        // formula => dy = ydx / (x + dx)

        let fees = global_config.platform_buy_fee;

        let adjusted_amount_in_float = convert_to_float(amount, token_one_accounts.0.decimals)
            .div(100_f64)
            .mul(100_f64.sub(fees));

        let adjusted_amount =
            convert_from_float(adjusted_amount_in_float, token_one_accounts.0.decimals);

        let denominator_sum = self
            .reserve_lamport
            .checked_add(adjusted_amount)
            .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

        let div_amt = convert_to_float(denominator_sum, token_one_accounts.0.decimals).div(
            convert_to_float(adjusted_amount, token_one_accounts.0.decimals),
        );

        let amount_out_in_float = convert_to_float(self.reserve_token, 9 as u8).div(div_amt);

        let amount_out = convert_from_float(amount_out_in_float, 9 as u8);

        let new_reserves_one = self
            .reserve_token
            .checked_sub(amount_out)
            .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

        let new_reserves_two = self
            .reserve_lamport
            .checked_add(amount)
            .ok_or(PumpfunError::OverflowOrUnderflowOccurred)?;

        self.update_reserves(global_config, new_reserves_one, new_reserves_two)?;

        msg! {"Reserves: {:?} {:?}", new_reserves_one, new_reserves_two}

        self.transfer_token_from_pool(
            token_one_accounts.1,
            token_one_accounts.2,
            amount_out,
            token_program,
            token_two_accounts.0,
            bump,
        )?;

        Ok(())

    }

    fn transfer_token_from_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        token_program: &Program<'info, Token>,
        authority: &AccountInfo<'info>,
        bump: u8,
    ) -> Result<()> {
        token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                token::Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                    authority: authority.to_account_info(),
                },
                &[&[
                    GLOBAL.as_bytes(),
                    &[bump]
                ]],
            ),
            amount,
        )?;

        Ok(())
    }

    fn transfer_token_to_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        authority: &Signer<'info>,
        token_program: &Program<'info, Token>,
    ) -> Result<()> {
        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                token::Transfer {
                    from: from.clone(),
                    to: to.clone(),
                    authority: authority.to_account_info(),
                },
            ),
            amount,
        )?;

        Ok(())
    }

    fn transfer_sol_from_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        system_program: &Program<'info, System>,
        bump: u8,
    ) -> Result<()> {
        system_program::transfer(
            CpiContext::new_with_signer(
                system_program.to_account_info(),
                system_program::Transfer {
                    from: from.clone(),
                    to: to.clone(),
                },
                &[&["global".as_bytes(), &[bump]]],
            ),
            amount,
        )?;

        Ok(())
    }

    fn transfer_sol_to_pool(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        amount: u64,
        system_program: &Program<'info, System>,
    ) -> Result<()> {
        system_program::transfer(
            CpiContext::new(
                system_program.to_account_info(),
                system_program::Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                },
            ),
            amount,
        )?;
        Ok(())
    }

    fn transfer_sol_with_signer(
        &self,
        from: &AccountInfo<'info>,
        to: &AccountInfo<'info>,
        system_program: &Program<'info, System>,
        signers: &[&[&[u8]]; 1],
        amount: u64,
    ) -> Result<()> {
        system_program::transfer(
            CpiContext::new_with_signer(
                system_program.to_account_info(),
                system_program::Transfer {
                    from: from.to_account_info(),
                    to: to.to_account_info(),
                },
                signers,
            ),
            amount,
        )?;
        Ok(())
    }
}