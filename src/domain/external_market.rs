use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalOutcome {
    pub id: Option<String>,
    pub label: String,
    pub probability: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExternalMarketStatus {
    Open,
    Closed,
    Resolved,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExternalResolution {
    BinaryYes,
    BinaryNo,
    MultipleChoice { winning_outcome_id: String },
    Cancelled,
    Ambiguous(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalMarketSnapshot {
    pub external_id: String,
    pub slug: Option<String>,
    pub question: String,
    pub url: String,
    pub status: ExternalMarketStatus,
    pub resolution: Option<ExternalResolution>,
    pub outcomes: Vec<ExternalOutcome>,
    pub raw_json: serde_json::Value,
}
