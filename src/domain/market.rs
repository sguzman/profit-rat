use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MarketType {
    Native,
    Manifold,
}

impl MarketType {
    pub fn from_str(value: &str) -> Self {
        match value {
            "manifold" => Self::Manifold,
            _ => Self::Native,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
    Settled,
    Cancelled,
    NeedsManualReview,
}

impl MarketStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Resolved => "resolved",
            Self::Settled => "settled",
            Self::Cancelled => "cancelled",
            Self::NeedsManualReview => "needs_manual_review",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "closed" => Self::Closed,
            "resolved" => Self::Resolved,
            "settled" => Self::Settled,
            "cancelled" => Self::Cancelled,
            "needs_manual_review" => Self::NeedsManualReview,
            _ => Self::Open,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct MarketRecord {
    pub id: i64,
    pub guild_id: String,
    pub channel_id: String,
    pub creator_discord_user_id: String,
    pub question: String,
    pub status: String,
    pub market_type: String,
    pub liquidity_b: f64,
    pub close_time: Option<String>,
    pub resolved_option_id: Option<i64>,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub updated_at: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub external_slug: Option<String>,
    pub last_external_sync_at: Option<String>,
    pub external_status: Option<String>,
    pub external_resolution: Option<String>,
}

#[allow(dead_code)]
impl MarketRecord {
    pub fn market_type(&self) -> MarketType {
        MarketType::from_str(&self.market_type)
    }

    pub fn status(&self) -> MarketStatus {
        MarketStatus::from_str(&self.status)
    }

    pub fn close_time(&self) -> Option<DateTime<Utc>> {
        self.close_time
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct MarketOptionRecord {
    pub id: i64,
    pub market_id: i64,
    pub label: String,
    pub shares_outstanding: f64,
    pub sort_order: i64,
    pub external_option_id: Option<String>,
    pub external_probability: Option<f64>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct PositionRecord {
    pub id: i64,
    pub market_id: i64,
    pub option_id: i64,
    pub discord_user_id: String,
    pub shares: f64,
    pub total_spent_mana: i64,
    pub total_received_mana: i64,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct GuildAccountRecord {
    pub id: i64,
    pub guild_id: String,
    pub discord_user_id: String,
    pub display_name: Option<String>,
    pub balance_mana: i64,
    pub total_claimed_mana: i64,
    pub last_claim_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl GuildAccountRecord {
    pub fn last_claim_at(&self) -> Option<DateTime<Utc>> {
        self.last_claim_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
    }
}

#[derive(Clone, Debug)]
pub struct MarketDetail {
    pub market: MarketRecord,
    pub options: Vec<MarketOptionRecord>,
}
