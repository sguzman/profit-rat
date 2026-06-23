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
    pub policies: PolicyConfig,
    pub transfers: TransferPolicyConfig,
    pub loans: LoanPolicyConfig,
    pub bot: BotPolicyConfig,
    pub bonds: BondPolicyConfig,
    pub manifold: ManifoldConfig,
    pub currency: CurrencyConfig,
    pub starting_balance: i64,
    pub claim_amount: i64,
    pub claim_period_seconds: i64,
    pub claim_period_name: String,
    pub default_liquidity_b: f64,
    pub manifold_api_base_url: String,
    pub manifold_snapshot_ttl_seconds: i64,
    pub manifold_poll_interval_seconds: i64,
    pub share_offer_expiration_seconds: i64,
    pub share_offer_cleanup_interval_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct PolicyConfig {
    pub starting_balance: i64,
    pub claim_amount: i64,
    pub claim_period_seconds: i64,
    pub claim_period_name: String,
    pub default_liquidity_b: f64,
    pub share_offer_expiration_seconds: i64,
    pub share_offer_cleanup_interval_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct TransferPolicyConfig {
    pub allow_money_donations: bool,
    pub allow_share_donations: bool,
    pub allow_money_offers: bool,
    pub allow_share_offers: bool,
    pub min_money_transfer: i64,
    pub min_share_transfer: f64,
    pub max_open_offers_per_user: i64,
}

#[derive(Clone, Debug)]
pub struct LoanPolicyConfig {
    pub allow_money_loans: bool,
    pub allow_share_loans: bool,
    pub allow_partial_repayment: bool,
    pub allow_early_repayment: bool,
    pub allow_interest: bool,
    pub default_interest_bps: i64,
    pub max_interest_bps: i64,
    pub default_duration_seconds: i64,
    pub max_duration_seconds: i64,
    pub max_open_loans_per_user: i64,
}

#[derive(Clone, Debug)]
pub struct BotPolicyConfig {
    pub auto_claim: bool,
    pub auto_accept_loans: bool,
    pub max_loan_interest_bps: i64,
    pub min_loan_duration_seconds: i64,
    pub auto_buy_bonds: bool,
    pub min_bond_yield_bps: i64,
    pub max_bond_yield_bps: i64,
    pub min_bond_maturity_seconds: i64,
    pub max_bond_maturity_seconds: i64,
    pub max_bond_price_mana: i64,
    pub max_bond_purchase_quantity: i64,
    pub max_total_bond_exposure_mana: i64,
    pub worker_interval_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct BondPolicyConfig {
    pub enabled: bool,
    pub default_yield_period_seconds: i64,
    pub max_yield_bps: i64,
    pub min_maturity_seconds: i64,
    pub max_maturity_seconds: i64,
    pub max_open_issuances_per_user: i64,
    pub worker_interval_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct ManifoldConfig {
    pub api_base_url: String,
    pub snapshot_ttl_seconds: i64,
    pub poll_interval_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct CurrencyConfig {
    pub code: String,
    pub display_name: String,
    pub singular: String,
    pub plural: String,
    pub symbol: String,
    pub textual_symbol: String,
    pub emoji: String,
    pub custom_emoji: String,
    pub image_symbol_path: String,
    pub image_symbol_url: String,
    pub position: CurrencyPosition,
    pub space_between: bool,
    pub show_symbol: bool,
    pub show_textual_symbol: bool,
    pub show_code: bool,
    pub use_emoji_in_embeds: bool,
    pub use_emoji_in_plaintext: bool,
    pub decimals: usize,
    pub thousands_separator: String,
    pub negative_style: NegativeStyle,
    pub short_suffixes: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum CurrencyPosition {
    Prefix,
    Suffix,
}

#[derive(Clone, Copy, Debug)]
pub enum NegativeStyle {
    Minus,
    Parens,
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
            .filter(|value| !value.trim().is_empty())
            .or_else(|| env::var("DISCORD_TOKEN").ok())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_default();
        let database_url = merged
            .database_url
            .or_else(|| env::var("DATABASE_URL").ok())
            .unwrap_or_else(|| format!("sqlite://{}", normalize_for_sqlite_url(&database_path)));

        let policy = PolicyConfig {
            starting_balance: merged
                .policies
                .starting_balance
                .map(Ok)
                .unwrap_or_else(|| env_i64("STARTING_BALANCE", 1_000))?,
            claim_amount: merged
                .policies
                .claim_amount
                .or(merged.policies.hourly_claim)
                .map(Ok)
                .unwrap_or_else(|| env_i64("CLAIM_AMOUNT", 10_000))?,
            claim_period_seconds: merged
                .policies
                .claim_period_seconds
                .or(merged.policies.claim_cooldown_seconds)
                .map(Ok)
                .unwrap_or_else(|| env_i64("CLAIM_PERIOD_SECONDS", 43_200))?,
            claim_period_name: merged
                .policies
                .claim_period_name
                .unwrap_or_else(|| "twice-daily login".to_string()),
            default_liquidity_b: merged
                .policies
                .default_liquidity_b
                .map(Ok)
                .unwrap_or_else(|| env_f64("DEFAULT_LIQUIDITY_B", 100.0))?,
            share_offer_expiration_seconds: merged
                .policies
                .share_offer_expiration_seconds
                .unwrap_or(60),
            share_offer_cleanup_interval_seconds: merged
                .policies
                .share_offer_cleanup_interval_seconds
                .unwrap_or(15),
        };

        let transfers = TransferPolicyConfig {
            allow_money_donations: merged.transfers.allow_money_donations.unwrap_or(true),
            allow_share_donations: merged.transfers.allow_share_donations.unwrap_or(true),
            allow_money_offers: merged.transfers.allow_money_offers.unwrap_or(true),
            allow_share_offers: merged.transfers.allow_share_offers.unwrap_or(true),
            min_money_transfer: merged.transfers.min_money_transfer.unwrap_or(1),
            min_share_transfer: merged.transfers.min_share_transfer.unwrap_or(0.01),
            max_open_offers_per_user: merged.transfers.max_open_offers_per_user.unwrap_or(25),
        };

        let loans = LoanPolicyConfig {
            allow_money_loans: merged.loans.allow_money_loans.unwrap_or(true),
            allow_share_loans: merged.loans.allow_share_loans.unwrap_or(true),
            allow_partial_repayment: merged.loans.allow_partial_repayment.unwrap_or(true),
            allow_early_repayment: merged.loans.allow_early_repayment.unwrap_or(true),
            allow_interest: merged.loans.allow_interest.unwrap_or(true),
            default_interest_bps: merged.loans.default_interest_bps.unwrap_or(0),
            max_interest_bps: merged.loans.max_interest_bps.unwrap_or(2_500),
            default_duration_seconds: merged.loans.default_duration_seconds.unwrap_or(86_400),
            max_duration_seconds: merged.loans.max_duration_seconds.unwrap_or(2_592_000),
            max_open_loans_per_user: merged.loans.max_open_loans_per_user.unwrap_or(10),
        };

        let bot = BotPolicyConfig {
            auto_claim: merged.bot.auto_claim.unwrap_or(true),
            auto_accept_loans: merged.bot.auto_accept_loans.unwrap_or(true),
            max_loan_interest_bps: merged.bot.max_loan_interest_bps.unwrap_or(500),
            min_loan_duration_seconds: merged.bot.min_loan_duration_seconds.unwrap_or(3_600),
            auto_buy_bonds: merged.bot.auto_buy_bonds.unwrap_or(true),
            min_bond_yield_bps: merged.bot.min_bond_yield_bps.unwrap_or(100),
            max_bond_yield_bps: merged.bot.max_bond_yield_bps.unwrap_or(500),
            min_bond_maturity_seconds: merged.bot.min_bond_maturity_seconds.unwrap_or(3_600),
            max_bond_maturity_seconds: merged.bot.max_bond_maturity_seconds.unwrap_or(86_400),
            max_bond_price_mana: merged.bot.max_bond_price_mana.unwrap_or(5_000),
            max_bond_purchase_quantity: merged.bot.max_bond_purchase_quantity.unwrap_or(1),
            max_total_bond_exposure_mana: merged
                .bot
                .max_total_bond_exposure_mana
                .unwrap_or(20_000),
            worker_interval_seconds: merged.bot.worker_interval_seconds.unwrap_or(60),
        };

        let bonds = BondPolicyConfig {
            enabled: merged.bonds.enabled.unwrap_or(true),
            default_yield_period_seconds: merged
                .bonds
                .default_yield_period_seconds
                .unwrap_or(3_600),
            max_yield_bps: merged.bonds.max_yield_bps.unwrap_or(5_000),
            min_maturity_seconds: merged.bonds.min_maturity_seconds.unwrap_or(3_600),
            max_maturity_seconds: merged.bonds.max_maturity_seconds.unwrap_or(7_776_000),
            max_open_issuances_per_user: merged.bonds.max_open_issuances_per_user.unwrap_or(10),
            worker_interval_seconds: merged.bonds.worker_interval_seconds.unwrap_or(60),
        };

        let manifold = ManifoldConfig {
            api_base_url: merged
                .manifold
                .api_base_url
                .or_else(|| env::var("MANIFOLD_API_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.manifold.markets/v0".to_string()),
            snapshot_ttl_seconds: merged.manifold.snapshot_ttl_seconds.unwrap_or(60),
            poll_interval_seconds: merged.manifold.poll_interval_seconds.unwrap_or(120),
        };

        let currency = CurrencyConfig {
            code: merged.currency.code.unwrap_or_else(|| "MANA".to_string()),
            display_name: merged
                .currency
                .display_name
                .unwrap_or_else(|| "Fake Mana".to_string()),
            singular: merged
                .currency
                .singular
                .unwrap_or_else(|| "mana".to_string()),
            plural: merged.currency.plural.unwrap_or_else(|| "mana".to_string()),
            symbol: merged.currency.symbol.unwrap_or_else(|| "".to_string()),
            textual_symbol: merged
                .currency
                .textual_symbol
                .unwrap_or_else(|| "mana".to_string()),
            emoji: merged.currency.emoji.unwrap_or_else(|| "💰".to_string()),
            custom_emoji: merged.currency.custom_emoji.unwrap_or_default(),
            image_symbol_path: merged.currency.image_symbol_path.unwrap_or_default(),
            image_symbol_url: merged.currency.image_symbol_url.unwrap_or_default(),
            position: CurrencyPosition::from_str(
                merged.currency.position.as_deref().unwrap_or("suffix"),
            ),
            space_between: merged.currency.space_between.unwrap_or(true),
            show_symbol: merged.currency.show_symbol.unwrap_or(false),
            show_textual_symbol: merged.currency.show_textual_symbol.unwrap_or(true),
            show_code: merged.currency.show_code.unwrap_or(false),
            use_emoji_in_embeds: merged.currency.use_emoji_in_embeds.unwrap_or(true),
            use_emoji_in_plaintext: merged.currency.use_emoji_in_plaintext.unwrap_or(false),
            decimals: merged.currency.decimals.unwrap_or(0),
            thousands_separator: merged
                .currency
                .thousands_separator
                .unwrap_or_else(|| ",".to_string()),
            negative_style: NegativeStyle::from_str(
                merged.currency.negative_style.as_deref().unwrap_or("minus"),
            ),
            short_suffixes: merged.currency.short_suffixes.unwrap_or(true),
        };

        Ok(Self {
            discord_token,
            cache_dir,
            log_dir,
            database_path,
            database_url,
            policies: policy.clone(),
            transfers,
            loans,
            bot,
            bonds,
            manifold: manifold.clone(),
            currency,
            starting_balance: policy.starting_balance,
            claim_amount: policy.claim_amount,
            claim_period_seconds: policy.claim_period_seconds,
            claim_period_name: policy.claim_period_name.clone(),
            default_liquidity_b: policy.default_liquidity_b,
            manifold_api_base_url: manifold.api_base_url.clone(),
            manifold_snapshot_ttl_seconds: manifold.snapshot_ttl_seconds,
            manifold_poll_interval_seconds: manifold.poll_interval_seconds,
            share_offer_expiration_seconds: policy.share_offer_expiration_seconds,
            share_offer_cleanup_interval_seconds: policy.share_offer_cleanup_interval_seconds,
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

impl CurrencyPosition {
    fn from_str(value: &str) -> Self {
        match value {
            "prefix" => Self::Prefix,
            _ => Self::Suffix,
        }
    }
}

impl NegativeStyle {
    fn from_str(value: &str) -> Self {
        match value {
            "parens" => Self::Parens,
            _ => Self::Minus,
        }
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
    policies: PartialPolicyConfig,
    #[serde(default)]
    transfers: PartialTransferPolicyConfig,
    #[serde(default)]
    loans: PartialLoanPolicyConfig,
    #[serde(default)]
    bot: PartialBotPolicyConfig,
    #[serde(default)]
    bonds: PartialBondPolicyConfig,
    #[serde(default)]
    manifold: PartialManifoldConfig,
    #[serde(default)]
    currency: PartialCurrencyConfig,
}

impl PartialConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            discord_token: overlay.discord_token.or(self.discord_token),
            cache_dir: overlay.cache_dir.or(self.cache_dir),
            database_url: overlay.database_url.or(self.database_url),
            policies: self.policies.merge(overlay.policies),
            transfers: self.transfers.merge(overlay.transfers),
            loans: self.loans.merge(overlay.loans),
            bot: self.bot.merge(overlay.bot),
            bonds: self.bonds.merge(overlay.bonds),
            manifold: self.manifold.merge(overlay.manifold),
            currency: self.currency.merge(overlay.currency),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialPolicyConfig {
    #[serde(default)]
    starting_balance: Option<i64>,
    #[serde(default)]
    claim_amount: Option<i64>,
    #[serde(default)]
    hourly_claim: Option<i64>,
    #[serde(default)]
    claim_period_seconds: Option<i64>,
    #[serde(default)]
    claim_cooldown_seconds: Option<i64>,
    #[serde(default)]
    claim_period_name: Option<String>,
    #[serde(default)]
    default_liquidity_b: Option<f64>,
    #[serde(default)]
    share_offer_expiration_seconds: Option<i64>,
    #[serde(default)]
    share_offer_cleanup_interval_seconds: Option<i64>,
}

impl PartialPolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            starting_balance: overlay.starting_balance.or(self.starting_balance),
            claim_amount: overlay.claim_amount.or(self.claim_amount),
            hourly_claim: overlay.hourly_claim.or(self.hourly_claim),
            claim_period_seconds: overlay.claim_period_seconds.or(self.claim_period_seconds),
            claim_cooldown_seconds: overlay
                .claim_cooldown_seconds
                .or(self.claim_cooldown_seconds),
            claim_period_name: overlay.claim_period_name.or(self.claim_period_name),
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
struct PartialTransferPolicyConfig {
    #[serde(default)]
    allow_money_donations: Option<bool>,
    #[serde(default)]
    allow_share_donations: Option<bool>,
    #[serde(default)]
    allow_money_offers: Option<bool>,
    #[serde(default)]
    allow_share_offers: Option<bool>,
    #[serde(default)]
    min_money_transfer: Option<i64>,
    #[serde(default)]
    min_share_transfer: Option<f64>,
    #[serde(default)]
    max_open_offers_per_user: Option<i64>,
}

impl PartialTransferPolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            allow_money_donations: overlay.allow_money_donations.or(self.allow_money_donations),
            allow_share_donations: overlay.allow_share_donations.or(self.allow_share_donations),
            allow_money_offers: overlay.allow_money_offers.or(self.allow_money_offers),
            allow_share_offers: overlay.allow_share_offers.or(self.allow_share_offers),
            min_money_transfer: overlay.min_money_transfer.or(self.min_money_transfer),
            min_share_transfer: overlay.min_share_transfer.or(self.min_share_transfer),
            max_open_offers_per_user: overlay
                .max_open_offers_per_user
                .or(self.max_open_offers_per_user),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialLoanPolicyConfig {
    #[serde(default)]
    allow_money_loans: Option<bool>,
    #[serde(default)]
    allow_share_loans: Option<bool>,
    #[serde(default)]
    allow_partial_repayment: Option<bool>,
    #[serde(default)]
    allow_early_repayment: Option<bool>,
    #[serde(default)]
    allow_interest: Option<bool>,
    #[serde(default)]
    default_interest_bps: Option<i64>,
    #[serde(default)]
    max_interest_bps: Option<i64>,
    #[serde(default)]
    default_duration_seconds: Option<i64>,
    #[serde(default)]
    max_duration_seconds: Option<i64>,
    #[serde(default)]
    max_open_loans_per_user: Option<i64>,
}

impl PartialLoanPolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            allow_money_loans: overlay.allow_money_loans.or(self.allow_money_loans),
            allow_share_loans: overlay.allow_share_loans.or(self.allow_share_loans),
            allow_partial_repayment: overlay
                .allow_partial_repayment
                .or(self.allow_partial_repayment),
            allow_early_repayment: overlay.allow_early_repayment.or(self.allow_early_repayment),
            allow_interest: overlay.allow_interest.or(self.allow_interest),
            default_interest_bps: overlay.default_interest_bps.or(self.default_interest_bps),
            max_interest_bps: overlay.max_interest_bps.or(self.max_interest_bps),
            default_duration_seconds: overlay
                .default_duration_seconds
                .or(self.default_duration_seconds),
            max_duration_seconds: overlay.max_duration_seconds.or(self.max_duration_seconds),
            max_open_loans_per_user: overlay
                .max_open_loans_per_user
                .or(self.max_open_loans_per_user),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialManifoldConfig {
    #[serde(default)]
    api_base_url: Option<String>,
    #[serde(default)]
    snapshot_ttl_seconds: Option<i64>,
    #[serde(default)]
    poll_interval_seconds: Option<i64>,
}

impl PartialManifoldConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            api_base_url: overlay.api_base_url.or(self.api_base_url),
            snapshot_ttl_seconds: overlay.snapshot_ttl_seconds.or(self.snapshot_ttl_seconds),
            poll_interval_seconds: overlay.poll_interval_seconds.or(self.poll_interval_seconds),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialBotPolicyConfig {
    #[serde(default)]
    auto_claim: Option<bool>,
    #[serde(default)]
    auto_accept_loans: Option<bool>,
    #[serde(default)]
    max_loan_interest_bps: Option<i64>,
    #[serde(default)]
    min_loan_duration_seconds: Option<i64>,
    #[serde(default)]
    auto_buy_bonds: Option<bool>,
    #[serde(default)]
    min_bond_yield_bps: Option<i64>,
    #[serde(default)]
    max_bond_yield_bps: Option<i64>,
    #[serde(default)]
    min_bond_maturity_seconds: Option<i64>,
    #[serde(default)]
    max_bond_maturity_seconds: Option<i64>,
    #[serde(default)]
    max_bond_price_mana: Option<i64>,
    #[serde(default)]
    max_bond_purchase_quantity: Option<i64>,
    #[serde(default)]
    max_total_bond_exposure_mana: Option<i64>,
    #[serde(default)]
    worker_interval_seconds: Option<i64>,
}

impl PartialBotPolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            auto_claim: overlay.auto_claim.or(self.auto_claim),
            auto_accept_loans: overlay.auto_accept_loans.or(self.auto_accept_loans),
            max_loan_interest_bps: overlay
                .max_loan_interest_bps
                .or(self.max_loan_interest_bps),
            min_loan_duration_seconds: overlay
                .min_loan_duration_seconds
                .or(self.min_loan_duration_seconds),
            auto_buy_bonds: overlay.auto_buy_bonds.or(self.auto_buy_bonds),
            min_bond_yield_bps: overlay.min_bond_yield_bps.or(self.min_bond_yield_bps),
            max_bond_yield_bps: overlay.max_bond_yield_bps.or(self.max_bond_yield_bps),
            min_bond_maturity_seconds: overlay
                .min_bond_maturity_seconds
                .or(self.min_bond_maturity_seconds),
            max_bond_maturity_seconds: overlay
                .max_bond_maturity_seconds
                .or(self.max_bond_maturity_seconds),
            max_bond_price_mana: overlay.max_bond_price_mana.or(self.max_bond_price_mana),
            max_bond_purchase_quantity: overlay
                .max_bond_purchase_quantity
                .or(self.max_bond_purchase_quantity),
            max_total_bond_exposure_mana: overlay
                .max_total_bond_exposure_mana
                .or(self.max_total_bond_exposure_mana),
            worker_interval_seconds: overlay
                .worker_interval_seconds
                .or(self.worker_interval_seconds),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialBondPolicyConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    default_yield_period_seconds: Option<i64>,
    #[serde(default)]
    max_yield_bps: Option<i64>,
    #[serde(default)]
    min_maturity_seconds: Option<i64>,
    #[serde(default)]
    max_maturity_seconds: Option<i64>,
    #[serde(default)]
    max_open_issuances_per_user: Option<i64>,
    #[serde(default)]
    worker_interval_seconds: Option<i64>,
}

impl PartialBondPolicyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            enabled: overlay.enabled.or(self.enabled),
            default_yield_period_seconds: overlay
                .default_yield_period_seconds
                .or(self.default_yield_period_seconds),
            max_yield_bps: overlay.max_yield_bps.or(self.max_yield_bps),
            min_maturity_seconds: overlay.min_maturity_seconds.or(self.min_maturity_seconds),
            max_maturity_seconds: overlay.max_maturity_seconds.or(self.max_maturity_seconds),
            max_open_issuances_per_user: overlay
                .max_open_issuances_per_user
                .or(self.max_open_issuances_per_user),
            worker_interval_seconds: overlay
                .worker_interval_seconds
                .or(self.worker_interval_seconds),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialCurrencyConfig {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    singular: Option<String>,
    #[serde(default)]
    plural: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    textual_symbol: Option<String>,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    custom_emoji: Option<String>,
    #[serde(default)]
    image_symbol_path: Option<String>,
    #[serde(default)]
    image_symbol_url: Option<String>,
    #[serde(default)]
    position: Option<String>,
    #[serde(default)]
    space_between: Option<bool>,
    #[serde(default)]
    show_symbol: Option<bool>,
    #[serde(default)]
    show_textual_symbol: Option<bool>,
    #[serde(default)]
    show_code: Option<bool>,
    #[serde(default)]
    use_emoji_in_embeds: Option<bool>,
    #[serde(default)]
    use_emoji_in_plaintext: Option<bool>,
    #[serde(default)]
    decimals: Option<usize>,
    #[serde(default)]
    thousands_separator: Option<String>,
    #[serde(default)]
    negative_style: Option<String>,
    #[serde(default)]
    short_suffixes: Option<bool>,
}

impl PartialCurrencyConfig {
    fn merge(self, overlay: Self) -> Self {
        Self {
            code: overlay.code.or(self.code),
            display_name: overlay.display_name.or(self.display_name),
            singular: overlay.singular.or(self.singular),
            plural: overlay.plural.or(self.plural),
            symbol: overlay.symbol.or(self.symbol),
            textual_symbol: overlay.textual_symbol.or(self.textual_symbol),
            emoji: overlay.emoji.or(self.emoji),
            custom_emoji: overlay.custom_emoji.or(self.custom_emoji),
            image_symbol_path: overlay.image_symbol_path.or(self.image_symbol_path),
            image_symbol_url: overlay.image_symbol_url.or(self.image_symbol_url),
            position: overlay.position.or(self.position),
            space_between: overlay.space_between.or(self.space_between),
            show_symbol: overlay.show_symbol.or(self.show_symbol),
            show_textual_symbol: overlay.show_textual_symbol.or(self.show_textual_symbol),
            show_code: overlay.show_code.or(self.show_code),
            use_emoji_in_embeds: overlay.use_emoji_in_embeds.or(self.use_emoji_in_embeds),
            use_emoji_in_plaintext: overlay
                .use_emoji_in_plaintext
                .or(self.use_emoji_in_plaintext),
            decimals: overlay.decimals.or(self.decimals),
            thousands_separator: overlay.thousands_separator.or(self.thousands_separator),
            negative_style: overlay.negative_style.or(self.negative_style),
            short_suffixes: overlay.short_suffixes.or(self.short_suffixes),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };

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
                max_open_offers_per_user: 25,
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
                max_loan_interest_bps: 500,
                min_loan_duration_seconds: 3_600,
                auto_buy_bonds: true,
                min_bond_yield_bps: 100,
                max_bond_yield_bps: 500,
                min_bond_maturity_seconds: 3_600,
                max_bond_maturity_seconds: 86_400,
                max_bond_price_mana: 5_000,
                max_bond_purchase_quantity: 1,
                max_total_bond_exposure_mana: 20_000,
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
                symbol: "".to_string(),
                textual_symbol: "mana".to_string(),
                emoji: "💰".to_string(),
                custom_emoji: "".to_string(),
                image_symbol_path: "".to_string(),
                image_symbol_url: "".to_string(),
                position: CurrencyPosition::Suffix,
                space_between: true,
                show_symbol: false,
                show_textual_symbol: true,
                show_code: false,
                use_emoji_in_embeds: true,
                use_emoji_in_plaintext: false,
                decimals: 0,
                thousands_separator: ",".to_string(),
                negative_style: NegativeStyle::Minus,
                short_suffixes: true,
            },
            starting_balance: 1_000,
            claim_amount: 10_000,
            claim_period_seconds: 43_200,
            claim_period_name: "twice-daily login".to_string(),
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
