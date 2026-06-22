use std::env;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub discord_token: String,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub database_path: PathBuf,
    pub database_url: String,
    pub starting_balance: i64,
    pub hourly_claim: i64,
    pub claim_cooldown_seconds: i64,
    pub default_liquidity_b: f64,
    pub manifold_api_base_url: String,
    pub manifold_snapshot_ttl_seconds: i64,
    pub manifold_poll_interval_seconds: i64,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        dotenvy::dotenv().ok();

        let cache_dir =
            PathBuf::from(env::var("CACHE_DIR").unwrap_or_else(|_| ".cache".to_string()));
        let log_dir = cache_dir.join("logs");
        let database_path = cache_dir.join("discord-bot.sqlite");

        let discord_token = env::var("DISCORD_TOKEN").unwrap_or_default();
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| format!("sqlite://{}", normalize_for_sqlite_url(&database_path)));

        Ok(Self {
            discord_token,
            cache_dir,
            log_dir,
            database_path,
            database_url,
            starting_balance: env_i64("STARTING_BALANCE", 1_000)?,
            hourly_claim: env_i64("HOURLY_CLAIM", 100)?,
            claim_cooldown_seconds: env_i64("CLAIM_COOLDOWN_SECONDS", 3_600)?,
            default_liquidity_b: env_f64("DEFAULT_LIQUIDITY_B", 100.0)?,
            manifold_api_base_url: env::var("MANIFOLD_API_BASE_URL")
                .unwrap_or_else(|_| "https://api.manifold.markets/v0".to_string()),
            manifold_snapshot_ttl_seconds: env_i64("MANIFOLD_SNAPSHOT_TTL_SECONDS", 60)?,
            manifold_poll_interval_seconds: env_i64("MANIFOLD_POLL_INTERVAL_SECONDS", 120)?,
        })
    }

    pub fn ensure_runtime_dirs(&self) -> AppResult<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(&self.log_dir)?;
        Ok(())
    }

    pub fn validate_for_runtime(&self) -> AppResult<()> {
        if self.discord_token.trim().is_empty() {
            return Err(AppError::MissingDiscordToken);
        }

        Ok(())
    }
}

fn env_i64(name: &str, default: i64) -> AppResult<i64> {
    match env::var(name) {
        Ok(value) => value
            .parse::<i64>()
            .map_err(|_| AppError::Config(format!("failed to parse `{name}` as i64"))),
        Err(_) => Ok(default),
    }
}

fn env_f64(name: &str, default: f64) -> AppResult<f64> {
    match env::var(name) {
        Ok(value) => value
            .parse::<f64>()
            .map_err(|_| AppError::Config(format!("failed to parse `{name}` as f64"))),
        Err(_) => Ok(default),
    }
}

fn normalize_for_sqlite_url(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::AppConfig;

    #[test]
    fn config_defaults_to_cache_paths() {
        let config = AppConfig::from_env().expect("config should load");
        assert_eq!(config.cache_dir, PathBuf::from(".cache"));
        assert!(config.database_url.contains(".cache/discord-bot.sqlite"));
        assert_eq!(config.log_dir, PathBuf::from(".cache/logs"));
    }

    #[test]
    fn ensure_runtime_dirs_creates_cache_subdirectories() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = AppConfig {
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
            manifold_poll_interval_seconds: 120,
        };

        config.ensure_runtime_dirs().expect("dirs");
        assert!(config.cache_dir.exists());
        assert!(config.log_dir.exists());
    }
}
