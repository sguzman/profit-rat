use std::sync::Arc;

use chrono::{Duration, Utc};
use sqlx::Row;
use tracing::instrument;

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::domain::market::GuildAccountRecord;
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

    #[instrument(skip(self), fields(guild_id, discord_user_id))]
    pub async fn ensure_account(
        &self,
        guild_id: &str,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<GuildAccountRecord> {
        if let Some(existing) = sqlx::query_as::<_, GuildAccountRecord>(
            "SELECT id, guild_id, discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at
             FROM guild_accounts
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .fetch_optional(&self.pool)
        .await?
        {
            if existing.display_name.as_deref() != Some(display_name) {
                sqlx::query(
                    "UPDATE guild_accounts
                     SET display_name = ?3, updated_at = ?4
                     WHERE guild_id = ?1 AND discord_user_id = ?2",
                )
                .bind(guild_id)
                .bind(discord_user_id)
                .bind(display_name)
                .bind(now_rfc3339())
                .execute(&self.pool)
                .await?;
            }
            return Ok(existing);
        }

        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO guild_accounts
             (guild_id, discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, NULL, ?5, ?5)",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .bind(display_name)
        .bind(self.config.starting_balance)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO economy_events
             (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
             VALUES (?1, ?2, NULL, NULL, 'money', ?3, NULL, 'initial_grant', 'guild bootstrap balance', ?4)",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .bind(self.config.starting_balance)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        sqlx::query_as::<_, GuildAccountRecord>(
            "SELECT id, guild_id, discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at
             FROM guild_accounts
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[instrument(skip(self), fields(guild_id, discord_user_id))]
    pub async fn balance(
        &self,
        guild_id: &str,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<BalanceSummary> {
        let account = self
            .ensure_account(guild_id, discord_user_id, display_name)
            .await?;
        let next_claim_at = account
            .last_claim_at()
            .map(|last| last + Duration::seconds(self.config.claim_period_seconds));
        Ok(BalanceSummary {
            balance_mana: account.balance_mana,
            total_claimed_mana: account.total_claimed_mana,
            next_claim_at,
        })
    }

    #[instrument(skip(self), fields(guild_id, discord_user_id))]
    pub async fn claim(
        &self,
        guild_id: &str,
        discord_user_id: &str,
        display_name: &str,
    ) -> AppResult<ClaimReceipt> {
        let account = self
            .ensure_account(guild_id, discord_user_id, display_name)
            .await?;
        let now = Utc::now();
        if let Some(last_claim_at) = account.last_claim_at() {
            let next = last_claim_at + Duration::seconds(self.config.claim_period_seconds);
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
            "UPDATE guild_accounts
             SET balance_mana = balance_mana + ?2,
                 total_claimed_mana = total_claimed_mana + ?2,
                 last_claim_at = ?3,
                 updated_at = ?3
             WHERE guild_id = ?1 AND discord_user_id = ?4",
        )
        .bind(guild_id)
        .bind(self.config.claim_amount)
        .bind(&claimed_at)
        .bind(discord_user_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO economy_events
             (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
             VALUES (?1, ?2, NULL, NULL, 'money', ?3, NULL, 'period_claim', 'login claim payout', ?4)",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .bind(self.config.claim_amount)
        .bind(&claimed_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let balance = sqlx::query(
            "SELECT balance_mana
             FROM guild_accounts
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(discord_user_id)
        .fetch_one(&self.pool)
        .await?
        .get::<i64, _>("balance_mana");

        Ok(ClaimReceipt {
            amount_mana: self.config.claim_amount,
            balance_mana: balance,
            next_claim_at: now + Duration::seconds(self.config.claim_period_seconds),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::config::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };
    use crate::db;

    fn test_config(cache_dir: std::path::PathBuf) -> AppConfig {
        AppConfig {
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
            claim_amount: 10_000,
            claim_period_seconds: 43_200,
            claim_period_name: "twice-daily login".to_string(),
            default_liquidity_b: 100.0,
            share_offer_expiration_seconds: 60,
            share_offer_cleanup_interval_seconds: 15,
            manifold_api_base_url: "https://api.manifold.markets/v0".to_string(),
            manifold_snapshot_ttl_seconds: 60,
            manifold_poll_interval_seconds: 120,
            policies: PolicyConfig {
                starting_balance: 1_000,
                claim_amount: 10_000,
                claim_period_seconds: 43_200,
                claim_period_name: "twice-daily login".to_string(),
                default_liquidity_b: 100.0,
                share_offer_expiration_seconds: 60,
                share_offer_cleanup_interval_seconds: 15,
            },
            transfers: TransferPolicyConfig {
                allow_money_donations: true,
                allow_share_donations: true,
                allow_money_offers: true,
                allow_share_offers: true,
                min_money_transfer: 1,
                min_share_transfer: 0.01,
                max_open_offers_per_user: 10,
            },
            loans: LoanPolicyConfig {
                allow_money_loans: true,
                allow_share_loans: true,
                allow_partial_repayment: true,
                allow_early_repayment: true,
                allow_interest: true,
                default_interest_bps: 0,
                max_interest_bps: 2_500,
                default_duration_seconds: 86_400,
                max_duration_seconds: 2_592_000,
                max_open_loans_per_user: 10,
            },
            bot: BotPolicyConfig {
                auto_claim: true,
                auto_accept_loans: true,
                loan_required_interest_bps: 500,
                min_loan_duration_seconds: 3_600,
                worker_interval_seconds: 60,
            },
            bonds: BondPolicyConfig {
                enabled: true,
                default_yield_period_seconds: 3_600,
                max_yield_bps: 5_000,
                min_maturity_seconds: 3_600,
                max_maturity_seconds: 7_776_000,
                max_open_issuances_per_user: 10,
                worker_interval_seconds: 60,
            },
            manifold: ManifoldConfig {
                api_base_url: "https://api.manifold.markets/v0".to_string(),
                snapshot_ttl_seconds: 60,
                poll_interval_seconds: 120,
            },
            currency: CurrencyConfig {
                code: "MANA".to_string(),
                display_name: "Fake Mana".to_string(),
                singular: "mana".to_string(),
                plural: "mana".to_string(),
                symbol: "$".to_string(),
                textual_symbol: "mana".to_string(),
                emoji: "💰".to_string(),
                custom_emoji: String::new(),
                image_symbol_path: String::new(),
                image_symbol_url: String::new(),
                position: CurrencyPosition::Suffix,
                space_between: true,
                show_symbol: false,
                show_textual_symbol: true,
                show_code: false,
                use_emoji_in_embeds: true,
                use_emoji_in_plaintext: true,
                decimals: 0,
                thousands_separator: ",".to_string(),
                negative_style: NegativeStyle::Minus,
                short_suffixes: false,
            },
        }
    }

    #[tokio::test]
    async fn claim_enforces_cooldown() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(test_config(cache_dir.clone()));
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::UserService::new(config.clone(), pool);

        let first = service
            .claim("guild-a", "u1", "Test")
            .await
            .expect("first claim");
        assert_eq!(first.balance_mana, 11_000);

        let second = service.claim("guild-a", "u1", "Test").await;
        assert!(second.is_err());
    }
}
