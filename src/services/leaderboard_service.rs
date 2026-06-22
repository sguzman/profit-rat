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

    #[instrument(skip(self), fields(guild_id, limit))]
    pub async fn top_balances(
        &self,
        guild_id: &str,
        limit: i64,
    ) -> AppResult<Vec<LeaderboardEntry>> {
        sqlx::query_as::<_, LeaderboardEntry>(
            "SELECT discord_user_id, display_name, balance_mana
             FROM guild_accounts
             WHERE guild_id = ?1
             ORDER BY balance_mana DESC, discord_user_id ASC
             LIMIT ?2",
        )
        .bind(guild_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
}
