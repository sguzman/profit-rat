use std::sync::Arc;

use chrono::{Duration, Utc};
use sqlx::Row;
use tracing::instrument;

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::domain::market::UserRecord;
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct UserService {
    config: Arc<AppConfig>,
    pool: DbPool,
}

#[derive(Clone, Debug)]
pub struct BalanceSummary {
    pub balance_mana: i64,
    pub total_claimed_mana: i64,
    pub next_claim_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct ClaimReceipt {
    pub amount_mana: i64,
    pub balance_mana: i64,
    pub next_claim_at: chrono::DateTime<Utc>,
}

impl UserService {
    pub fn new(config: Arc<AppConfig>, pool: DbPool) -> Self {
        Self { config, pool }
    }

    #[instrument(skip(self))]
    pub async fn ensure_user(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<UserRecord> {
        if let Some(existing) = sqlx::query_as::<_, UserRecord>(
            "SELECT discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at
             FROM users WHERE discord_user_id = ?1",
        )
        .bind(discord_user_id)
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(existing);
        }

        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO users (discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, NULL, ?4, ?4)",
        )
        .bind(discord_user_id)
        .bind(display_name)
        .bind(self.config.starting_balance)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO balance_events (discord_user_id, amount_mana, reason, related_market_id, created_at)
             VALUES (?1, ?2, 'initial_grant', NULL, ?3)",
        )
        .bind(discord_user_id)
        .bind(self.config.starting_balance)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        sqlx::query_as::<_, UserRecord>(
            "SELECT discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at
             FROM users WHERE discord_user_id = ?1",
        )
        .bind(discord_user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[instrument(skip(self))]
    pub async fn balance(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<BalanceSummary> {
        let user = self.ensure_user(discord_user_id, display_name).await?;
        let next_claim_at = user
            .last_claim_at()
            .map(|last| last + Duration::seconds(self.config.claim_cooldown_seconds));
        Ok(BalanceSummary {
            balance_mana: user.balance_mana,
            total_claimed_mana: user.total_claimed_mana,
            next_claim_at,
        })
    }

    #[instrument(skip(self))]
    pub async fn claim(
        &self,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<ClaimReceipt> {
        let user = self.ensure_user(discord_user_id, display_name).await?;
        let now = Utc::now();
        if let Some(last_claim_at) = user.last_claim_at() {
            let next = last_claim_at + Duration::seconds(self.config.claim_cooldown_seconds);
            if now < next {
                return Err(AppError::Conflict(format!(
                    "claim is on cooldown until {}",
                    next.to_rfc3339()
                )));
            }
        }

        let claimed_at = now.to_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE users
             SET balance_mana = balance_mana + ?2,
                 total_claimed_mana = total_claimed_mana + ?2,
                 last_claim_at = ?3,
                 updated_at = ?3
             WHERE discord_user_id = ?1",
        )
        .bind(discord_user_id)
        .bind(self.config.hourly_claim)
        .bind(&claimed_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO balance_events (discord_user_id, amount_mana, reason, related_market_id, created_at)
             VALUES (?1, ?2, 'hourly_claim', NULL, ?3)",
        )
        .bind(discord_user_id)
        .bind(self.config.hourly_claim)
        .bind(&claimed_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let balance = sqlx::query("SELECT balance_mana FROM users WHERE discord_user_id = ?1")
            .bind(discord_user_id)
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>("balance_mana");

        Ok(ClaimReceipt {
            amount_mana: self.config.hourly_claim,
            balance_mana: balance,
            next_claim_at: now + Duration::seconds(self.config.claim_cooldown_seconds),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::{config::AppConfig, db};

    #[tokio::test]
    async fn claim_enforces_cooldown() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(AppConfig {
            discord_token: "token".to_string(),
            cache_dir: cache_dir.clone(),
            log_dir: cache_dir.join("logs"),
            database_path: cache_dir.join("discord-bot.sqlite"),
            database_url: format!(
                "sqlite://{}",
                cache_dir
                    .join("discord-bot.sqlite")
                    .to_string_lossy()
                    .replace('\\', "/")
            ),
            starting_balance: 1_000,
            hourly_claim: 100,
            claim_cooldown_seconds: 3_600,
            default_liquidity_b: 100.0,
            manifold_api_base_url: "https://api.manifold.markets/v0".to_string(),
            manifold_snapshot_ttl_seconds: 60,
        });
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::UserService::new(config.clone(), pool);

        let first = service.claim("u1", "Test").await.expect("first claim");
        assert_eq!(first.balance_mana, 1_100);

        let second = service.claim("u1", "Test").await;
        assert!(second.is_err());
    }
}
