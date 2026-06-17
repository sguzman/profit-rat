use std::sync::Arc;

use sqlx::Row;
use tracing::{debug, instrument};

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::domain::market::{MarketOptionRecord, MarketStatus, MarketType};
use crate::domain::pricing::{lmsr_probabilities, sale_value_for_shares, shares_for_budget};
use crate::error::{AppError, AppResult};
use crate::integrations::manifold::ManifoldClient;

#[derive(Clone)]
pub struct TradingService {
    config: Arc<AppConfig>,
    pool: DbPool,
    manifold: Arc<ManifoldClient>,
}

#[derive(Clone, Debug)]
pub struct BuyRequest {
    pub user_id: String,
    pub display_name: String,
    pub market_id: i64,
    pub option_label: String,
    pub amount_mana: i64,
}

#[derive(Clone, Debug)]
pub struct SellRequest {
    pub user_id: String,
    pub display_name: String,
    pub market_id: i64,
    pub option_label: String,
    pub shares: f64,
}

#[derive(Clone, Debug)]
pub struct TradeReceipt {
    pub market_id: i64,
    pub market_type: String,
    pub option_label: String,
    pub balance_mana: i64,
    pub mana_amount: i64,
    pub shares_delta: f64,
    pub price_before: f64,
    pub price_after: f64,
}

impl TradingService {
    pub fn new(config: Arc<AppConfig>, pool: DbPool, manifold: Arc<ManifoldClient>) -> Self {
        Self {
            config,
            pool,
            manifold,
        }
    }

    #[instrument(skip(self))]
    pub async fn buy(&self, request: BuyRequest) -> AppResult<TradeReceipt> {
        self.ensure_user(&request.user_id, &request.display_name)
            .await?;
        let detail = self.load_market(request.market_id).await?;
        if detail.market.status() != MarketStatus::Open {
            return Err(AppError::Conflict(
                "market is not open for trading".to_string(),
            ));
        }
        if request.amount_mana <= 0 {
            return Err(AppError::Validation(
                "buy amount must be positive".to_string(),
            ));
        }

        let option_index = find_option_index(&detail.options, &request.option_label)?;
        let option = detail.options[option_index].clone();
        let balance = self.user_balance(&request.user_id).await?;
        if balance < request.amount_mana {
            return Err(AppError::Conflict(
                "insufficient fake mana balance".to_string(),
            ));
        }

        match detail.market.market_type() {
            MarketType::Native => {
                self.buy_native(request, &detail.options, option_index, option)
                    .await
            }
            MarketType::Manifold => {
                self.buy_external(request, &detail.options, option_index, option)
                    .await
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn sell(&self, request: SellRequest) -> AppResult<TradeReceipt> {
        self.ensure_user(&request.user_id, &request.display_name)
            .await?;
        let detail = self.load_market(request.market_id).await?;
        if detail.market.status() != MarketStatus::Open {
            return Err(AppError::Conflict(
                "market is not open for selling".to_string(),
            ));
        }
        let option_index = find_option_index(&detail.options, &request.option_label)?;
        let option = detail.options[option_index].clone();
        let position_shares = self
            .position_shares(request.market_id, option.id, &request.user_id)
            .await?;
        if request.shares <= 0.0 {
            return Err(AppError::Validation(
                "sell shares must be positive".to_string(),
            ));
        }
        if position_shares + 1e-9 < request.shares {
            return Err(AppError::Conflict(
                "cannot sell more shares than you hold".to_string(),
            ));
        }

        match detail.market.market_type() {
            MarketType::Native => {
                self.sell_native(
                    request,
                    detail.market.liquidity_b,
                    &detail.options,
                    option_index,
                    option,
                )
                .await
            }
            MarketType::Manifold => {
                self.sell_external(request, &detail.options, option_index, option)
                    .await
            }
        }
    }

    async fn buy_native(
        &self,
        request: BuyRequest,
        options: &[MarketOptionRecord],
        option_index: usize,
        option: MarketOptionRecord,
    ) -> AppResult<TradeReceipt> {
        let shares_state = options
            .iter()
            .map(|item| item.shares_outstanding)
            .collect::<Vec<_>>();
        let liquidity_b = self.market_liquidity(request.market_id).await?;
        let probabilities_before = lmsr_probabilities(&shares_state, liquidity_b)?;
        let price_before = probabilities_before[option_index];
        let shares_delta = shares_for_budget(
            &shares_state,
            option_index,
            request.amount_mana,
            liquidity_b,
        )?;
        let mut updated_shares = shares_state.clone();
        updated_shares[option_index] += shares_delta;
        let probabilities_after = lmsr_probabilities(&updated_shares, liquidity_b)?;
        let price_after = probabilities_after[option_index];

        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance_mana = balance_mana - ?2, updated_at = ?3 WHERE discord_user_id = ?1")
            .bind(&request.user_id)
            .bind(request.amount_mana)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE market_options SET shares_outstanding = shares_outstanding + ?2 WHERE id = ?1",
        )
        .bind(option.id)
        .bind(shares_delta)
        .execute(&mut *tx)
        .await?;
        self.upsert_position(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            shares_delta,
            request.amount_mana,
            0,
        )
        .await?;
        self.insert_trade(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            "buy",
            request.amount_mana,
            shares_delta,
            price_before,
            price_after,
            None,
            None,
        )
        .await?;
        self.insert_balance_event(
            &mut tx,
            &request.user_id,
            -request.amount_mana,
            "buy",
            request.market_id,
        )
        .await?;
        tx.commit().await?;

        let balance_mana = self.user_balance(&request.user_id).await?;
        Ok(TradeReceipt {
            market_id: request.market_id,
            market_type: "native".to_string(),
            option_label: option.label,
            balance_mana,
            mana_amount: request.amount_mana,
            shares_delta,
            price_before,
            price_after,
        })
    }

    async fn buy_external(
        &self,
        request: BuyRequest,
        options: &[MarketOptionRecord],
        option_index: usize,
        option: MarketOptionRecord,
    ) -> AppResult<TradeReceipt> {
        let (snapshot_id, refreshed_options) = self
            .ensure_recent_external_snapshot(request.market_id)
            .await?;
        let price_before = refreshed_options[option_index]
            .external_probability
            .ok_or_else(|| AppError::External("missing external price".to_string()))?;
        if !(0.0..1.0).contains(&price_before) {
            return Err(AppError::External(
                "external price must be between 0 and 1".to_string(),
            ));
        }
        let shares_delta = request.amount_mana as f64 / price_before;

        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance_mana = balance_mana - ?2, updated_at = ?3 WHERE discord_user_id = ?1")
            .bind(&request.user_id)
            .bind(request.amount_mana)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        self.upsert_position(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            shares_delta,
            request.amount_mana,
            0,
        )
        .await?;
        self.insert_trade(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            "buy",
            request.amount_mana,
            shares_delta,
            price_before,
            price_before,
            Some(price_before),
            snapshot_id,
        )
        .await?;
        self.insert_balance_event(
            &mut tx,
            &request.user_id,
            -request.amount_mana,
            "buy",
            request.market_id,
        )
        .await?;
        tx.commit().await?;

        Ok(TradeReceipt {
            market_id: request.market_id,
            market_type: "manifold".to_string(),
            option_label: option.label,
            balance_mana: self.user_balance(&request.user_id).await?,
            mana_amount: request.amount_mana,
            shares_delta,
            price_before,
            price_after: price_before,
        })
    }

    async fn sell_native(
        &self,
        request: SellRequest,
        liquidity_b: f64,
        options: &[MarketOptionRecord],
        option_index: usize,
        option: MarketOptionRecord,
    ) -> AppResult<TradeReceipt> {
        let shares_state = options
            .iter()
            .map(|item| item.shares_outstanding)
            .collect::<Vec<_>>();
        let probabilities_before = lmsr_probabilities(&shares_state, liquidity_b)?;
        let price_before = probabilities_before[option_index];
        let revenue = sale_value_for_shares(
            &shares_state,
            option_index,
            request.shares,
            liquidity_b,
        )?
        .round() as i64;
        let mut updated_shares = shares_state.clone();
        updated_shares[option_index] -= request.shares;
        let probabilities_after = lmsr_probabilities(&updated_shares, liquidity_b)?;
        let price_after = probabilities_after[option_index];

        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance_mana = balance_mana + ?2, updated_at = ?3 WHERE discord_user_id = ?1")
            .bind(&request.user_id)
            .bind(revenue)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE market_options SET shares_outstanding = shares_outstanding - ?2 WHERE id = ?1",
        )
        .bind(option.id)
        .bind(request.shares)
        .execute(&mut *tx)
        .await?;
        self.upsert_position(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            -request.shares,
            0,
            revenue,
        )
        .await?;
        self.insert_trade(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            "sell",
            revenue,
            -request.shares,
            price_before,
            price_after,
            None,
            None,
        )
        .await?;
        self.insert_balance_event(
            &mut tx,
            &request.user_id,
            revenue,
            "sell",
            request.market_id,
        )
        .await?;
        tx.commit().await?;

        Ok(TradeReceipt {
            market_id: request.market_id,
            market_type: "native".to_string(),
            option_label: option.label,
            balance_mana: self.user_balance(&request.user_id).await?,
            mana_amount: revenue,
            shares_delta: request.shares,
            price_before,
            price_after,
        })
    }

    async fn sell_external(
        &self,
        request: SellRequest,
        options: &[MarketOptionRecord],
        option_index: usize,
        option: MarketOptionRecord,
    ) -> AppResult<TradeReceipt> {
        let (snapshot_id, refreshed_options) = self
            .ensure_recent_external_snapshot(request.market_id)
            .await?;
        let price = refreshed_options[option_index]
            .external_probability
            .ok_or_else(|| AppError::External("missing external price".to_string()))?;
        let revenue = (request.shares * price).round() as i64;
        debug!(market_id = request.market_id, %price, revenue, "selling manifold position");

        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET balance_mana = balance_mana + ?2, updated_at = ?3 WHERE discord_user_id = ?1")
            .bind(&request.user_id)
            .bind(revenue)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        self.upsert_position(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            -request.shares,
            0,
            revenue,
        )
        .await?;
        self.insert_trade(
            &mut tx,
            request.market_id,
            option.id,
            &request.user_id,
            "sell",
            revenue,
            -request.shares,
            price,
            price,
            Some(price),
            snapshot_id,
        )
        .await?;
        self.insert_balance_event(
            &mut tx,
            &request.user_id,
            revenue,
            "sell",
            request.market_id,
        )
        .await?;
        tx.commit().await?;

        Ok(TradeReceipt {
            market_id: request.market_id,
            market_type: "manifold".to_string(),
            option_label: option.label,
            balance_mana: self.user_balance(&request.user_id).await?,
            mana_amount: revenue,
            shares_delta: request.shares,
            price_before: price,
            price_after: price,
        })
    }

    async fn ensure_recent_external_snapshot(
        &self,
        market_id: i64,
    ) -> AppResult<(Option<i64>, Vec<MarketOptionRecord>)> {
        let row =
            sqlx::query("SELECT external_id, last_external_sync_at FROM markets WHERE id = ?1")
                .bind(market_id)
                .fetch_one(&self.pool)
                .await?;
        let external_id: String = row.get("external_id");
        let last_sync: Option<String> = row.get("last_external_sync_at");
        let stale = last_sync
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| chrono::Utc::now() - value.with_timezone(&chrono::Utc))
            .map(|age| age.num_seconds() >= self.config.manifold_snapshot_ttl_seconds)
            .unwrap_or(true);

        if stale {
            let snapshot = self.manifold.fetch_market(&external_id).await?;
            let mut tx = self.pool.begin().await?;
            let snapshot_id = sqlx::query(
                "INSERT INTO external_market_snapshots
                 (market_id, external_source, external_id, probability, raw_status, raw_resolution, raw_json, fetched_at)
                 VALUES (?1, 'manifold', ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(market_id)
            .bind(&snapshot.external_id)
            .bind(snapshot.outcomes.first().map(|outcome| outcome.probability))
            .bind(format!("{:?}", snapshot.status))
            .bind(snapshot.resolution.as_ref().map(|value| format!("{value:?}")))
            .bind(serde_json::to_string(&snapshot.raw_json)?)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();

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
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query(
                "UPDATE markets
                 SET question = ?2, external_url = ?3, external_slug = ?4, last_external_sync_at = ?5, external_status = ?6, external_resolution = ?7, updated_at = ?5
                 WHERE id = ?1",
            )
            .bind(market_id)
            .bind(&snapshot.question)
            .bind(&snapshot.url)
            .bind(snapshot.slug.clone())
            .bind(now_rfc3339())
            .bind(format!("{:?}", snapshot.status))
            .bind(snapshot.resolution.as_ref().map(|value| format!("{value:?}")))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            let options = sqlx::query_as::<_, MarketOptionRecord>(
                "SELECT id, market_id, label, shares_outstanding, sort_order, external_option_id, external_probability
                 FROM market_options WHERE market_id = ?1 ORDER BY sort_order ASC",
            )
            .bind(market_id)
            .fetch_all(&self.pool)
            .await?;
            Ok((Some(snapshot_id), options))
        } else {
            let snapshot_id = sqlx::query(
                "SELECT id FROM external_market_snapshots WHERE market_id = ?1 ORDER BY id DESC LIMIT 1",
            )
            .bind(market_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.get::<i64, _>("id"));
            let options = sqlx::query_as::<_, MarketOptionRecord>(
                "SELECT id, market_id, label, shares_outstanding, sort_order, external_option_id, external_probability
                 FROM market_options WHERE market_id = ?1 ORDER BY sort_order ASC",
            )
            .bind(market_id)
            .fetch_all(&self.pool)
            .await?;
            Ok((snapshot_id, options))
        }
    }

    async fn ensure_user(&self, user_id: &str, display_name: &str) -> AppResult<()> {
        let existing = sqlx::query("SELECT discord_user_id FROM users WHERE discord_user_id = ?1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        if existing.is_none() {
            let now = now_rfc3339();
            let mut tx = self.pool.begin().await?;
            sqlx::query(
                "INSERT INTO users (discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 0, NULL, ?4, ?4)",
            )
            .bind(user_id)
            .bind(display_name)
            .bind(self.config.starting_balance)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO balance_events (discord_user_id, amount_mana, reason, related_market_id, created_at)
                 VALUES (?1, ?2, 'initial_grant', NULL, ?3)",
            )
            .bind(user_id)
            .bind(self.config.starting_balance)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }
        Ok(())
    }

    async fn user_balance(&self, user_id: &str) -> AppResult<i64> {
        let row = sqlx::query("SELECT balance_mana FROM users WHERE discord_user_id = ?1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("balance_mana"))
    }

    async fn load_market(&self, market_id: i64) -> AppResult<crate::domain::market::MarketDetail> {
        let market = sqlx::query_as::<_, crate::domain::market::MarketRecord>(
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
        Ok(crate::domain::market::MarketDetail { market, options })
    }

    async fn market_liquidity(&self, market_id: i64) -> AppResult<f64> {
        let row = sqlx::query("SELECT liquidity_b FROM markets WHERE id = ?1")
            .bind(market_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("liquidity_b"))
    }

    async fn position_shares(
        &self,
        market_id: i64,
        option_id: i64,
        user_id: &str,
    ) -> AppResult<f64> {
        let row = sqlx::query(
            "SELECT shares FROM positions WHERE market_id = ?1 AND option_id = ?2 AND discord_user_id = ?3",
        )
        .bind(market_id)
        .bind(option_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("shares")).unwrap_or(0.0))
    }

    async fn upsert_position(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        market_id: i64,
        option_id: i64,
        user_id: &str,
        shares_delta: f64,
        spent_delta: i64,
        received_delta: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO positions (market_id, option_id, discord_user_id, shares, total_spent_mana, total_received_mana, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(market_id, option_id, discord_user_id) DO UPDATE SET
                shares = shares + excluded.shares,
                total_spent_mana = total_spent_mana + excluded.total_spent_mana,
                total_received_mana = total_received_mana + excluded.total_received_mana,
                updated_at = excluded.updated_at",
        )
        .bind(market_id)
        .bind(option_id)
        .bind(user_id)
        .bind(shares_delta)
        .bind(spent_delta)
        .bind(received_delta)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_trade(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        market_id: i64,
        option_id: i64,
        user_id: &str,
        side: &str,
        mana_amount: i64,
        shares_delta: f64,
        price_before: f64,
        price_after: f64,
        external_price_at_trade: Option<f64>,
        external_snapshot_id: Option<i64>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO trades (market_id, option_id, discord_user_id, side, mana_amount, shares_delta, price_before, price_after, external_price_at_trade, external_snapshot_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(market_id)
        .bind(option_id)
        .bind(user_id)
        .bind(side)
        .bind(mana_amount)
        .bind(shares_delta)
        .bind(price_before)
        .bind(price_after)
        .bind(external_price_at_trade)
        .bind(external_snapshot_id)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_balance_event(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        user_id: &str,
        amount_mana: i64,
        reason: &str,
        market_id: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO balance_events (discord_user_id, amount_mana, reason, related_market_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(user_id)
        .bind(amount_mana)
        .bind(reason)
        .bind(market_id)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

fn find_option_index(options: &[MarketOptionRecord], option_label: &str) -> AppResult<usize> {
    options
        .iter()
        .position(|option| option.label.eq_ignore_ascii_case(option_label))
        .ok_or_else(|| AppError::NotFound(format!("option `{option_label}` was not found")))
}
