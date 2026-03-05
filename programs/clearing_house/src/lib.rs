use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    error::ClearingHouseError,
    state::{Market, Order, OrderSide, OrderStatus, UserPosition},
};

declare_id!("5pjD2YK4DCZnvQt3SnB9fwmmhxFyubBrBMuQitd3Kyz5");

pub mod error;
pub mod state;

fn check_context<T>(ctx: &Context<T>) -> Result<()>
where
    T: anchor_lang::Bumps,
{
    if !check_id(ctx.program_id) {
        return err!(ClearingHouseError::InvalidProgramId);
    }
    if !ctx.remaining_accounts.is_empty() {
        return err!(ClearingHouseError::UnexpectedAccount);
    }
    Ok(())
}

#[program]
pub mod clearing_house {

    use anchor_spl::token::{transfer_checked, TransferChecked};

    use crate::state::OrderStatus;

    use super::*;

    pub fn initialize_market(ctx: Context<InitializeMarket>) -> Result<()> {
        check_context(&ctx)?;

        require!(
            ctx.accounts.mint_a.key() < ctx.accounts.mint_b.key(),
            ClearingHouseError::InvalidMint
        );

        ctx.accounts.market.mint_a = ctx.accounts.mint_a.key();
        ctx.accounts.market.mint_b = ctx.accounts.mint_b.key();
        ctx.accounts.market.vault_a = ctx.accounts.vault_a.key();
        ctx.accounts.market.vault_b = ctx.accounts.vault_b.key();
        ctx.accounts.market.status = true;
        ctx.accounts.market.bump = ctx.bumps.market;
        Ok(())
    }

    pub fn initialize_user(ctx: Context<InitializeUser>) -> Result<()> {
        check_context(&ctx)?;
        ctx.accounts.user_position.user = ctx.accounts.user.key();
        ctx.accounts.user_position.market = ctx.accounts.market.key();
        ctx.accounts.user_position.bump = ctx.bumps.user_position;

        Ok(())
    }

    pub fn place_order(
        ctx: Context<PlaceOrder>,
        side: OrderSide,
        price: u64,
        amount: u64,
    ) -> Result<()> {
        check_context(&ctx)?;
        require!(amount > 0 && price > 0, ClearingHouseError::InvalidAmount);

        require!(ctx.accounts.market.status, ClearingHouseError::MarketPaused);

        ctx.accounts.order.market = ctx.accounts.market.key();
        ctx.accounts.order.user = ctx.accounts.user.key();
        ctx.accounts.order.amount = amount;
        ctx.accounts.order.price = price;
        ctx.accounts.order.side = side;

        ctx.accounts.order.order_id = ctx.accounts.user_position.order_count;
        ctx.accounts.user_position.order_count += 1;
        ctx.accounts.user_position.open_orders += 1;

        ctx.accounts.order.status = OrderStatus::Open;

        if ctx.accounts.order.side == OrderSide::Buy {
            // buyer wants to receive token a. they pay with token b
            let total_amount = amount as u128 * price as u128;

            require!(total_amount > 0, ClearingHouseError::InvalidAmount);

            let total_amount_u64 =
                u64::try_from(total_amount).map_err(|_| ClearingHouseError::MathOverflow)?;

            let cpi_context = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.user_token_account_b.to_account_info(),
                    to: ctx.accounts.vault_b.to_account_info(),
                    mint: ctx.accounts.mint_b.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            );

            transfer_checked(cpi_context, total_amount_u64, ctx.accounts.mint_b.decimals)?;
        } else {
            // seller wants to give token a. they receive token b
            let cpi_context = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.user_token_account_a.to_account_info(),
                    to: ctx.accounts.vault_a.to_account_info(),
                    mint: ctx.accounts.mint_a.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            );

            transfer_checked(cpi_context, amount, ctx.accounts.mint_a.decimals)?;
        }

        Ok(())
    }

    pub fn match_order(ctx: Context<MatchOrder>) -> Result<()> {
        check_context(&ctx)?;
        require!(
            ctx.accounts.buy_order.price >= ctx.accounts.sell_order.price,
            ClearingHouseError::PriceMismatch
        );

        require!(
            ctx.accounts.buy_order.side == OrderSide::Buy,
            ClearingHouseError::SameSideMatch
        );

        require!(
            ctx.accounts.sell_order.side == OrderSide::Sell,
            ClearingHouseError::SameSideMatch
        );

        let buyer_receives = ctx.accounts.sell_order.amount;

        require!(buyer_receives > 0, ClearingHouseError::InvalidAmount);

        let buyer_deposit =
            ctx.accounts.buy_order.amount as u128 * ctx.accounts.buy_order.price as u128;
        require!(buyer_deposit > 0, ClearingHouseError::InvalidAmount);

        let buyer_deposit_to_u64 =
            u64::try_from(buyer_deposit).map_err(|_| ClearingHouseError::MathOverflow)?;

        let seller_payment =
            ctx.accounts.sell_order.amount as u128 * ctx.accounts.sell_order.price as u128;

        require!(seller_payment > 0, ClearingHouseError::InvalidAmount);

        let seller_payment_to_u64 =
            u64::try_from(seller_payment).map_err(|_| ClearingHouseError::MathOverflow)?;

        let refund_to_buyer = buyer_deposit_to_u64 - seller_payment_to_u64;

        let mint_a_key = ctx.accounts.mint_a.key();
        let mint_b_key = ctx.accounts.mint_b.key();

        // transfer from vault_a to user token account a
        let seeds = [
            b"market",
            mint_a_key.as_ref(),
            mint_b_key.as_ref(),
            &[ctx.accounts.market.bump],
        ];
        let signer_seeds = &[&seeds[..]];

        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                mint: ctx.accounts.mint_a.to_account_info(),
                from: ctx.accounts.vault_a.to_account_info(),
                to: ctx.accounts.buyer_token_account_a.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            signer_seeds,
        );

        transfer_checked(cpi_context, buyer_receives, ctx.accounts.mint_a.decimals)?;

        // transfer from vault_b to seller token account a
        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                mint: ctx.accounts.mint_b.to_account_info(),
                from: ctx.accounts.vault_b.to_account_info(),
                to: ctx.accounts.seller_token_account_b.to_account_info(),
                authority: ctx.accounts.market.to_account_info(),
            },
            signer_seeds,
        );

        transfer_checked(
            cpi_context,
            seller_payment_to_u64,
            ctx.accounts.mint_b.decimals,
        )?;

        if refund_to_buyer > 0 {
            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    mint: ctx.accounts.mint_b.to_account_info(),
                    from: ctx.accounts.vault_b.to_account_info(),
                    to: ctx.accounts.buyer_token_account_b.to_account_info(),
                    authority: ctx.accounts.market.to_account_info(),
                },
                signer_seeds,
            );

            transfer_checked(cpi_context, refund_to_buyer, ctx.accounts.mint_b.decimals)?;
        }
        ctx.accounts.buy_user_position.open_orders -= 1;
        ctx.accounts.sell_user_position.open_orders -= 1;
        ctx.accounts.buy_order.status = OrderStatus::Filled;
        ctx.accounts.sell_order.status = OrderStatus::Filled;
        Ok(())
    }

    pub fn cancel_order(ctx: Context<CancelOrder>) -> Result<()> {
        check_context(&ctx)?;

        require!(
            ctx.accounts.order.amount > 0 && ctx.accounts.order.price > 0,
            ClearingHouseError::InvalidAmount
        );

        let mint_a_key = ctx.accounts.mint_a.key();
        let mint_b_key = ctx.accounts.mint_b.key();

        let seeds = [
            b"market",
            mint_a_key.as_ref(),
            mint_b_key.as_ref(),
            &[ctx.accounts.market.bump],
        ];
        let signer_seeds = &[&seeds[..]];

        if ctx.accounts.order.side == OrderSide::Buy {
            let order_amount = ctx.accounts.order.amount as u128 * ctx.accounts.order.price as u128;
            require!(order_amount > 0, ClearingHouseError::InvalidAmount);

            let order_amount_to_u64 =
                u64::try_from(order_amount).map_err(|_| ClearingHouseError::MathOverflow)?;

            // transfer token b to user
            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault_b.to_account_info(),
                    to: ctx.accounts.user_token_account_b.to_account_info(),
                    mint: ctx.accounts.mint_b.to_account_info(),
                    authority: ctx.accounts.market.to_account_info(),
                },
                signer_seeds,
            );

            transfer_checked(
                cpi_context,
                order_amount_to_u64,
                ctx.accounts.mint_b.decimals,
            )?;
        } else {
            // transfer from vault b to token account b
            // transfer token b to user
            let cpi_context = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault_a.to_account_info(),
                    to: ctx.accounts.user_token_account_a.to_account_info(),
                    mint: ctx.accounts.mint_a.to_account_info(),
                    authority: ctx.accounts.market.to_account_info(),
                },
                signer_seeds,
            );

            transfer_checked(
                cpi_context,
                ctx.accounts.order.amount,
                ctx.accounts.mint_a.decimals,
            )?;
        }

        ctx.accounts.user_position.open_orders -= 1;
        ctx.accounts.order.status = OrderStatus::Cancelled;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub mint_a: Account<'info, Mint>,
    pub mint_b: Account<'info, Mint>,

    #[account(init, payer = payer, space = 8 + Market::INIT_SPACE, seeds = [b"market", mint_a.key().as_ref(), mint_b.key().as_ref()], bump )]
    pub market: Account<'info, Market>,

    #[account(init, payer = payer, associated_token::mint = mint_a, associated_token::authority = market)]
    pub vault_a: Account<'info, TokenAccount>,
    #[account(init, payer = payer, associated_token::mint = mint_b, associated_token::authority = market)]
    pub vault_b: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeUser<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(constraint = mint_a.key() == market.mint_a @ ClearingHouseError::InvalidMint)]
    pub mint_a: Account<'info, Mint>,

    #[account(constraint = mint_b.key() == market.mint_b @ ClearingHouseError::InvalidMint)]
    pub mint_b: Account<'info, Mint>,

    #[account(seeds = [b"market", mint_a.key().as_ref(), mint_b.key().as_ref()], bump )]
    pub market: Account<'info, Market>,

    #[account(init, payer = user, space =
        8 + UserPosition::INIT_SPACE,
        seeds = [b"user_position", user.key().as_ref(), market.key().as_ref()], bump)]
    pub user_position: Account<'info, UserPosition>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PlaceOrder<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(constraint = mint_a.key() == market.mint_a @ ClearingHouseError::InvalidMint)]
    pub mint_a: Account<'info, Mint>,

    #[account(constraint = mint_b.key() == market.mint_b @ ClearingHouseError::InvalidMint)]
    pub mint_b: Account<'info, Mint>,

    #[account(seeds = [b"market", mint_a.key().as_ref(), mint_b.key().as_ref()], bump )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut, seeds = [b"user_position", user.key().as_ref(), market.key().as_ref()], bump )]
    pub user_position: Box<Account<'info, UserPosition>>,

    #[account(init,
        payer = user,
        space = 8 + Order::INIT_SPACE,
        seeds = [b"order", user.key().as_ref(), market.key().as_ref(), user_position.order_count.to_le_bytes().as_ref()],
        bump
    )]
    pub order: Box<Account<'info, Order>>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = market)]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = market)]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = user)]
    pub user_token_account_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = user)]
    pub user_token_account_b: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct MatchOrder<'info> {
    #[account(mut)]
    pub matcher: Signer<'info>,

    #[account(constraint = mint_a.key() == market.mint_a @ ClearingHouseError::InvalidMint)]
    pub mint_a: Account<'info, Mint>,

    #[account(constraint = mint_b.key() == market.mint_b @ ClearingHouseError::InvalidMint)]
    pub mint_b: Account<'info, Mint>,

    #[account(seeds = [b"market", mint_a.key().as_ref(), mint_b.key().as_ref()], bump )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut, constraint = buy_order.market == market.key() && buy_order.status == OrderStatus::Open @ ClearingHouseError::OrderNotOpen)]
    pub buy_order: Box<Account<'info, Order>>,
    #[account(mut, constraint = sell_order.market == market.key() && sell_order.status == OrderStatus::Open @ ClearingHouseError::OrderNotOpen)]
    pub sell_order: Box<Account<'info, Order>>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = market)]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = market)]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = buy_order.user)]
    pub buyer_token_account_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = sell_order.user)]
    pub seller_token_account_b: Box<Account<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_b,
              associated_token::authority = buy_order.user)]
    pub buyer_token_account_b: Box<Account<'info, TokenAccount>>,

    #[account(mut, seeds = [b"user_position", buy_order.user.key().as_ref(), market.key().as_ref()], bump )]
    pub buy_user_position: Box<Account<'info, UserPosition>>,

    #[account(mut, seeds = [b"user_position", sell_order.user.key().as_ref(), market.key().as_ref()], bump )]
    pub sell_user_position: Box<Account<'info, UserPosition>>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelOrder<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(constraint = mint_a.key() == market.mint_a @ ClearingHouseError::InvalidMint)]
    pub mint_a: Account<'info, Mint>,

    #[account(constraint = mint_b.key() == market.mint_b @ ClearingHouseError::InvalidMint)]
    pub mint_b: Account<'info, Mint>,

    #[account(seeds = [b"market", mint_a.key().as_ref(), mint_b.key().as_ref()], bump )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = market)]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = market)]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_a, associated_token::authority = user)]
    pub user_token_account_a: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = mint_b, associated_token::authority = user)]
    pub user_token_account_b: Account<'info, TokenAccount>,

    #[account(mut, constraint = order.market == market.key() && user.key() == order.user && order.status == OrderStatus::Open
        @ ClearingHouseError::OrderNotOpen)]
    pub order: Box<Account<'info, Order>>,

    #[account(mut, seeds = [b"user_position", user.key().as_ref(), market.key().as_ref()], bump )]
    pub user_position: Account<'info, UserPosition>,

    pub token_program: Program<'info, Token>,
}
