use anchor_lang::error_code;

#[error_code]
pub enum ClearingHouseError {
    #[msg("Order price must be greater than zero")]
    InvalidPrice,

    #[msg("Order amount must be greater than zero")]
    InvalidAmount,

    #[msg("Order does not belong to this market")]
    InvalidMarket,

    #[msg("Order does not belong to this user")]
    InvalidOrderOwner,

    #[msg("Order is not open")]
    OrderNotOpen,

    #[msg(
        "Invalid program id. For using program from another account please update id in the code"
    )]
    InvalidProgramId, // 6011 0x177b

    #[msg("Unexpected account")]
    UnexpectedAccount, // 6012 0x177c

    #[msg("Buy price must be greater than or equal to sell price for a match")]
    PriceMismatch,

    #[msg("Buy order and sell order must be on the same market")]
    MarketMismatch,

    #[msg("Cannot match two orders on the same side")]
    SameSideMatch,

    #[msg("Insufficient funds to place order")]
    InsufficientFunds,

    #[msg("Math overflow")]
    MathOverflow,

    #[msg("Invalid vault for this market")]
    InvalidVault,

    #[msg("Invalid mint for this market")]
    InvalidMint,

    #[msg("User position does not belong to this market")]
    InvalidUserPosition,

    #[msg("Unauthorized signer")]
    Unauthorized,

    #[msg("Market is paused")]
    MarketPaused,
}
