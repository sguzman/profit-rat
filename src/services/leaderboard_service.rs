use sqlx::FromRow;
use tracing::instrument;

use crate::db::DbPool;
use crate::error::AppResult;

#[derive(Clone)]
pub struct LeaderboardService {
    pool: DbPool,
}

#[derive(Debug, FromRow)]
pub struct LeaderboardEntry {
    pub discord_user_id: String,
    pub display_name: Option<String>,
    pub balance_mana: i64,
}

impl LeaderboardService {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    #[instrument(skip(self))]
    pub async fn top_balances(&self, limit: i64) -> AppResult<Vec<LeaderboardEntry>> {
        sqlx::query_as::<_, LeaderboardEntry>(
            "SELECT discord_user_id, display_name, balance_mana
             FROM users ORDER BY balance_mana DESC, discord_user_id ASC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
}
