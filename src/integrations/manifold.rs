use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, instrument, warn};
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
        let trimmed = url_or_id.trim();

        if let Some(slug) = extract_slug_candidate(trimmed) {
            debug!(input = %trimmed, slug = %slug, "fetching manifold market by slug");
            return self.fetch_market_by_slug(&slug).await;
        }

        debug!(input = %trimmed, "fetching manifold market by id");
        self.fetch_market_by_id(trimmed).await
    }

    #[instrument(skip(self))]
    pub async fn fetch_market_by_id(&self, market_id: &str) -> AppResult<ExternalMarketSnapshot> {
        let url = format!(
            "{}/market/{}",
            self.base_url.trim_end_matches('/'),
            market_id
        );
        let raw = self.fetch_market_payload(&url).await?;
        self.normalize(raw)
    }

    #[instrument(skip(self))]
    pub async fn fetch_market_by_slug(&self, slug: &str) -> AppResult<ExternalMarketSnapshot> {
        let url = format!("{}/slug/{}", self.base_url.trim_end_matches('/'), slug);
        let raw = self.fetch_market_payload(&url).await?;
        self.normalize(raw)
    }

    #[instrument(skip(self))]
    async fn fetch_market_payload(&self, url: &str) -> AppResult<ManifoldMarket> {
        let response = self.http.get(url).send().await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            warn!(
                %url,
                %status,
                body = %truncate_for_log(&body),
                "manifold request returned non-success status"
            );
            return Err(AppError::External(format!(
                "manifold request failed with status {status}"
            )));
        }

        serde_json::from_str::<ManifoldMarket>(&body).map_err(|error| {
            warn!(
                %url,
                %error,
                body = %truncate_for_log(&body),
                "failed to decode manifold market payload"
            );
            AppError::External("manifold returned an unexpected response body".to_string())
        })
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

fn extract_slug_candidate(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let parsed = Url::parse(trimmed).ok()?;
        let segments = parsed
            .path_segments()?
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        return segments.last().map(|segment| (*segment).to_string());
    }

    let without_host = trimmed
        .strip_prefix("manifold.markets/")
        .or_else(|| trimmed.strip_prefix("www.manifold.markets/"))
        .or_else(|| trimmed.strip_prefix("api.manifold.markets/v0/slug/"))
        .unwrap_or(trimmed);

    if !without_host.contains('/') {
        return None;
    }

    without_host
        .split('/')
        .filter(|segment| !segment.is_empty())
        .next_back()
        .map(ToOwned::to_owned)
}

fn truncate_for_log(body: &str) -> String {
    const LIMIT: usize = 400;
    if body.len() <= LIMIT {
        body.to_string()
    } else {
        format!("{}...", &body[..LIMIT])
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

#[cfg(test)]
mod tests {
    use super::extract_slug_candidate;

    #[test]
    fn extracts_slug_from_full_market_url() {
        assert_eq!(
            extract_slug_candidate(
                "https://manifold.markets/ManifoldSports/arg-vs-aut-world-cup-26"
            ),
            Some("arg-vs-aut-world-cup-26".to_string())
        );
    }

    #[test]
    fn extracts_slug_from_hostless_path() {
        assert_eq!(
            extract_slug_candidate("manifold.markets/ManifoldSports/arg-vs-aut-world-cup-26"),
            Some("arg-vs-aut-world-cup-26".to_string())
        );
    }

    #[test]
    fn does_not_treat_bare_id_as_slug() {
        assert_eq!(extract_slug_candidate("QOUnZl2nNU"), None);
    }
}
