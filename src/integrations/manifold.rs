use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, instrument};
use url::Url;

use crate::domain::external_market::{
    ExternalMarketSnapshot, ExternalMarketStatus, ExternalOutcome, ExternalResolution,
};
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct ManifoldClient {
    http: Client,
    base_url: String,
}

impl ManifoldClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: Client::new(),
            base_url,
        }
    }

    #[instrument(skip(self))]
    pub async fn fetch_market(&self, url_or_id: &str) -> AppResult<ExternalMarketSnapshot> {
        if url_or_id.starts_with("http://") || url_or_id.starts_with("https://") {
            let parsed = Url::parse(url_or_id)?;
            if let Some(slug) = parsed.path_segments().and_then(|segments| segments.last()) {
                return self.fetch_market_by_slug(slug).await;
            }
        }

        if url_or_id.contains('/') {
            return self
                .fetch_market_by_slug(url_or_id.rsplit('/').next().unwrap_or(url_or_id))
                .await;
        }

        self.fetch_market_by_id(url_or_id).await
    }

    #[instrument(skip(self))]
    pub async fn fetch_market_by_id(&self, market_id: &str) -> AppResult<ExternalMarketSnapshot> {
        let url = format!(
            "{}/market/{}",
            self.base_url.trim_end_matches('/'),
            market_id
        );
        let response = self.http.get(url).send().await?.error_for_status()?;
        let raw = response.json::<ManifoldMarket>().await?;
        self.normalize(raw)
    }

    #[instrument(skip(self))]
    pub async fn fetch_market_by_slug(&self, slug: &str) -> AppResult<ExternalMarketSnapshot> {
        let url = format!("{}/slug/{}", self.base_url.trim_end_matches('/'), slug);
        let response = self.http.get(url).send().await?.error_for_status()?;
        let raw = response.json::<ManifoldMarket>().await?;
        self.normalize(raw)
    }

    fn normalize(&self, raw: ManifoldMarket) -> AppResult<ExternalMarketSnapshot> {
        debug!(market_id = %raw.id, outcome_type = %raw.outcome_type, "normalizing manifold market");
        let raw_json = serde_json::to_value(&raw)?;
        let resolution = match raw.resolution.as_deref() {
            Some("YES") => Some(ExternalResolution::BinaryYes),
            Some("NO") => Some(ExternalResolution::BinaryNo),
            Some("CANCEL") => Some(ExternalResolution::Cancelled),
            Some(other) => {
                if let Some(answer) = raw.answers.as_ref().and_then(|answers| {
                    answers
                        .iter()
                        .find(|answer| answer.number.to_string() == other || answer.id == other)
                }) {
                    Some(ExternalResolution::MultipleChoice {
                        winning_outcome_id: answer.id.clone(),
                    })
                } else {
                    Some(ExternalResolution::Ambiguous(other.to_string()))
                }
            }
            None => None,
        };

        let status = if raw.is_resolved.unwrap_or(false) || resolution.is_some() {
            ExternalMarketStatus::Resolved
        } else {
            ExternalMarketStatus::Open
        };

        let outcomes = match raw.outcome_type.as_str() {
            "BINARY" => {
                let probability = raw.probability.ok_or_else(|| {
                    AppError::External(
                        "manifold binary market did not include probability".to_string(),
                    )
                })?;
                vec![
                    ExternalOutcome {
                        id: Some("YES".to_string()),
                        label: "YES".to_string(),
                        probability,
                    },
                    ExternalOutcome {
                        id: Some("NO".to_string()),
                        label: "NO".to_string(),
                        probability: 1.0 - probability,
                    },
                ]
            }
            "MULTIPLE_CHOICE" | "DEPENDENT_MULTIPLE_CHOICE" => raw
                .answers
                .unwrap_or_default()
                .into_iter()
                .map(|answer| ExternalOutcome {
                    id: Some(answer.id),
                    label: answer.text,
                    probability: answer.probability.unwrap_or(0.0),
                })
                .collect(),
            other => {
                return Err(AppError::External(format!(
                    "unsupported manifold market type `{other}`"
                )));
            }
        };

        Ok(ExternalMarketSnapshot {
            external_id: raw.id,
            slug: raw.slug,
            question: raw.question,
            url: raw.url,
            status,
            resolution,
            outcomes,
            raw_json,
        })
    }
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifoldMarket {
    id: String,
    question: String,
    url: String,
    slug: Option<String>,
    #[serde(rename = "outcomeType")]
    outcome_type: String,
    probability: Option<f64>,
    answers: Option<Vec<ManifoldAnswer>>,
    #[serde(rename = "isResolved")]
    is_resolved: Option<bool>,
    resolution: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifoldAnswer {
    id: String,
    text: String,
    number: i64,
    probability: Option<f64>,
}
