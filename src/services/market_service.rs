use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;
use sqlx::Row;
use tracing::{debug, instrument, warn};

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::domain::external_market::{ExternalMarketSnapshot, ExternalResolution};
use crate::domain::market::{
    MarketDetail, MarketOptionRecord, MarketRecord, MarketStatus, MarketType, PositionRecord,
};
use crate::error::{AppError, AppResult};
use crate::integrations::manifold::ManifoldClient;

#[derive(Clone)]
pub struct MarketService {
    config: Arc<AppConfig>,
    pool: DbPool,
    manifold: Arc<ManifoldClient>,
}

#[derive(Clone, Debug)]
pub struct CreateMarketRequest {
    pub guild_id: String,
    pub channel_id: String,
    pub creator_user_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub liquidity_b: Option<f64>,
    pub close_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct MarketView {
    pub detail: MarketDetail,
    pub probabilities: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct ListMarketsItem {
    pub id: i64,
    pub question: String,
    pub status: String,
    pub market_type: String,
}

#[derive(Clone, Debug)]
pub struct MarketResolutionAnnouncement {
    pub channel_id: u64,
    pub market_id: i64,
    pub question: String,
    pub status: MarketStatus,
    pub market_type: MarketType,
    pub winning_option: Option<String>,
    pub total_payout: i64,
    pub external_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MarketHolderLine {
    pub display_name: String,
    pub option_label: String,
    pub shares: f64,
    pub total_spent_mana: i64,
    pub total_received_mana: i64,
    pub current_value_mana: i64,
    pub unrealized_pnl_mana: i64,
}

impl MarketService {
    pub fn new(config: Arc<AppConfig>, pool: DbPool, manifold: Arc<ManifoldClient>) -> Self {
        Self {
            config,
            pool,
            manifold,
        }
    }

    #[instrument(skip(self))]
    pub async fn create_native_market(
        &self,
        request: CreateMarketRequest,
    ) -> AppResult<MarketView> {
        validate_market_request(&request)?;
        let now = now_rfc3339();
        let liquidity_b = request
            .liquidity_b
            .unwrap_or(self.config.default_liquidity_b);
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "INSERT INTO markets
             (guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'native', ?6, ?7, NULL, ?8, NULL, ?8)",
        )
        .bind(&request.guild_id)
        .bind(&request.channel_id)
        .bind(&request.creator_user_id)
        .bind(&request.question)
        .bind(MarketStatus::Open.as_str())
        .bind(liquidity_b)
        .bind(request.close_time.map(|value| value.to_rfc3339()))
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let market_id = result.last_insert_rowid();

        for (index, option) in request.options.iter().enumerate() {
            sqlx::query(
                "INSERT INTO market_options (market_id, label, shares_outstanding, sort_order, external_option_id, external_probability)
                 VALUES (?1, ?2, 0.0, ?3, NULL, NULL)",
            )
            .bind(market_id)
            .bind(option)
            .bind(i64::try_from(index)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        self.market_view(market_id).await
    }

    #[instrument(skip(self), fields(guild_id))]
    pub async fn list_markets(
        &self,
        guild_id: &str,
        status: Option<String>,
    ) -> AppResult<Vec<ListMarketsItem>> {
        let status = status.unwrap_or_else(|| "open".to_string());
        let items = if status == "all" {
            sqlx::query_as::<_, MarketRecord>(
                "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
                 FROM markets
                 WHERE guild_id = ?1
                 ORDER BY id DESC
                 LIMIT 25",
            )
            .bind(guild_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, MarketRecord>(
                "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
                 FROM markets
                 WHERE guild_id = ?1 AND status = ?2
                 ORDER BY id DESC
                 LIMIT 25",
            )
            .bind(guild_id)
            .bind(status)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(items
            .into_iter()
            .map(|market| ListMarketsItem {
                id: market.id,
                question: market.question,
                status: market.status,
                market_type: market.market_type,
            })
            .collect())
    }

    #[instrument(skip(self))]
    pub async fn autocomplete_markets(
        &self,
        guild_id: &str,
        partial: &str,
        status_filter: Option<&str>,
        market_type_filter: Option<&str>,
        limit: i64,
    ) -> AppResult<Vec<serenity::AutocompleteChoice>> {
        let partial = partial.trim();
        let like = format!("%{partial}%");
        let markets = sqlx::query_as::<_, MarketRecord>(
            "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
             FROM markets
             WHERE guild_id = ?1
               AND (?2 IS NULL OR status = ?2)
               AND (?3 IS NULL OR market_type = ?3)
               AND (?4 = '' OR question LIKE ?5 OR CAST(id AS TEXT) LIKE ?5)
             ORDER BY CASE WHEN status = 'open' THEN 0 ELSE 1 END, id DESC
             LIMIT ?6",
        )
        .bind(guild_id)
        .bind(status_filter)
        .bind(market_type_filter)
        .bind(partial)
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(markets
            .into_iter()
            .map(|market| {
                serenity::AutocompleteChoice::new(
                    format!(
                        "#{} [{}|{}] {}",
                        market.id, market.market_type, market.status, market.question
                    ),
                    market.id.to_string(),
                )
            })
            .collect())
    }

    #[instrument(skip(self))]
    pub async fn autocomplete_market_options(
        &self,
        market_id: i64,
        partial: &str,
        limit: i64,
    ) -> AppResult<Vec<serenity::AutocompleteChoice>> {
        let partial = partial.trim();
        let like = format!("%{partial}%");
        let options = sqlx::query_as::<_, MarketOptionRecord>(
            "SELECT id, market_id, label, shares_outstanding, sort_order, external_option_id, external_probability
             FROM market_options
             WHERE market_id = ?1
               AND (?2 = '' OR label LIKE ?3)
             ORDER BY sort_order ASC
             LIMIT ?4",
        )
        .bind(market_id)
        .bind(partial)
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(options
            .into_iter()
            .map(|option| {
                let display = match option.external_probability {
                    Some(probability) => {
                        format!("{} ({:.1}%)", option.label, probability * 100.0)
                    }
                    None => option.label.clone(),
                };
                serenity::AutocompleteChoice::new(display, option.label)
            })
            .collect())
    }

    #[instrument(skip(self))]
    pub async fn market_view(&self, market_id: i64) -> AppResult<MarketView> {
        let detail = self.market_detail(market_id).await?;
        let probabilities = match detail.market.market_type() {
            MarketType::Native => {
                let shares = detail
                    .options
                    .iter()
                    .map(|option| option.shares_outstanding)
                    .collect::<Vec<_>>();
                crate::domain::pricing::lmsr_probabilities(&shares, detail.market.liquidity_b)?
            }
            MarketType::Manifold => detail
                .options
                .iter()
                .map(|option| option.external_probability.unwrap_or(0.0))
                .collect(),
        };

        Ok(MarketView {
            detail,
            probabilities,
        })
    }

    #[instrument(skip(self))]
    pub async fn market_view_for_guild(&self, guild_id: &str, market_id: i64) -> AppResult<MarketView> {
        let view = self.market_view(market_id).await?;
        self.ensure_market_belongs_to_guild(guild_id, &view.detail.market)?;
        Ok(view)
    }

    #[instrument(skip(self))]
    pub async fn market_detail(&self, market_id: i64) -> AppResult<MarketDetail> {
        let market = sqlx::query_as::<_, MarketRecord>(
            "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
             FROM markets WHERE id = ?1",
        )
        .bind(market_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("market {market_id} was not found")))?;

        let options = sqlx::query_as::<_, MarketOptionRecord>(
            "SELECT id, market_id, label, shares_outstanding, sort_order, external_option_id, external_probability
             FROM market_options WHERE market_id = ?1 ORDER BY sort_order ASC",
        )
        .bind(market_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(MarketDetail { market, options })
    }

    #[instrument(skip(self))]
    pub async fn track_manifold_market(
        &self,
        guild_id: &str,
        channel_id: &str,
        creator_user_id: &str,
        url_or_id: &str,
    ) -> AppResult<MarketView> {
        let snapshot = self.manifold.fetch_market(url_or_id).await?;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "INSERT INTO markets
             (guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution)
             VALUES (?1, ?2, ?3, ?4, ?5, 'manifold', ?6, NULL, NULL, ?7, NULL, ?7, 'manifold', ?8, ?9, ?10, ?7, ?11, ?12)",
        )
        .bind(guild_id)
        .bind(channel_id)
        .bind(creator_user_id)
        .bind(&snapshot.question)
        .bind(MarketStatus::Open.as_str())
        .bind(self.config.default_liquidity_b)
        .bind(&now)
        .bind(&snapshot.external_id)
        .bind(&snapshot.url)
        .bind(snapshot.slug.clone())
        .bind(format!("{:?}", snapshot.status))
        .bind(snapshot.resolution.as_ref().map(|value| format!("{value:?}")))
        .execute(&mut *tx)
        .await?;
        let market_id = result.last_insert_rowid();

        self.insert_snapshot(&mut tx, market_id, &snapshot).await?;
        self.replace_external_options(&mut tx, market_id, &snapshot)
            .await?;
        tx.commit().await?;
        self.market_view(market_id).await
    }

    #[instrument(skip(self))]
    pub async fn sync_manifold_market(&self, market_id: i64) -> AppResult<MarketView> {
        let detail = self.market_detail(market_id).await?;
        if detail.market.market_type() != MarketType::Manifold {
            return Err(AppError::Validation(
                "market is not a manifold-tracked market".to_string(),
            ));
        }
        let external_id = detail.market.external_id.clone().ok_or_else(|| {
            AppError::External("tracked market is missing external id".to_string())
        })?;
        let snapshot = self.manifold.fetch_market(&external_id).await?;
        let mut tx = self.pool.begin().await?;
        self.insert_snapshot(&mut tx, market_id, &snapshot).await?;
        self.replace_external_options(&mut tx, market_id, &snapshot)
            .await?;

        sqlx::query(
            "UPDATE markets
             SET question = ?2,
                 external_url = ?3,
                 external_slug = ?4,
                 last_external_sync_at = ?5,
                 external_status = ?6,
                 external_resolution = ?7,
                 updated_at = ?5
             WHERE id = ?1",
        )
        .bind(market_id)
        .bind(&snapshot.question)
        .bind(&snapshot.url)
        .bind(snapshot.slug.clone())
        .bind(now_rfc3339())
        .bind(format!("{:?}", snapshot.status))
        .bind(
            snapshot
                .resolution
                .as_ref()
                .map(|value| format!("{value:?}")),
        )
        .execute(&mut *tx)
        .await?;

        let settlement_result = self
            .settle_external_if_possible(
                &mut tx,
                &detail.market.guild_id,
                market_id,
                &detail.options,
                &snapshot,
            )
            .await?;
        if settlement_result == MarketStatus::NeedsManualReview {
            warn!(
                market_id,
                "external market needs manual review before settlement"
            );
        }

        tx.commit().await?;
        self.market_view(market_id).await
    }

    #[instrument(skip(self))]
    pub async fn poll_manifold_resolutions(&self) -> AppResult<Vec<MarketResolutionAnnouncement>> {
        let open_markets = sqlx::query_as::<_, MarketRecord>(
            "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
             FROM markets
             WHERE market_type = 'manifold' AND status = 'open'
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut announcements = Vec::new();
        for market in open_markets {
            match self.sync_manifold_market(market.id).await {
                Ok(view) => {
                    let status = view.detail.market.status();
                    if status == MarketStatus::Open {
                        continue;
                    }

                    let winning_option =
                        view.detail.market.resolved_option_id.and_then(|winner_id| {
                            view.detail
                                .options
                                .iter()
                                .find(|option| option.id == winner_id)
                                .map(|option| option.label.clone())
                        });
                    let total_payout = if status == MarketStatus::Settled {
                        self.total_external_payout(view.detail.market.id).await?
                    } else {
                        0
                    };
                    let channel_id =
                        view.detail.market.channel_id.parse::<u64>().map_err(|_| {
                            AppError::External(format!(
                                "invalid channel id stored for market {}",
                                view.detail.market.id
                            ))
                        })?;

                    announcements.push(MarketResolutionAnnouncement {
                        channel_id,
                        market_id: view.detail.market.id,
                        question: view.detail.market.question.clone(),
                        status,
                        market_type: view.detail.market.market_type(),
                        winning_option,
                        total_payout,
                        external_url: view.detail.market.external_url.clone(),
                    });
                }
                Err(error) => {
                    warn!(market_id = market.id, %error, "failed to poll tracked manifold market");
                }
            }
        }

        Ok(announcements)
    }

    #[instrument(skip(self), fields(guild_id, market_id))]
    pub async fn resolve_native_market(
        &self,
        guild_id: &str,
        market_id: i64,
        winning_label: &str,
    ) -> AppResult<i64> {
        let detail = self.market_detail(market_id).await?;
        self.ensure_market_belongs_to_guild(guild_id, &detail.market)?;
        if detail.market.market_type() != MarketType::Native {
            return Err(AppError::Validation(
                "use `/msync` for manifold-tracked markets".to_string(),
            ));
        }
        if matches!(
            detail.market.status(),
            MarketStatus::Resolved | MarketStatus::Settled
        ) {
            return Err(AppError::Conflict("market is already resolved".to_string()));
        }

        let winner = detail
            .options
            .iter()
            .find(|option| option.label.eq_ignore_ascii_case(winning_label))
            .ok_or_else(|| AppError::NotFound(format!("option `{winning_label}` was not found")))?;
        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE markets
             SET status = 'resolved', resolved_option_id = ?2, resolved_at = ?3, updated_at = ?3
             WHERE id = ?1",
        )
        .bind(market_id)
        .bind(winner.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        let positions = sqlx::query_as::<_, PositionRecord>(
            "SELECT id, market_id, option_id, discord_user_id, shares, total_spent_mana, total_received_mana, updated_at
             FROM positions WHERE market_id = ?1",
        )
        .bind(market_id)
        .fetch_all(&mut *tx)
        .await?;

        let total_payout = self
            .settle_positions(
                &mut tx,
                &detail.market.guild_id,
                market_id,
                winner.id,
                positions,
                "resolution_payout",
            )
            .await?;

        sqlx::query("UPDATE markets SET status = 'settled', updated_at = ?2 WHERE id = ?1")
            .bind(market_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(total_payout)
    }

    pub async fn positions_for_user(
        &self,
        guild_id: &str,
        discord_user_id: &str,
        market_id: Option<i64>,
    ) -> AppResult<Vec<(PositionRecord, MarketRecord, MarketOptionRecord)>> {
        let rows = if let Some(market_id) = market_id {
            sqlx::query(
                "SELECT
                    p.id as p_id, p.market_id as p_market_id, p.option_id as p_option_id, p.discord_user_id as p_discord_user_id,
                    p.shares as p_shares, p.total_spent_mana as p_total_spent_mana, p.total_received_mana as p_total_received_mana, p.updated_at as p_updated_at,
                    m.id as m_id, m.guild_id, m.channel_id, m.creator_discord_user_id, m.question, m.status, m.market_type, m.liquidity_b,
                    m.close_time, m.resolved_option_id, m.created_at, m.resolved_at, m.updated_at, m.external_source, m.external_id,
                    m.external_url, m.external_slug, m.last_external_sync_at, m.external_status, m.external_resolution,
                    o.id as o_id, o.market_id as o_market_id, o.label, o.shares_outstanding, o.sort_order, o.external_option_id, o.external_probability
                 FROM positions p
                 JOIN markets m ON m.id = p.market_id
                 JOIN market_options o ON o.id = p.option_id
                 WHERE m.guild_id = ?1 AND p.discord_user_id = ?2 AND p.market_id = ?3
                 ORDER BY p.market_id DESC, o.sort_order ASC",
            )
                .bind(guild_id)
                .bind(discord_user_id)
                .bind(market_id)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query(
                "SELECT
                    p.id as p_id, p.market_id as p_market_id, p.option_id as p_option_id, p.discord_user_id as p_discord_user_id,
                    p.shares as p_shares, p.total_spent_mana as p_total_spent_mana, p.total_received_mana as p_total_received_mana, p.updated_at as p_updated_at,
                    m.id as m_id, m.guild_id, m.channel_id, m.creator_discord_user_id, m.question, m.status, m.market_type, m.liquidity_b,
                    m.close_time, m.resolved_option_id, m.created_at, m.resolved_at, m.updated_at, m.external_source, m.external_id,
                    m.external_url, m.external_slug, m.last_external_sync_at, m.external_status, m.external_resolution,
                    o.id as o_id, o.market_id as o_market_id, o.label, o.shares_outstanding, o.sort_order, o.external_option_id, o.external_probability
                 FROM positions p
                 JOIN markets m ON m.id = p.market_id
                 JOIN market_options o ON o.id = p.option_id
                 WHERE m.guild_id = ?1 AND p.discord_user_id = ?2
                 ORDER BY p.market_id DESC, o.sort_order ASC",
            )
                .bind(guild_id)
                .bind(discord_user_id)
                .fetch_all(&self.pool)
                .await?
        };

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let position = PositionRecord {
                id: row.get("p_id"),
                market_id: row.get("p_market_id"),
                option_id: row.get("p_option_id"),
                discord_user_id: row.get("p_discord_user_id"),
                shares: row.get("p_shares"),
                total_spent_mana: row.get("p_total_spent_mana"),
                total_received_mana: row.get("p_total_received_mana"),
                updated_at: row.get("p_updated_at"),
            };
            let market = MarketRecord {
                id: row.get("m_id"),
                guild_id: row.get("guild_id"),
                channel_id: row.get("channel_id"),
                creator_discord_user_id: row.get("creator_discord_user_id"),
                question: row.get("question"),
                status: row.get("status"),
                market_type: row.get("market_type"),
                liquidity_b: row.get("liquidity_b"),
                close_time: row.get("close_time"),
                resolved_option_id: row.get("resolved_option_id"),
                created_at: row.get("created_at"),
                resolved_at: row.get("resolved_at"),
                updated_at: row.get("updated_at"),
                external_source: row.get("external_source"),
                external_id: row.get("external_id"),
                external_url: row.get("external_url"),
                external_slug: row.get("external_slug"),
                last_external_sync_at: row.get("last_external_sync_at"),
                external_status: row.get("external_status"),
                external_resolution: row.get("external_resolution"),
            };
            let option = MarketOptionRecord {
                id: row.get("o_id"),
                market_id: row.get("o_market_id"),
                label: row.get("label"),
                shares_outstanding: row.get("shares_outstanding"),
                sort_order: row.get("sort_order"),
                external_option_id: row.get("external_option_id"),
                external_probability: row.get("external_probability"),
            };
            out.push((position, market, option));
        }
        Ok(out)
    }

    #[instrument(skip(self), fields(guild_id, market_id))]
    pub async fn market_holders(
        &self,
        guild_id: &str,
        market_id: i64,
    ) -> AppResult<(MarketView, Vec<MarketHolderLine>)> {
        let view = self.market_view_for_guild(guild_id, market_id).await?;
        let price_by_option_id = view
            .detail
            .options
            .iter()
            .zip(view.probabilities.iter())
            .map(|(option, probability)| (option.id, *probability))
            .collect::<HashMap<_, _>>();

        let rows = sqlx::query(
            "SELECT
                p.shares,
                p.total_spent_mana,
                p.total_received_mana,
                o.label,
                u.display_name,
                p.discord_user_id,
                p.option_id
             FROM positions p
             JOIN markets m ON m.id = p.market_id
             JOIN market_options o ON o.id = p.option_id
             LEFT JOIN guild_accounts u
               ON u.guild_id = m.guild_id
              AND u.discord_user_id = p.discord_user_id
             WHERE m.guild_id = ?1 AND p.market_id = ?2 AND p.shares > 0.0000001
             ORDER BY o.sort_order ASC, COALESCE(u.display_name, p.discord_user_id) ASC",
        )
        .bind(guild_id)
        .bind(market_id)
        .fetch_all(&self.pool)
        .await?;

        let holders = rows
            .into_iter()
            .map(|row| {
                let option_id = row.get::<i64, _>("option_id");
                let shares = row.get::<f64, _>("shares");
                let total_spent_mana = row.get::<i64, _>("total_spent_mana");
                let total_received_mana = row.get::<i64, _>("total_received_mana");
                let current_price = price_by_option_id.get(&option_id).copied().unwrap_or(0.0);
                let current_value_mana = (shares * current_price).round() as i64;
                let unrealized_pnl_mana =
                    current_value_mana + total_received_mana - total_spent_mana;

                MarketHolderLine {
                    display_name: row
                        .get::<Option<String>, _>("display_name")
                        .unwrap_or_else(|| row.get::<String, _>("discord_user_id")),
                    option_label: row.get("label"),
                    shares,
                    total_spent_mana,
                    total_received_mana,
                    current_value_mana,
                    unrealized_pnl_mana,
                }
            })
            .collect::<Vec<_>>();

        Ok((view, holders))
    }

    async fn insert_snapshot(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        market_id: i64,
        snapshot: &ExternalMarketSnapshot,
    ) -> AppResult<i64> {
        let primary_probability = snapshot.outcomes.first().map(|outcome| outcome.probability);
        let raw_status = format!("{:?}", snapshot.status);
        let raw_resolution = snapshot
            .resolution
            .as_ref()
            .map(|value| format!("{value:?}"));
        let raw_json = serde_json::to_string(&snapshot.raw_json)?;
        let result = sqlx::query(
            "INSERT INTO external_market_snapshots
             (market_id, external_source, external_id, probability, raw_status, raw_resolution, raw_json, fetched_at)
             VALUES (?1, 'manifold', ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(market_id)
        .bind(&snapshot.external_id)
        .bind(primary_probability)
        .bind(raw_status)
        .bind(raw_resolution)
        .bind(raw_json)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(result.last_insert_rowid())
    }

    async fn replace_external_options(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        market_id: i64,
        snapshot: &ExternalMarketSnapshot,
    ) -> AppResult<()> {
        for (index, outcome) in snapshot.outcomes.iter().enumerate() {
            sqlx::query(
                "INSERT INTO market_options (market_id, label, shares_outstanding, sort_order, external_option_id, external_probability)
                 VALUES (?1, ?2, 0.0, ?3, ?4, ?5)
                 ON CONFLICT(market_id, label) DO UPDATE SET
                    sort_order = excluded.sort_order,
                    external_option_id = excluded.external_option_id,
                    external_probability = excluded.external_probability",
            )
            .bind(market_id)
            .bind(&outcome.label)
            .bind(i64::try_from(index)?)
            .bind(outcome.id.clone())
            .bind(outcome.probability)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    async fn settle_external_if_possible(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        market_id: i64,
        existing_options: &[MarketOptionRecord],
        snapshot: &ExternalMarketSnapshot,
    ) -> AppResult<MarketStatus> {
        let winner_option_id = match snapshot.resolution.as_ref() {
            Some(ExternalResolution::BinaryYes) => existing_options
                .iter()
                .find(|option| option.label.eq_ignore_ascii_case("YES"))
                .map(|option| option.id),
            Some(ExternalResolution::BinaryNo) => existing_options
                .iter()
                .find(|option| option.label.eq_ignore_ascii_case("NO"))
                .map(|option| option.id),
            Some(ExternalResolution::MultipleChoice { winning_outcome_id }) => existing_options
                .iter()
                .find(|option| {
                    option.external_option_id.as_deref() == Some(winning_outcome_id.as_str())
                })
                .map(|option| option.id),
            Some(ExternalResolution::Cancelled) => None,
            Some(ExternalResolution::Ambiguous(reason)) => {
                debug!(market_id, %reason, "external resolution was ambiguous");
                sqlx::query("UPDATE markets SET status = 'needs_manual_review', updated_at = ?2 WHERE id = ?1")
                    .bind(market_id)
                    .bind(now_rfc3339())
                    .execute(&mut **tx)
                    .await?;
                return Ok(MarketStatus::NeedsManualReview);
            }
            None => return Ok(MarketStatus::Open),
        };

        if let Some(winner_option_id) = winner_option_id {
            let positions = sqlx::query_as::<_, PositionRecord>(
                "SELECT id, market_id, option_id, discord_user_id, shares, total_spent_mana, total_received_mana, updated_at
                 FROM positions WHERE market_id = ?1",
            )
            .bind(market_id)
            .fetch_all(&mut **tx)
            .await?;

            sqlx::query(
                "UPDATE markets
                 SET status = 'resolved', resolved_option_id = ?2, resolved_at = ?3, updated_at = ?3
                 WHERE id = ?1",
            )
            .bind(market_id)
            .bind(winner_option_id)
            .bind(now_rfc3339())
            .execute(&mut **tx)
            .await?;

            self.settle_positions(
                tx,
                guild_id,
                market_id,
                winner_option_id,
                positions,
                "external_resolution_payout",
            )
            .await?;

            sqlx::query("UPDATE markets SET status = 'settled', updated_at = ?2 WHERE id = ?1")
                .bind(market_id)
                .bind(now_rfc3339())
                .execute(&mut **tx)
                .await?;
            Ok(MarketStatus::Settled)
        } else {
            sqlx::query("UPDATE markets SET status = 'cancelled', updated_at = ?2 WHERE id = ?1")
                .bind(market_id)
                .bind(now_rfc3339())
                .execute(&mut **tx)
                .await?;
            Ok(MarketStatus::Cancelled)
        }
    }

    async fn settle_positions(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        market_id: i64,
        winner_option_id: i64,
        positions: Vec<PositionRecord>,
        reason: &str,
    ) -> AppResult<i64> {
        let mut total_payout = 0_i64;
        for position in positions {
            if position.option_id != winner_option_id {
                continue;
            }
            let payout = position.shares.round() as i64;
            total_payout += payout;
            sqlx::query(
                "UPDATE guild_accounts
                 SET balance_mana = balance_mana + ?2, updated_at = ?3
                 WHERE guild_id = ?1 AND discord_user_id = ?4",
            )
            .bind(guild_id)
            .bind(payout)
            .bind(now_rfc3339())
            .bind(&position.discord_user_id)
            .execute(&mut **tx)
            .await?;

            sqlx::query(
                "INSERT INTO economy_events
                 (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'money', ?5, NULL, ?6, 'market settlement payout', ?7)",
            )
            .bind(guild_id)
            .bind(&position.discord_user_id)
            .bind(market_id)
            .bind(winner_option_id)
            .bind(payout)
            .bind(reason)
            .bind(now_rfc3339())
            .execute(&mut **tx)
            .await?;
        }
        Ok(total_payout)
    }

    async fn total_external_payout(&self, market_id: i64) -> AppResult<i64> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(amount_mana), 0) AS total
             FROM economy_events
             WHERE related_market_id = ?1 AND reason = 'external_resolution_payout'",
        )
        .bind(market_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("total"))
    }

    fn ensure_market_belongs_to_guild(
        &self,
        guild_id: &str,
        market: &MarketRecord,
    ) -> AppResult<()> {
        if market.guild_id != guild_id {
            return Err(AppError::NotFound(format!(
                "market {} was not found in this server",
                market.id
            )));
        }
        Ok(())
    }
}

fn validate_market_request(request: &CreateMarketRequest) -> AppResult<()> {
    if request.question.trim().is_empty() {
        return Err(AppError::Validation(
            "market question cannot be empty".to_string(),
        ));
    }
    if request.question.chars().count() > 200 {
        return Err(AppError::Validation(
            "market question must be 200 characters or fewer".to_string(),
        ));
    }
    if !(2..=10).contains(&request.options.len()) {
        return Err(AppError::Validation(
            "markets must have between 2 and 10 options".to_string(),
        ));
    }
    if request
        .options
        .iter()
        .any(|option| option.trim().is_empty())
    {
        return Err(AppError::Validation(
            "market options cannot be empty".to_string(),
        ));
    }
    Ok(())
}
