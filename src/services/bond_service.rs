use std::sync::Arc;

use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;
use sqlx::Row;
use tracing::{debug, instrument};

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct BondService {
    config: Arc<AppConfig>,
    pool: DbPool,
}

#[derive(Clone, Debug)]
pub struct CreateBondRequest {
    pub guild_id: String,
    pub issuer_user_id: String,
    pub issuer_display_name: String,
    pub title: String,
    pub description: Option<String>,
    pub price_per_bond_mana: i64,
    pub total_bonds: i64,
    pub yield_bps: i64,
    pub yield_period_seconds: Option<i64>,
    pub matures_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct CreateBondReceipt {
    pub issuance_id: i64,
    pub title: String,
    pub price_per_bond_mana: i64,
    pub payout_per_bond_mana: i64,
    pub total_bonds: i64,
    pub yield_bps: i64,
    pub yield_period_seconds: i64,
    pub matures_at: DateTime<Utc>,
    pub escrow_reserved_mana: i64,
    pub issuer_balance_mana: i64,
}

#[derive(Clone, Debug)]
pub struct BondListing {
    pub issuance_id: i64,
    pub title: String,
    pub issuer_display_name: String,
    pub price_per_bond_mana: i64,
    pub payout_per_bond_mana: i64,
    pub total_bonds: i64,
    pub remaining_bonds: i64,
    pub yield_bps: i64,
    pub yield_period_seconds: i64,
    pub matures_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct BondPurchaseReceipt {
    pub issuance_id: i64,
    pub title: String,
    pub quantity: i64,
    pub spent_mana: i64,
    pub payout_at_maturity_mana: i64,
    pub remaining_bonds: i64,
    pub buyer_balance_mana: i64,
}

#[derive(Clone, Debug)]
pub struct BotBondPolicy {
    pub min_yield_bps: i64,
    pub max_yield_bps: i64,
    pub min_maturity_seconds: i64,
    pub max_maturity_seconds: i64,
    pub max_price_mana: i64,
    pub max_purchase_quantity: i64,
    pub max_total_exposure_mana: i64,
}

#[derive(Clone, Debug)]
pub struct BotBondOfferOutcome {
    pub accepted: bool,
    pub quantity: i64,
    pub reason: String,
    pub receipt: Option<BondPurchaseReceipt>,
}

#[derive(Clone, Debug)]
pub struct BondPositionLine {
    pub issuance_id: i64,
    pub title: String,
    pub issuer_display_name: String,
    pub bonds_owned: i64,
    pub total_spent_mana: i64,
    pub total_redeemed_mana: i64,
    pub payout_per_bond_mana: i64,
    pub projected_payout_mana: i64,
    pub matures_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct MaturedBondSummary {
    pub issuance_id: i64,
    pub guild_id: String,
    pub title: String,
    pub holders_paid: i64,
    pub total_paid_mana: i64,
    pub issuer_refund_mana: i64,
}

impl BondService {
    pub fn new(config: Arc<AppConfig>, pool: DbPool) -> Self {
        Self { config, pool }
    }

    #[instrument(skip(self))]
    pub async fn create_bond(&self, request: CreateBondRequest) -> AppResult<CreateBondReceipt> {
        if !self.config.bonds.enabled {
            return Err(AppError::Conflict(
                "bonds are disabled by server policy".to_string(),
            ));
        }
        self.ensure_account(
            &request.guild_id,
            &request.issuer_user_id,
            &request.issuer_display_name,
        )
        .await?;
        self.validate_bond_request(&request)?;
        let open_count = sqlx::query(
            "SELECT COUNT(*) AS count
             FROM bond_issuances
             WHERE guild_id = ?1 AND issuer_discord_user_id = ?2 AND status = 'open'",
        )
        .bind(&request.guild_id)
        .bind(&request.issuer_user_id)
        .fetch_one(&self.pool)
        .await?
        .get::<i64, _>("count");
        if open_count >= self.config.bonds.max_open_issuances_per_user {
            return Err(AppError::Conflict(format!(
                "you already have the maximum number of open bond issuances ({})",
                self.config.bonds.max_open_issuances_per_user
            )));
        }

        let yield_period_seconds = request
            .yield_period_seconds
            .unwrap_or(self.config.bonds.default_yield_period_seconds);
        if yield_period_seconds <= 0 {
            return Err(AppError::Validation(
                "yield period must be positive".to_string(),
            ));
        }

        let payout_per_bond_mana = compute_bond_payout_per_bond(
            request.price_per_bond_mana,
            request.yield_bps,
            yield_period_seconds,
            request.matures_at,
        )?;
        let escrow_reserved_mana = payout_per_bond_mana
            .checked_mul(request.total_bonds)
            .ok_or_else(|| AppError::Validation("bond escrow was too large".to_string()))?;
        let issuer_balance = self
            .balance(&request.guild_id, &request.issuer_user_id)
            .await?;
        if issuer_balance < escrow_reserved_mana {
            return Err(AppError::Conflict(format!(
                "issuer needs {} available to reserve full bond payout escrow",
                escrow_reserved_mana
            )));
        }

        let now = now_rfc3339();
        let mut tx = self.pool.begin().await?;
        self.adjust_balance(
            &mut tx,
            &request.guild_id,
            &request.issuer_user_id,
            -escrow_reserved_mana,
        )
        .await?;
        self.insert_money_event(
            &mut tx,
            &request.guild_id,
            &request.issuer_user_id,
            -escrow_reserved_mana,
            "bond_escrow_reserved",
            Some(format!("reserved escrow for bond issuance `{}`", request.title)),
        )
        .await?;
        let result = sqlx::query(
            "INSERT INTO bond_issuances
             (guild_id, issuer_discord_user_id, title, description, price_per_bond_mana, payout_per_bond_mana, total_bonds, remaining_bonds, escrow_reserved_mana, escrow_remaining_mana, yield_bps, yield_period_seconds, matures_at, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?8, ?9, ?10, ?11, 'open', ?12, ?12)",
        )
        .bind(&request.guild_id)
        .bind(&request.issuer_user_id)
        .bind(&request.title)
        .bind(request.description.as_deref())
        .bind(request.price_per_bond_mana)
        .bind(payout_per_bond_mana)
        .bind(request.total_bonds)
        .bind(escrow_reserved_mana)
        .bind(request.yield_bps)
        .bind(yield_period_seconds)
        .bind(request.matures_at.to_rfc3339())
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(CreateBondReceipt {
            issuance_id: result.last_insert_rowid(),
            title: request.title,
            price_per_bond_mana: request.price_per_bond_mana,
            payout_per_bond_mana,
            total_bonds: request.total_bonds,
            yield_bps: request.yield_bps,
            yield_period_seconds,
            matures_at: request.matures_at,
            escrow_reserved_mana,
            issuer_balance_mana: self
                .balance(&request.guild_id, &request.issuer_user_id)
                .await?,
        })
    }

    #[instrument(skip(self), fields(guild_id))]
    pub async fn list_bonds(&self, guild_id: &str) -> AppResult<Vec<BondListing>> {
        let rows = sqlx::query(
            "SELECT
                b.id,
                b.title,
                b.price_per_bond_mana,
                b.payout_per_bond_mana,
                b.total_bonds,
                b.remaining_bonds,
                b.yield_bps,
                b.yield_period_seconds,
                b.matures_at,
                b.status,
                ga.display_name,
                b.issuer_discord_user_id
             FROM bond_issuances b
             LEFT JOIN guild_accounts ga
               ON ga.guild_id = b.guild_id
              AND ga.discord_user_id = b.issuer_discord_user_id
             WHERE b.guild_id = ?1
             ORDER BY
                CASE WHEN b.status = 'open' THEN 0 ELSE 1 END,
                b.matures_at ASC,
                b.id DESC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(BondListing {
                    issuance_id: row.get("id"),
                    title: row.get("title"),
                    issuer_display_name: row
                        .get::<Option<String>, _>("display_name")
                        .unwrap_or_else(|| row.get::<String, _>("issuer_discord_user_id")),
                    price_per_bond_mana: row.get("price_per_bond_mana"),
                    payout_per_bond_mana: row.get("payout_per_bond_mana"),
                    total_bonds: row.get("total_bonds"),
                    remaining_bonds: row.get("remaining_bonds"),
                    yield_bps: row.get("yield_bps"),
                    yield_period_seconds: row.get("yield_period_seconds"),
                    matures_at: parse_rfc3339_utc(&row.get::<String, _>("matures_at"))?,
                    status: row.get("status"),
                })
            })
            .collect()
    }

    #[instrument(skip(self), fields(guild_id, issuance_id))]
    pub async fn buy_bond(
        &self,
        guild_id: &str,
        buyer_user_id: &str,
        buyer_display_name: &str,
        issuance_id: i64,
        quantity: i64,
    ) -> AppResult<BondPurchaseReceipt> {
        if quantity <= 0 {
            return Err(AppError::Validation(
                "bond quantity must be positive".to_string(),
            ));
        }
        self.ensure_account(guild_id, buyer_user_id, buyer_display_name)
            .await?;

        let mut tx = self.pool.begin().await?;
        let issuance = self.load_open_issuance_tx(&mut tx, guild_id, issuance_id).await?;
        if issuance.issuer_discord_user_id == buyer_user_id {
            return Err(AppError::Validation(
                "you cannot buy your own bond issuance".to_string(),
            ));
        }
        if issuance.remaining_bonds < quantity {
            return Err(AppError::Conflict(format!(
                "only {} bonds remain in that issuance",
                issuance.remaining_bonds
            )));
        }

        let cost = issuance
            .price_per_bond_mana
            .checked_mul(quantity)
            .ok_or_else(|| AppError::Validation("bond cost was too large".to_string()))?;
        let projected_payout = issuance
            .payout_per_bond_mana
            .checked_mul(quantity)
            .ok_or_else(|| AppError::Validation("bond payout was too large".to_string()))?;
        let buyer_balance = self.balance(guild_id, buyer_user_id).await?;
        if buyer_balance < cost {
            return Err(AppError::Conflict(
                "you do not have enough balance to buy those bonds".to_string(),
            ));
        }

        self.adjust_balance(&mut tx, guild_id, buyer_user_id, -cost)
            .await?;
        self.adjust_balance(
            &mut tx,
            guild_id,
            &issuance.issuer_discord_user_id,
            cost,
        )
        .await?;
        self.insert_money_event(
            &mut tx,
            guild_id,
            buyer_user_id,
            -cost,
            "bond_purchase",
            Some(format!("bought {quantity} bond(s) from issuance #{issuance_id}")),
        )
        .await?;
        self.insert_money_event(
            &mut tx,
            guild_id,
            &issuance.issuer_discord_user_id,
            cost,
            "bond_sale_proceeds",
            Some(format!("sold {quantity} bond(s) from issuance #{issuance_id}")),
        )
        .await?;
        sqlx::query(
            "UPDATE bond_issuances
             SET remaining_bonds = remaining_bonds - ?2,
                 updated_at = ?3
             WHERE id = ?1",
        )
        .bind(issuance_id)
        .bind(quantity)
        .bind(now_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO bond_positions
             (issuance_id, guild_id, holder_discord_user_id, bonds_owned, total_spent_mana, total_redeemed_mana, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)
             ON CONFLICT(issuance_id, holder_discord_user_id) DO UPDATE SET
                bonds_owned = bonds_owned + excluded.bonds_owned,
                total_spent_mana = total_spent_mana + excluded.total_spent_mana,
                updated_at = excluded.updated_at",
        )
        .bind(issuance_id)
        .bind(guild_id)
        .bind(buyer_user_id)
        .bind(quantity)
        .bind(cost)
        .bind(now_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let remaining_bonds = self
            .issuance_remaining_bonds(guild_id, issuance_id)
            .await?;
        Ok(BondPurchaseReceipt {
            issuance_id,
            title: issuance.title,
            quantity,
            spent_mana: cost,
            payout_at_maturity_mana: projected_payout,
            remaining_bonds,
            buyer_balance_mana: self.balance(guild_id, buyer_user_id).await?,
        })
    }

    #[instrument(skip(self), fields(guild_id, holder_user_id))]
    pub async fn positions_for_holder(
        &self,
        guild_id: &str,
        holder_user_id: &str,
    ) -> AppResult<Vec<BondPositionLine>> {
        let rows = sqlx::query(
            "SELECT
                b.id,
                b.title,
                b.payout_per_bond_mana,
                b.matures_at,
                b.status,
                p.bonds_owned,
                p.total_spent_mana,
                p.total_redeemed_mana,
                ga.display_name,
                b.issuer_discord_user_id
             FROM bond_positions p
             JOIN bond_issuances b ON b.id = p.issuance_id
             LEFT JOIN guild_accounts ga
               ON ga.guild_id = b.guild_id
              AND ga.discord_user_id = b.issuer_discord_user_id
             WHERE p.guild_id = ?1 AND p.holder_discord_user_id = ?2 AND p.bonds_owned > 0
             ORDER BY b.matures_at ASC, b.id DESC",
        )
        .bind(guild_id)
        .bind(holder_user_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let bonds_owned = row.get::<i64, _>("bonds_owned");
                let payout_per_bond_mana = row.get::<i64, _>("payout_per_bond_mana");
                Ok(BondPositionLine {
                    issuance_id: row.get("id"),
                    title: row.get("title"),
                    issuer_display_name: row
                        .get::<Option<String>, _>("display_name")
                        .unwrap_or_else(|| row.get::<String, _>("issuer_discord_user_id")),
                    bonds_owned,
                    total_spent_mana: row.get("total_spent_mana"),
                    total_redeemed_mana: row.get("total_redeemed_mana"),
                    payout_per_bond_mana,
                    projected_payout_mana: payout_per_bond_mana * bonds_owned,
                    matures_at: parse_rfc3339_utc(&row.get::<String, _>("matures_at"))?,
                    status: row.get("status"),
                })
            })
            .collect()
    }

    #[instrument(skip(self))]
    pub async fn autocomplete_open_bonds(
        &self,
        guild_id: &str,
        partial: &str,
        limit: i64,
    ) -> AppResult<Vec<serenity::AutocompleteChoice>> {
        let like = format!("%{}%", partial.trim());
        let rows = sqlx::query(
            "SELECT id, title, remaining_bonds, payout_per_bond_mana
             FROM bond_issuances
             WHERE guild_id = ?1
               AND status = 'open'
               AND (?2 = '' OR title LIKE ?3 OR CAST(id AS TEXT) LIKE ?3)
             ORDER BY matures_at ASC, id DESC
             LIMIT ?4",
        )
        .bind(guild_id)
        .bind(partial.trim())
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                serenity::AutocompleteChoice::new(
                    format!(
                        "#{} [{} left | {} payout] {}",
                        row.get::<i64, _>("id"),
                        row.get::<i64, _>("remaining_bonds"),
                        row.get::<i64, _>("payout_per_bond_mana"),
                        row.get::<String, _>("title")
                    ),
                    row.get::<i64, _>("id").to_string(),
                )
            })
            .collect())
    }

    #[instrument(skip(self))]
    pub async fn mature_due_bonds(&self) -> AppResult<Vec<MaturedBondSummary>> {
        let rows = sqlx::query(
            "SELECT id, guild_id
             FROM bond_issuances
             WHERE status = 'open' AND matures_at <= ?1
             ORDER BY matures_at ASC, id ASC",
        )
        .bind(now_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut matured = Vec::new();
        for row in rows {
            let issuance_id = row.get::<i64, _>("id");
            let guild_id = row.get::<String, _>("guild_id");
            matured.push(self.mature_single_issuance(&guild_id, issuance_id).await?);
        }
        Ok(matured)
    }

    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self), fields(guild_id, buyer_user_id))]
    pub async fn auto_buy_eligible_bonds(
        &self,
        guild_id: &str,
        buyer_user_id: &str,
        buyer_display_name: &str,
        min_yield_bps: i64,
        max_yield_bps: i64,
        min_maturity_seconds: i64,
        max_maturity_seconds: i64,
        max_price_mana: i64,
        max_purchase_quantity: i64,
        max_total_exposure_mana: i64,
    ) -> AppResult<u64> {
        let policy = BotBondPolicy {
            min_yield_bps,
            max_yield_bps,
            min_maturity_seconds,
            max_maturity_seconds,
            max_price_mana,
            max_purchase_quantity,
            max_total_exposure_mana,
        };
        if !self.config.bonds.enabled || policy.max_purchase_quantity <= 0 {
            return Ok(0);
        }
        self.ensure_account(guild_id, buyer_user_id, buyer_display_name)
            .await?;

        let current_exposure = self
            .open_exposure_for_holder(guild_id, buyer_user_id)
            .await?;
        if current_exposure >= policy.max_total_exposure_mana {
            return Ok(0);
        }

        let rows = sqlx::query(
            "SELECT
                id,
                issuer_discord_user_id,
                title,
                price_per_bond_mana,
                payout_per_bond_mana,
                remaining_bonds,
                yield_bps,
                yield_period_seconds,
                matures_at
             FROM bond_issuances
             WHERE guild_id = ?1 AND status = 'open'
             ORDER BY matures_at ASC, id ASC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;

        let mut purchased = 0;
        let mut remaining_exposure_budget = policy.max_total_exposure_mana - current_exposure;
        for row in rows {
            let issuance = BondIssuanceRow {
                id: row.get("id"),
                guild_id: guild_id.to_string(),
                issuer_discord_user_id: row.get("issuer_discord_user_id"),
                title: row.get("title"),
                price_per_bond_mana: row.get("price_per_bond_mana"),
                payout_per_bond_mana: row.get("payout_per_bond_mana"),
                total_bonds: 0,
                remaining_bonds: row.get("remaining_bonds"),
                escrow_reserved_mana: 0,
                yield_bps: row.get("yield_bps"),
                yield_period_seconds: row.get("yield_period_seconds"),
                matures_at: row.get("matures_at"),
                status: "open".to_string(),
            };

            let evaluation = self
                .evaluate_bot_bond_purchase(
                    guild_id,
                    buyer_user_id,
                    &issuance,
                    remaining_exposure_budget,
                    &policy,
                )
                .await?;
            let Some(quantity) = evaluation.approved_quantity else {
                continue;
            };

            if self
                .buy_bond(
                    guild_id,
                    buyer_user_id,
                    buyer_display_name,
                    issuance.id,
                    quantity,
                )
                .await
                .is_ok()
            {
                purchased += quantity as u64;
                remaining_exposure_budget = remaining_exposure_budget
                    .saturating_sub(issuance.price_per_bond_mana * quantity);
                if remaining_exposure_budget <= 0 {
                    break;
                }
            }
        }

        Ok(purchased)
    }

    #[instrument(skip(self), fields(guild_id, seller_user_id, issuance_id))]
    pub async fn sell_bond_to_bot(
        &self,
        guild_id: &str,
        seller_user_id: &str,
        seller_display_name: &str,
        bot_user_id: &str,
        bot_display_name: &str,
        issuance_id: i64,
    ) -> AppResult<BotBondOfferOutcome> {
        let policy = BotBondPolicy {
            min_yield_bps: self.config.bot.min_bond_yield_bps,
            max_yield_bps: self.config.bot.max_bond_yield_bps,
            min_maturity_seconds: self.config.bot.min_bond_maturity_seconds,
            max_maturity_seconds: self.config.bot.max_bond_maturity_seconds,
            max_price_mana: self.config.bot.max_bond_price_mana,
            max_purchase_quantity: self.config.bot.max_bond_purchase_quantity,
            max_total_exposure_mana: self.config.bot.max_total_bond_exposure_mana,
        };
        if !self.config.bonds.enabled {
            return Ok(BotBondOfferOutcome {
                accepted: false,
                quantity: 0,
                reason: "bot bond buying is disabled because bonds are disabled in config".to_string(),
                receipt: None,
            });
        }
        if !self.config.bot.auto_buy_bonds {
            return Ok(BotBondOfferOutcome {
                accepted: false,
                quantity: 0,
                reason: "the bot is not buying bonds right now".to_string(),
                receipt: None,
            });
        }

        self.ensure_account(guild_id, seller_user_id, seller_display_name)
            .await?;
        self.ensure_account(guild_id, bot_user_id, bot_display_name)
            .await?;

        let mut tx = self.pool.begin().await?;
        let issuance = self.load_open_issuance_tx(&mut tx, guild_id, issuance_id).await?;
        drop(tx);

        let current_exposure = self.open_exposure_for_holder(guild_id, bot_user_id).await?;
        let remaining_exposure_budget =
            policy.max_total_exposure_mana.saturating_sub(current_exposure);
        let evaluation = self
            .evaluate_bot_bond_purchase(
                guild_id,
                bot_user_id,
                &issuance,
                remaining_exposure_budget,
                &policy,
            )
            .await?;
        let Some(quantity) = evaluation.approved_quantity else {
            return Ok(BotBondOfferOutcome {
                accepted: false,
                quantity: 0,
                reason: evaluation.reason,
                receipt: None,
            });
        };

        let receipt = self
            .buy_bond(
                guild_id,
                bot_user_id,
                bot_display_name,
                issuance_id,
                quantity,
            )
            .await?;
        Ok(BotBondOfferOutcome {
            accepted: true,
            quantity,
            reason: evaluation.reason,
            receipt: Some(receipt),
        })
    }

    async fn mature_single_issuance(
        &self,
        guild_id: &str,
        issuance_id: i64,
    ) -> AppResult<MaturedBondSummary> {
        let mut tx = self.pool.begin().await?;
        let issuance = self.load_any_issuance_tx(&mut tx, guild_id, issuance_id).await?;
        if issuance.status != "open" {
            return Ok(MaturedBondSummary {
                issuance_id,
                guild_id: guild_id.to_string(),
                title: issuance.title,
                holders_paid: 0,
                total_paid_mana: 0,
                issuer_refund_mana: 0,
            });
        }

        let positions = sqlx::query(
            "SELECT holder_discord_user_id, bonds_owned
             FROM bond_positions
             WHERE issuance_id = ?1 AND guild_id = ?2 AND bonds_owned > 0",
        )
        .bind(issuance_id)
        .bind(guild_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut total_paid_mana = 0;
        let mut holders_paid = 0;
        for row in positions {
            let holder_user_id = row.get::<String, _>("holder_discord_user_id");
            let bonds_owned = row.get::<i64, _>("bonds_owned");
            let payout = issuance
                .payout_per_bond_mana
                .checked_mul(bonds_owned)
                .ok_or_else(|| AppError::Validation("bond maturity payout was too large".to_string()))?;
            if payout <= 0 {
                continue;
            }
            holders_paid += 1;
            total_paid_mana += payout;
            self.adjust_balance(&mut tx, guild_id, &holder_user_id, payout)
                .await?;
            self.insert_money_event(
                &mut tx,
                guild_id,
                &holder_user_id,
                payout,
                "bond_matured_payout",
                Some(format!("bond issuance #{issuance_id} matured")),
            )
            .await?;
            sqlx::query(
                "UPDATE bond_positions
                 SET total_redeemed_mana = total_redeemed_mana + ?2,
                     bonds_owned = 0,
                     updated_at = ?3
                 WHERE issuance_id = ?1 AND holder_discord_user_id = ?4",
            )
            .bind(issuance_id)
            .bind(payout)
            .bind(now_rfc3339())
            .bind(&holder_user_id)
            .execute(&mut *tx)
            .await?;
        }

        let unsold_bonds = issuance.remaining_bonds.max(0);
        let issuer_refund_mana = issuance
            .payout_per_bond_mana
            .checked_mul(unsold_bonds)
            .ok_or_else(|| AppError::Validation("bond refund was too large".to_string()))?;
        if issuer_refund_mana > 0 {
            self.adjust_balance(
                &mut tx,
                guild_id,
                &issuance.issuer_discord_user_id,
                issuer_refund_mana,
            )
            .await?;
            self.insert_money_event(
                &mut tx,
                guild_id,
                &issuance.issuer_discord_user_id,
                issuer_refund_mana,
                "bond_unsold_refund",
                Some(format!("refunded unsold bond escrow from issuance #{issuance_id}")),
            )
            .await?;
        }

        let escrow_remaining_mana = issuance
            .escrow_reserved_mana
            .saturating_sub(total_paid_mana)
            .saturating_sub(issuer_refund_mana);
        sqlx::query(
            "UPDATE bond_issuances
             SET status = 'matured',
                 remaining_bonds = 0,
                 escrow_remaining_mana = ?2,
                 updated_at = ?3
             WHERE id = ?1",
        )
        .bind(issuance_id)
        .bind(escrow_remaining_mana)
        .bind(now_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        debug!(
            issuance_id,
            holders_paid,
            total_paid_mana,
            issuer_refund_mana,
            "matured bond issuance"
        );

        Ok(MaturedBondSummary {
            issuance_id,
            guild_id: guild_id.to_string(),
            title: issuance.title,
            holders_paid,
            total_paid_mana,
            issuer_refund_mana,
        })
    }

    fn validate_bond_request(&self, request: &CreateBondRequest) -> AppResult<()> {
        if request.title.trim().is_empty() {
            return Err(AppError::Validation(
                "bond title cannot be empty".to_string(),
            ));
        }
        if request.price_per_bond_mana <= 0 {
            return Err(AppError::Validation(
                "bond price must be positive".to_string(),
            ));
        }
        if request.total_bonds <= 0 {
            return Err(AppError::Validation(
                "total bonds must be positive".to_string(),
            ));
        }
        if request.yield_bps < 0 || request.yield_bps > self.config.bonds.max_yield_bps {
            return Err(AppError::Validation(format!(
                "yield_bps must be between 0 and {}",
                self.config.bonds.max_yield_bps
            )));
        }
        let maturity_seconds = (request.matures_at - Utc::now()).num_seconds();
        if maturity_seconds < self.config.bonds.min_maturity_seconds
            || maturity_seconds > self.config.bonds.max_maturity_seconds
        {
            return Err(AppError::Validation(format!(
                "maturity must be between {} and {} seconds from now",
                self.config.bonds.min_maturity_seconds,
                self.config.bonds.max_maturity_seconds
            )));
        }
        Ok(())
    }

    async fn ensure_account(
        &self,
        guild_id: &str,
        user_id: &str,
        display_name: &str,
    ) -> AppResult<()> {
        let existing = sqlx::query(
            "SELECT 1 FROM guild_accounts WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_none() {
            sqlx::query(
                "INSERT INTO guild_accounts
                 (guild_id, discord_user_id, display_name, balance_mana, total_claimed_mana, last_claim_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, NULL, ?5, ?5)",
            )
            .bind(guild_id)
            .bind(user_id)
            .bind(display_name)
            .bind(self.config.starting_balance)
            .bind(now_rfc3339())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn balance(&self, guild_id: &str, user_id: &str) -> AppResult<i64> {
        let row = sqlx::query(
            "SELECT balance_mana
             FROM guild_accounts
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("balance_mana"))
    }

    async fn adjust_balance(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        user_id: &str,
        delta: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE guild_accounts
             SET balance_mana = balance_mana + ?3, updated_at = ?4
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(delta)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_money_event(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        user_id: &str,
        amount_mana: i64,
        reason: &str,
        note: Option<String>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO economy_events
             (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
             VALUES (?1, ?2, NULL, NULL, 'money', ?3, NULL, ?4, ?5, ?6)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(amount_mana)
        .bind(reason)
        .bind(note)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn load_open_issuance_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        issuance_id: i64,
    ) -> AppResult<BondIssuanceRow> {
        let issuance = self.load_any_issuance_tx(tx, guild_id, issuance_id).await?;
        if issuance.status != "open" {
            return Err(AppError::Conflict(format!(
                "bond issuance #{issuance_id} is not open; current status is {}",
                issuance.status
            )));
        }
        Ok(issuance)
    }

    async fn load_any_issuance_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        issuance_id: i64,
    ) -> AppResult<BondIssuanceRow> {
        sqlx::query_as::<_, BondIssuanceRow>(
            "SELECT id, guild_id, issuer_discord_user_id, title, price_per_bond_mana, payout_per_bond_mana, total_bonds, remaining_bonds, escrow_reserved_mana, yield_bps, yield_period_seconds, matures_at, status
             FROM bond_issuances
             WHERE guild_id = ?1 AND id = ?2",
        )
        .bind(guild_id)
        .bind(issuance_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("bond issuance {issuance_id} was not found")))
    }

    async fn issuance_remaining_bonds(&self, guild_id: &str, issuance_id: i64) -> AppResult<i64> {
        let row = sqlx::query(
            "SELECT remaining_bonds
             FROM bond_issuances
             WHERE guild_id = ?1 AND id = ?2",
        )
        .bind(guild_id)
        .bind(issuance_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("remaining_bonds"))
    }

    async fn open_exposure_for_holder(&self, guild_id: &str, holder_user_id: &str) -> AppResult<i64> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(p.total_spent_mana), 0) AS exposure
             FROM bond_positions p
             JOIN bond_issuances b ON b.id = p.issuance_id
             WHERE p.guild_id = ?1
               AND p.holder_discord_user_id = ?2
               AND p.bonds_owned > 0
               AND b.status = 'open'",
        )
        .bind(guild_id)
        .bind(holder_user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("exposure"))
    }

    async fn holder_has_open_bond_position(
        &self,
        guild_id: &str,
        holder_user_id: &str,
        issuance_id: i64,
    ) -> AppResult<bool> {
        Ok(sqlx::query(
            "SELECT 1
             FROM bond_positions
             WHERE issuance_id = ?1 AND guild_id = ?2 AND holder_discord_user_id = ?3 AND bonds_owned > 0",
        )
        .bind(issuance_id)
        .bind(guild_id)
        .bind(holder_user_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some())
    }

    async fn evaluate_bot_bond_purchase(
        &self,
        guild_id: &str,
        buyer_user_id: &str,
        issuance: &BondIssuanceRow,
        remaining_exposure_budget: i64,
        policy: &BotBondPolicy,
    ) -> AppResult<BotBondEvaluation> {
        if issuance.issuer_discord_user_id == buyer_user_id {
            return Ok(BotBondEvaluation::reject(
                "the bot will not buy its own bond issuance",
            ));
        }
        if issuance.remaining_bonds <= 0 {
            return Ok(BotBondEvaluation::reject("that bond issuance has no supply left"));
        }
        if policy.max_purchase_quantity <= 0 {
            return Ok(BotBondEvaluation::reject("the bot is configured to buy zero bonds"));
        }
        if remaining_exposure_budget <= 0 {
            return Ok(BotBondEvaluation::reject(
                "the bot has already hit its bond exposure limit",
            ));
        }
        if self
            .holder_has_open_bond_position(guild_id, buyer_user_id, issuance.id)
            .await?
        {
            return Ok(BotBondEvaluation::reject(
                "the bot already holds this bond issuance",
            ));
        }

        let matures_at = parse_rfc3339_utc(&issuance.matures_at)?;
        let maturity_seconds = (matures_at - Utc::now()).num_seconds();
        if issuance.price_per_bond_mana > policy.max_price_mana {
            return Ok(BotBondEvaluation::reject(format!(
                "price {} is above the bot limit of {}",
                issuance.price_per_bond_mana, policy.max_price_mana
            )));
        }
        if issuance.yield_bps < policy.min_yield_bps {
            return Ok(BotBondEvaluation::reject(format!(
                "yield {} bps is below the bot minimum of {} bps",
                issuance.yield_bps, policy.min_yield_bps
            )));
        }
        if issuance.yield_bps > policy.max_yield_bps {
            return Ok(BotBondEvaluation::reject(format!(
                "yield {} bps is above the bot maximum of {} bps",
                issuance.yield_bps, policy.max_yield_bps
            )));
        }
        if maturity_seconds < policy.min_maturity_seconds {
            return Ok(BotBondEvaluation::reject(format!(
                "maturity {}s is shorter than the bot minimum of {}s",
                maturity_seconds, policy.min_maturity_seconds
            )));
        }
        if maturity_seconds > policy.max_maturity_seconds {
            return Ok(BotBondEvaluation::reject(format!(
                "maturity {}s is longer than the bot maximum of {}s",
                maturity_seconds, policy.max_maturity_seconds
            )));
        }

        let affordable_by_policy = (remaining_exposure_budget / issuance.price_per_bond_mana).max(0);
        let affordable_by_balance =
            (self.balance(guild_id, buyer_user_id).await? / issuance.price_per_bond_mana).max(0);
        let quantity = issuance
            .remaining_bonds
            .min(policy.max_purchase_quantity)
            .min(affordable_by_policy)
            .min(affordable_by_balance);
        if quantity <= 0 {
            return Ok(BotBondEvaluation::reject(
                "the bot cannot currently afford even one bond under its balance and exposure limits",
            ));
        }

        Ok(BotBondEvaluation {
            approved_quantity: Some(quantity),
            reason: format!(
                "accepted {} bond(s) because yield, price, maturity, and exposure limits all passed",
                quantity
            ),
        })
    }
}

#[derive(sqlx::FromRow)]
struct BondIssuanceRow {
    id: i64,
    guild_id: String,
    issuer_discord_user_id: String,
    title: String,
    price_per_bond_mana: i64,
    payout_per_bond_mana: i64,
    total_bonds: i64,
    remaining_bonds: i64,
    escrow_reserved_mana: i64,
    yield_bps: i64,
    yield_period_seconds: i64,
    matures_at: String,
    status: String,
}

struct BotBondEvaluation {
    approved_quantity: Option<i64>,
    reason: String,
}

impl BotBondEvaluation {
    fn reject(reason: impl Into<String>) -> Self {
        Self {
            approved_quantity: None,
            reason: reason.into(),
        }
    }
}

fn compute_bond_payout_per_bond(
    price_per_bond_mana: i64,
    yield_bps: i64,
    yield_period_seconds: i64,
    matures_at: DateTime<Utc>,
) -> AppResult<i64> {
    let duration_seconds = (matures_at - Utc::now()).num_seconds().max(1);
    let periods = ((duration_seconds + yield_period_seconds - 1) / yield_period_seconds).max(1);
    let growth = (1.0 + (yield_bps as f64 / 10_000.0)).powf(periods as f64);
    let payout = (price_per_bond_mana as f64 * growth).round() as i64;
    if payout <= 0 {
        return Err(AppError::Validation(
            "bond payout must be positive".to_string(),
        ));
    }
    Ok(payout)
}

fn parse_rfc3339_utc(value: &str) -> AppResult<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::Other(anyhow::anyhow!("invalid RFC3339 timestamp: {value}")))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use tempfile::tempdir;

    use crate::config::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };
    use crate::db;
    use crate::services::bond_service::CreateBondRequest;

    fn test_config(cache_dir: std::path::PathBuf) -> AppConfig {
        AppConfig {
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
            starting_balance: 100_000,
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
                starting_balance: 100_000,
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
                startup_announcement_channel_name: "bots".to_string(),
                startup_announcement_fallback_channel_name: "general".to_string(),
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
                symbol: "$".to_string(),
                textual_symbol: "mana".to_string(),
                emoji: "money".to_string(),
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
    async fn sell_bond_to_bot_accepts_eligible_bond() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(test_config(cache_dir.clone()));
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::BondService::new(config, pool);

        let receipt = service
            .create_bond(CreateBondRequest {
                guild_id: "guild-a".to_string(),
                issuer_user_id: "seller".to_string(),
                issuer_display_name: "Seller".to_string(),
                title: "Treasury Note".to_string(),
                description: None,
                price_per_bond_mana: 1_000,
                total_bonds: 3,
                yield_bps: 400,
                yield_period_seconds: Some(3_600),
                matures_at: Utc::now() + Duration::hours(2),
            })
            .await
            .expect("create bond");

        let outcome = service
            .sell_bond_to_bot(
                "guild-a",
                "seller",
                "Seller",
                "bot",
                "Profit Rat",
                receipt.issuance_id,
            )
            .await
            .expect("sell to bot");

        assert!(outcome.accepted);
        assert_eq!(outcome.quantity, 1);
        assert!(outcome.receipt.is_some());
    }

    #[tokio::test]
    async fn sell_bond_to_bot_rejects_expensive_bond() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(test_config(cache_dir.clone()));
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::BondService::new(config, pool);

        let receipt = service
            .create_bond(CreateBondRequest {
                guild_id: "guild-a".to_string(),
                issuer_user_id: "seller".to_string(),
                issuer_display_name: "Seller".to_string(),
                title: "Too Rich For Rat".to_string(),
                description: None,
                price_per_bond_mana: 6_000,
                total_bonds: 2,
                yield_bps: 400,
                yield_period_seconds: Some(3_600),
                matures_at: Utc::now() + Duration::hours(2),
            })
            .await
            .expect("create bond");

        let outcome = service
            .sell_bond_to_bot(
                "guild-a",
                "seller",
                "Seller",
                "bot",
                "Profit Rat",
                receipt.issuance_id,
            )
            .await
            .expect("sell to bot");

        assert!(!outcome.accepted);
        assert_eq!(outcome.quantity, 0);
        assert!(outcome.reason.contains("above the bot limit"));
    }
}
