mod leaderboard;
mod market;
mod social;
mod trading;
mod user;

use crate::error::AppError;

pub fn all() -> Vec<poise::Command<crate::bot::Data, AppError>> {
    vec![
        user::ping(),
        user::balance(),
        user::claim(),
        market::create_market(),
        market::markets(),
        market::list_markets(),
        market::market(),
        market::market_holders(),
        market::resolve_market(),
        market::track_manifold(),
        market::manifold_market(),
        market::msync(),
        trading::buy(),
        trading::sell(),
        trading::offer_shares(),
        trading::incoming_share_offers(),
        trading::accept_share_offer(),
        trading::decline_share_offer(),
        trading::positions(),
        trading::mpositions(),
        social::donate_money(),
        social::donate_shares(),
        social::offer_loan_money(),
        social::offer_loan_shares(),
        social::incoming_loans(),
        social::accept_loan(),
        social::decline_loan(),
        social::loan_status(),
        social::repay_loan(),
        leaderboard::leaderboard(),
    ]
}
