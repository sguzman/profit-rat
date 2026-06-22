use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, AppResult};

const DEFAULT_CONFIG_PATH: &str = "profit-rat.toml";
const DEFAULT_LOCAL_CONFIG_PATH: &str = "profit-rat.local.toml";

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
    pub share_offer_expiration_seconds: i64,
    pub share_offer_cleanup_interval_seconds: i64,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        dotenvy::dotenv().ok();

        let config_path = env::var("PROFIT_RAT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH));
        let local_config_path = env::var("PROFIT_RAT_LOCAL_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_LOCAL_CONFIG_PATH));

        let file_config = read_optional_config(&config_path)?;
        let local_config = read_optional_config(&local_config_path)?;
        let merged = file_config.merge(local_config);

        let cache_dir = PathBuf::from(
            merged
                .cache_dir
                .or_else(|| env::var("CACHE_DIR").ok())
                .unwrap_or_else(|| ".cache".to_string()),
        );
        let log_dir = cache_dir.join("logs");
        let database_path = cache_dir.join("discord-bot.sqlite");

        let discord_token = merged
            .discord_token
            .or_else(|| env::var("DISCORD_TOKEN").ok())
            .unwrap_or_default();
        let database_url = merged
            .database_url
            .or_else(|| env::var("DATABASE_URL").ok())
            .unwrap_or_else(|| format!("sqlite://{}", normalize_for_sqlite_url(&database_path)));

        Ok(Self {
            discord_token,
            cache_dir,
            log_dir,
            database_path,
            database_url,
            starting_balance: merged
                .policies
                .starting_balance
                .map(Ok)
                .unwrap_or_else(|| env_i64("STARTING_BALANCE", 1_000))?,
            hourly_claim: merged
                .policies
                .hourly_claim
                .map(Ok)
                .unwrap_or_else(|| env_i64("HOURLY_CLAIM", 100))?,
            claim_cooldown_seconds: merged
                .policies
                .claim_cooldown_seconds
                .map(Ok)
                .unwrap_or_else(|| env_i64("CLAIM_COOLDOWN_SECONDS", 3_600))?,
            default_liquidity_b: merged
                .policies
                .default_liquidity_b
                .map(Ok)
                .unwrap_or_else(|| env_f64("DEFAULT_LIQUIDITY_B", 100.0))?,
            manifold_api_base_url: merged
                .manifold
                .api_base_url
                .or_else(|| env::var("MANIFOLD_API_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.manifold.markets/v0".to_string()),
            manifold_snapshot_ttl_seconds: merged
                .manifold
                .snapshot_ttl_seconds
                .map(Ok)
                .unwrap_or_else(|| env_i64("MANIFOLD_SNAPSHOT_TTL_SECONDS", 60))?,
            manifold_poll_interval_seconds: merged
                .manifold
                .poll_interval_seconds
                .map(Ok)
                .unwrap_or_else(|| env_i64("MANIFOLD_POLL_INTERVAL_SECONDS", 120))?,
            share_offer_expiration_seconds: merged
                .policies
                .share_offer_expiration_seconds
                .unwrap_or(60),
            share_offer_cleanup_interval_seconds: merged
                .policies
                .share_offer_cleanup_interval_seconds
                .unwrap_or(15),
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

fn read_optional_config(path: &Path) -> AppResult<PartialConfig> {
    if !path.exists() {
        return Ok(PartialConfig::default());
    }

    let contents = fs::read_to_string(path)?;
    toml::from_str::<PartialConfig>(&contents)
        .map_err(|error| AppError::Config(format!("failed to parse `{}`: {error}", path.display())))
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialConfig {
    #[serde(default)]
    discord_token: Option<String>,
    #[serde(default)]
    cache_dir: Option<String>,
    #[serde(default)]
    database_url: Option<String>,
    #[serde(default)]
    policies: PolicyConfig,
    #[serde(default)]
    manifold: ManifoldConfig,
}

impl PartialConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            discord_token: overlay.discord_token.or(self.discord_token),
            cache_dir: overlay.cache_dir.or(self.cache_dir),
            database_url: overlay.database_url.or(self.database_url),
            policies: self.policies.merge(overlay.policies),
            manifold: self.manifold.merge(overlay.manifold),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PolicyConfig {
    #[serde(default)]
    starting_balance: Option<i64>,
    #[serde(default)]
    hourly_claim: Option<i64>,
    #[serde(default)]
    claim_cooldown_seconds: Option<i64>,
    #[serde(default)]
    default_liquidity_b: Option<f64>,
    #[serde(default)]
    share_offer_expiration_seconds: Option<i64>,
    #[serde(default)]
    share_offer_cleanup_interval_seconds: Option<i64>,
}

impl PolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            starting_balance: overlay.starting_balance.or(self.starting_balance),
            hourly_claim: overlay.hourly_claim.or(self.hourly_claim),
            claim_cooldown_seconds: overlay
                .claim_cooldown_seconds
                .or(self.claim_cooldown_seconds),
            default_liquidity_b: overlay.default_liquidity_b.or(self.default_liquidity_b),
            share_offer_expiration_seconds: overlay
                .share_offer_expiration_seconds
                .or(self.share_offer_expiration_seconds),
            share_offer_cleanup_interval_seconds: overlay
                .share_offer_cleanup_interval_seconds
                .or(self.share_offer_cleanup_interval_seconds),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ManifoldConfig {
    #[serde(default)]
    api_base_url: Option<String>,
    #[serde(default)]
    snapshot_ttl_seconds: Option<i64>,
    #[serde(default)]
    poll_interval_seconds: Option<i64>,
}

impl ManifoldConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            api_base_url: overlay.api_base_url.or(self.api_base_url),
            snapshot_ttl_seconds: overlay.snapshot_ttl_seconds.or(self.snapshot_ttl_seconds),
            poll_interval_seconds: overlay.poll_interval_seconds.or(self.poll_interval_seconds),
        }
    }
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
            share_offer_expiration_seconds: 60,
            share_offer_cleanup_interval_seconds: 15,
        };

        config.ensure_runtime_dirs().expect("dirs");
        assert!(config.cache_dir.exists());
        assert!(config.log_dir.exists());
    }
}
