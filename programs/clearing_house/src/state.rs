use anchor_lang::prelude::*;

#[derive(AnchorSerialize, InitSpace, AnchorDeserialize, Clone, PartialEq)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(AnchorSerialize, InitSpace, AnchorDeserialize, Clone, PartialEq)]
pub enum OrderStatus {
    Open,
    Filled,
    Cancelled,
}

#[account]
#[derive(InitSpace)]
pub struct Market {
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub bump: u8,
    pub status: bool,
}

#[account]
#[derive(InitSpace)]
pub struct Order {
    pub market: Pubkey,
    pub user: Pubkey,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub order_id: u64,
    pub price: u64,
    pub amount: u64,
    pub filled_amount: u64,
}

#[account]
#[derive(InitSpace)]
pub struct UserPosition {
    pub user: Pubkey,
    pub market: Pubkey,
    pub order_count: u64,
    pub open_orders: u64,
    pub bump: u8,
}
