pub mod leaderboard_service;
pub mod market_service;
pub mod social_service;
pub mod trading_service;
pub mod user_service;

use std::sync::Arc;

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::integrations::manifold::ManifoldClient;

#[derive(Clone)]
pub struct Services {
    pub users: user_service::UserService,
    pub markets: market_service::MarketService,
    pub trading: trading_service::TradingService,
    pub social: social_service::SocialService,
    pub leaderboards: leaderboard_service::LeaderboardService,
}

impl Services {
    pub fn new(config: Arc<AppConfig>, pool: DbPool, manifold: Arc<ManifoldClient>) -> Self {
        let users = user_service::UserService::new(config.clone(), pool.clone());
        let markets =
            market_service::MarketService::new(config.clone(), pool.clone(), manifold.clone());
        let trading = trading_service::TradingService::new(config.clone(), pool.clone(), manifold);
        let social = social_service::SocialService::new(config, pool.clone());
        let leaderboards = leaderboard_service::LeaderboardService::new(pool);

        Self {
            users,
            markets,
            trading,
            social,
            leaderboards,
        }
    }
}
