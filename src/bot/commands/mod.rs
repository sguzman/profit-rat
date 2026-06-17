mod leaderboard;
mod market;
mod trading;
mod user;

use crate::error::AppError;

pub fn all() -> Vec<poise::Command<crate::bot::Data, AppError>> {
    vec![
        user::ping(),
        user::balance(),
        user::claim(),
        market::create_market(),
        market::list_markets(),
        market::market(),
        market::resolve_market(),
        market::track_manifold(),
        market::manifold_market(),
        market::msync(),
        trading::buy(),
        trading::sell(),
        trading::positions(),
        trading::mpositions(),
        leaderboard::leaderboard(),
    ]
}
