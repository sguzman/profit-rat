use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Row, Sqlite};
use std::fs;
use std::str::FromStr;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::error::AppResult;

pub type DbPool = Pool<Sqlite>;

pub async fn connect(config: &AppConfig) -> AppResult<DbPool> {
    let options = SqliteConnectOptions::from_str(&config.database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    migrate_legacy_global_users(&pool, config).await?;
    Ok(pool)
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

async fn migrate_legacy_global_users(pool: &DbPool, config: &AppConfig) -> AppResult<()> {
    let legacy_users = sqlx::query(
        "SELECT discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at
         FROM users",
    )
    .fetch_all(pool)
    .await?;
    if legacy_users.is_empty() {
        return Ok(());
    }

    let mut ambiguous_users = Vec::new();
    for row in legacy_users {
        let discord_user_id: String = row.get("discord_user_id");
        let guild_rows = sqlx::query(
            "SELECT DISTINCT guild_id
             FROM (
                SELECT guild_id FROM markets WHERE creator_discord_user_id = ?1
                UNION
                SELECT m.guild_id
                FROM positions p
                JOIN markets m ON m.id = p.market_id
                WHERE p.discord_user_id = ?1
                UNION
                SELECT m.guild_id
                FROM trades t
                JOIN markets m ON m.id = t.market_id
                WHERE t.discord_user_id = ?1
             )
             WHERE guild_id IS NOT NULL",
        )
        .bind(&discord_user_id)
        .fetch_all(pool)
        .await?;

        if guild_rows.len() != 1 {
            ambiguous_users.push(discord_user_id);
            continue;
        }

        let guild_id: String = guild_rows[0].get("guild_id");
        sqlx::query(
            "INSERT OR IGNORE INTO guild_accounts
             (guild_id, discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(guild_id)
        .bind(row.get::<String, _>("discord_user_id"))
        .bind(row.get::<Option<String>, _>("display_name"))
        .bind(row.get::<i64, _>("balance_mana"))
        .bind(row.get::<i64, _>("total_claimed_mana"))
        .bind(row.get::<Option<String>, _>("last_claim_at"))
        .bind(row.get::<String, _>("created_at"))
        .bind(row.get::<String, _>("updated_at"))
        .execute(pool)
        .await?;
    }

    if !ambiguous_users.is_empty() {
        let backup_path = config.cache_dir.join(format!(
            "discord-bot.legacy-backup.{}.sqlite",
            Utc::now().timestamp()
        ));
        if !backup_path.exists() {
            if let Err(error) = fs::copy(&config.database_path, &backup_path) {
                warn!(%error, backup = %backup_path.display(), "failed to back up legacy database before ambiguous guild migration");
            } else {
                warn!(
                    backup = %backup_path.display(),
                    user_count = ambiguous_users.len(),
                    "backed up legacy database because some global users could not be assigned to exactly one guild"
                );
            }
        }
        warn!(users = ?ambiguous_users, "some legacy users were not migrated into guild accounts and will start fresh per guild");
    } else {
        info!("legacy global users were migrated into guild accounts");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::config::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };

    fn test_config(cache_dir: std::path::PathBuf) -> AppConfig {
        AppConfig {
            discord_token: "test-token".to_string(),
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
    async fn migrations_apply_to_fresh_database() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = test_config(cache_dir.clone());

        config.ensure_runtime_dirs().expect("dirs");
        let pool = super::connect(&config).await.expect("pool");
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
        )
        .fetch_one(&pool)
        .await
        .expect("query");
        assert_eq!(count.0, 1);
    }
}
