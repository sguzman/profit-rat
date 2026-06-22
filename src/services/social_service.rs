use std::sync::Arc;

use chrono::{Duration, Utc};
use poise::serenity_prelude as serenity;
use sqlx::{FromRow, Row};
use tracing::instrument;

use crate::config::AppConfig;
use crate::db::{DbPool, now_rfc3339};
use crate::domain::market::{MarketOptionRecord, MarketRecord};
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct SocialService {
    config: Arc<AppConfig>,
    pool: DbPool,
}

#[derive(Clone, Debug)]
pub struct MoneyDonationReceipt {
    pub recipient_display_name: String,
    pub amount_mana: i64,
    pub sender_balance_mana: i64,
}

#[derive(Clone, Debug)]
pub struct ShareDonationReceipt {
    pub recipient_display_name: String,
    pub market_id: i64,
    pub option_label: String,
    pub shares: f64,
}

#[derive(Clone, Debug)]
pub struct LoanOfferReceipt {
    pub loan_id: i64,
    pub borrower_display_name: String,
    pub asset_type: String,
    pub market_id: Option<i64>,
    pub option_label: Option<String>,
    pub principal_mana: Option<i64>,
    pub principal_shares: Option<f64>,
    pub repayment_mana: Option<i64>,
    pub repayment_shares: Option<f64>,
    pub interest_bps: i64,
    pub expires_at: chrono::DateTime<Utc>,
    pub due_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct IncomingLoanSummary {
    pub loan_id: i64,
    pub lender_display_name: String,
    pub asset_type: String,
    pub market_id: Option<i64>,
    pub market_question: Option<String>,
    pub option_label: Option<String>,
    pub principal_mana: Option<i64>,
    pub principal_shares: Option<f64>,
    pub repayment_mana: Option<i64>,
    pub repayment_shares: Option<f64>,
    pub due_at: chrono::DateTime<Utc>,
    pub expires_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct LoanActionReceipt {
    pub loan_id: i64,
    pub counterparty_display_name: String,
    pub asset_type: String,
    pub market_id: Option<i64>,
    pub option_label: Option<String>,
    pub principal_mana: Option<i64>,
    pub principal_shares: Option<f64>,
    pub repayment_mana: Option<i64>,
    pub repayment_shares: Option<f64>,
    pub status: String,
    pub due_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct LoanStatusLine {
    pub loan_id: i64,
    pub direction: String,
    pub counterparty_display_name: String,
    pub asset_type: String,
    pub market_id: Option<i64>,
    pub option_label: Option<String>,
    pub principal_mana: Option<i64>,
    pub principal_shares: Option<f64>,
    pub repayment_mana: Option<i64>,
    pub repayment_shares: Option<f64>,
    pub repaid_mana: i64,
    pub repaid_shares: f64,
    pub status: String,
    pub due_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RepaymentReceipt {
    pub loan_id: i64,
    pub asset_type: String,
    pub paid_mana: Option<i64>,
    pub paid_shares: Option<f64>,
    pub remaining_mana: Option<i64>,
    pub remaining_shares: Option<f64>,
    pub status: String,
}

#[derive(Clone, Debug, FromRow)]
struct LoanRecord {
    id: i64,
    guild_id: String,
    asset_type: String,
    market_id: Option<i64>,
    option_id: Option<i64>,
    lender_discord_user_id: String,
    borrower_discord_user_id: String,
    principal_mana: Option<i64>,
    principal_shares: Option<f64>,
    repayment_mana: Option<i64>,
    repayment_shares: Option<f64>,
    interest_bps: i64,
    due_at: String,
    status: String,
    expires_at: String,
}

impl SocialService {
    pub fn new(config: Arc<AppConfig>, pool: DbPool) -> Self {
        Self { config, pool }
    }

    #[instrument(skip(self), fields(guild_id, sender_user_id, recipient_user_id, amount_mana))]
    pub async fn donate_money(
        &self,
        guild_id: &str,
        sender_user_id: &str,
        sender_display_name: &str,
        recipient_user_id: &str,
        recipient_display_name: &str,
        amount_mana: i64,
    ) -> AppResult<MoneyDonationReceipt> {
        if !self.config.transfers.allow_money_donations {
            return Err(AppError::Conflict(
                "money donations are disabled by server policy".to_string(),
            ));
        }
        if sender_user_id == recipient_user_id {
            return Err(AppError::Validation(
                "you cannot donate money to yourself".to_string(),
            ));
        }
        if amount_mana < self.config.transfers.min_money_transfer {
            return Err(AppError::Validation(format!(
                "money donations must be at least {}",
                self.config.transfers.min_money_transfer
            )));
        }

        self.ensure_account(guild_id, sender_user_id, sender_display_name)
            .await?;
        self.ensure_account(guild_id, recipient_user_id, recipient_display_name)
            .await?;

        let sender_balance = self.balance(guild_id, sender_user_id).await?;
        if sender_balance < amount_mana {
            return Err(AppError::Conflict(
                "you do not have enough balance for that donation".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        self.adjust_balance(&mut tx, guild_id, sender_user_id, -amount_mana).await?;
        self.adjust_balance(&mut tx, guild_id, recipient_user_id, amount_mana).await?;
        self.insert_money_event(
            &mut tx,
            guild_id,
            sender_user_id,
            -amount_mana,
            "donate_money_sent",
            None,
            None,
        )
        .await?;
        self.insert_money_event(
            &mut tx,
            guild_id,
            recipient_user_id,
            amount_mana,
            "donate_money_received",
            None,
            None,
        )
        .await?;
        tx.commit().await?;

        Ok(MoneyDonationReceipt {
            recipient_display_name: recipient_display_name.to_string(),
            amount_mana,
            sender_balance_mana: self.balance(guild_id, sender_user_id).await?,
        })
    }

    #[instrument(skip(self), fields(guild_id, sender_user_id, recipient_user_id, market_id, shares))]
    pub async fn donate_shares(
        &self,
        guild_id: &str,
        sender_user_id: &str,
        sender_display_name: &str,
        recipient_user_id: &str,
        recipient_display_name: &str,
        market_id: i64,
        option_label: &str,
        shares: f64,
    ) -> AppResult<ShareDonationReceipt> {
        if !self.config.transfers.allow_share_donations {
            return Err(AppError::Conflict(
                "share donations are disabled by server policy".to_string(),
            ));
        }
        if sender_user_id == recipient_user_id {
            return Err(AppError::Validation(
                "you cannot donate shares to yourself".to_string(),
            ));
        }
        if shares < self.config.transfers.min_share_transfer {
            return Err(AppError::Validation(format!(
                "share donations must be at least {:.4} shares",
                self.config.transfers.min_share_transfer
            )));
        }

        self.ensure_account(guild_id, sender_user_id, sender_display_name)
            .await?;
        self.ensure_account(guild_id, recipient_user_id, recipient_display_name)
            .await?;

        let (market, option) = self.load_market_and_option(guild_id, market_id, option_label).await?;
        let sender_shares = self.position_shares(market_id, option.id, sender_user_id).await?;
        if sender_shares + 1e-9 < shares {
            return Err(AppError::Conflict(
                "you do not have enough shares for that donation".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        self.upsert_position(
            &mut tx,
            market_id,
            option.id,
            sender_user_id,
            -shares,
            0,
            0,
        )
        .await?;
        self.upsert_position(
            &mut tx,
            market_id,
            option.id,
            recipient_user_id,
            shares,
            0,
            0,
        )
        .await?;
        self.insert_share_event(
            &mut tx,
            guild_id,
            sender_user_id,
            market_id,
            option.id,
            -shares,
            "donate_shares_sent",
        )
        .await?;
        self.insert_share_event(
            &mut tx,
            guild_id,
            recipient_user_id,
            market_id,
            option.id,
            shares,
            "donate_shares_received",
        )
        .await?;
        tx.commit().await?;

        Ok(ShareDonationReceipt {
            recipient_display_name: recipient_display_name.to_string(),
            market_id: market.id,
            option_label: option.label,
            shares,
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self), fields(guild_id, lender_user_id, borrower_user_id, principal_mana))]
    pub async fn offer_loan_money(
        &self,
        guild_id: &str,
        lender_user_id: &str,
        lender_display_name: &str,
        borrower_user_id: &str,
        borrower_display_name: &str,
        principal_mana: i64,
        interest_bps: Option<i64>,
        duration_seconds: Option<i64>,
    ) -> AppResult<LoanOfferReceipt> {
        if !self.config.loans.allow_money_loans {
            return Err(AppError::Conflict(
                "money loans are disabled by server policy".to_string(),
            ));
        }
        self.ensure_loan_parties(
            guild_id,
            lender_user_id,
            lender_display_name,
            borrower_user_id,
            borrower_display_name,
        )
        .await?;
        if principal_mana < self.config.transfers.min_money_transfer {
            return Err(AppError::Validation("loan principal is too small".to_string()));
        }

        let interest_bps = self.normalize_interest_bps(interest_bps)?;
        let duration_seconds = self.normalize_duration(duration_seconds)?;
        let lender_balance = self.balance(guild_id, lender_user_id).await?;
        if lender_balance < principal_mana {
            return Err(AppError::Conflict(
                "lender does not currently have enough balance".to_string(),
            ));
        }

        let repayment_mana =
            principal_mana + ((principal_mana * interest_bps + 9_999) / 10_000);
        let (expires_at, due_at) = self.loan_timestamps(duration_seconds);

        let result = sqlx::query(
            "INSERT INTO loans
             (guild_id, asset_type, market_id, option_id, lender_discord_user_id, borrower_discord_user_id, principal_mana, principal_shares, repayment_mana, repayment_shares, interest_bps, due_at, status, created_at, accepted_at, responded_at, closed_at, expires_at)
             VALUES (?1, 'money', NULL, NULL, ?2, ?3, ?4, NULL, ?5, NULL, ?6, ?7, 'pending', ?8, NULL, NULL, NULL, ?9)",
        )
        .bind(guild_id)
        .bind(lender_user_id)
        .bind(borrower_user_id)
        .bind(principal_mana)
        .bind(repayment_mana)
        .bind(interest_bps)
        .bind(due_at.to_rfc3339())
        .bind(now_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(LoanOfferReceipt {
            loan_id: result.last_insert_rowid(),
            borrower_display_name: borrower_display_name.to_string(),
            asset_type: "money".to_string(),
            market_id: None,
            option_label: None,
            principal_mana: Some(principal_mana),
            principal_shares: None,
            repayment_mana: Some(repayment_mana),
            repayment_shares: None,
            interest_bps,
            expires_at,
            due_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self), fields(guild_id, lender_user_id, borrower_user_id, market_id, shares))]
    pub async fn offer_loan_shares(
        &self,
        guild_id: &str,
        lender_user_id: &str,
        lender_display_name: &str,
        borrower_user_id: &str,
        borrower_display_name: &str,
        market_id: i64,
        option_label: &str,
        shares: f64,
        interest_bps: Option<i64>,
        duration_seconds: Option<i64>,
    ) -> AppResult<LoanOfferReceipt> {
        if !self.config.loans.allow_share_loans {
            return Err(AppError::Conflict(
                "share loans are disabled by server policy".to_string(),
            ));
        }
        self.ensure_loan_parties(
            guild_id,
            lender_user_id,
            lender_display_name,
            borrower_user_id,
            borrower_display_name,
        )
        .await?;
        if shares < self.config.transfers.min_share_transfer {
            return Err(AppError::Validation("loan share principal is too small".to_string()));
        }

        let interest_bps = self.normalize_interest_bps(interest_bps)?;
        let duration_seconds = self.normalize_duration(duration_seconds)?;
        let (_, option) = self.load_market_and_option(guild_id, market_id, option_label).await?;
        let lender_shares = self.position_shares(market_id, option.id, lender_user_id).await?;
        if lender_shares + 1e-9 < shares {
            return Err(AppError::Conflict(
                "lender does not currently have enough shares".to_string(),
            ));
        }

        let repayment_shares = shares * (1.0 + (interest_bps as f64 / 10_000.0));
        let (expires_at, due_at) = self.loan_timestamps(duration_seconds);
        let result = sqlx::query(
            "INSERT INTO loans
             (guild_id, asset_type, market_id, option_id, lender_discord_user_id, borrower_discord_user_id, principal_mana, principal_shares, repayment_mana, repayment_shares, interest_bps, due_at, status, created_at, accepted_at, responded_at, closed_at, expires_at)
             VALUES (?1, 'shares', ?2, ?3, ?4, ?5, NULL, ?6, NULL, ?7, ?8, ?9, 'pending', ?10, NULL, NULL, NULL, ?11)",
        )
        .bind(guild_id)
        .bind(market_id)
        .bind(option.id)
        .bind(lender_user_id)
        .bind(borrower_user_id)
        .bind(shares)
        .bind(repayment_shares)
        .bind(interest_bps)
        .bind(due_at.to_rfc3339())
        .bind(now_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(LoanOfferReceipt {
            loan_id: result.last_insert_rowid(),
            borrower_display_name: borrower_display_name.to_string(),
            asset_type: "shares".to_string(),
            market_id: Some(market_id),
            option_label: Some(option.label),
            principal_mana: None,
            principal_shares: Some(shares),
            repayment_mana: None,
            repayment_shares: Some(repayment_shares),
            interest_bps,
            expires_at,
            due_at,
        })
    }

    pub async fn incoming_loans(
        &self,
        guild_id: &str,
        borrower_user_id: &str,
    ) -> AppResult<Vec<IncomingLoanSummary>> {
        self.expire_pending_loans().await?;
        let rows = sqlx::query(
            "SELECT
                l.id,
                l.asset_type,
                l.market_id,
                l.principal_mana,
                l.principal_shares,
                l.repayment_mana,
                l.repayment_shares,
                l.due_at,
                l.expires_at,
                l.lender_discord_user_id,
                ga.display_name AS lender_display_name,
                m.question,
                mo.label AS option_label
             FROM loans l
             LEFT JOIN guild_accounts ga
               ON ga.guild_id = l.guild_id
              AND ga.discord_user_id = l.lender_discord_user_id
             LEFT JOIN markets m ON m.id = l.market_id
             LEFT JOIN market_options mo ON mo.id = l.option_id
             WHERE l.guild_id = ?1
               AND l.borrower_discord_user_id = ?2
               AND l.status = 'pending'
             ORDER BY l.expires_at ASC, l.id ASC",
        )
        .bind(guild_id)
        .bind(borrower_user_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(IncomingLoanSummary {
                    loan_id: row.get("id"),
                    lender_display_name: row
                        .get::<Option<String>, _>("lender_display_name")
                        .unwrap_or_else(|| row.get::<String, _>("lender_discord_user_id")),
                    asset_type: row.get("asset_type"),
                    market_id: row.get("market_id"),
                    market_question: row.get("question"),
                    option_label: row.get("option_label"),
                    principal_mana: row.get("principal_mana"),
                    principal_shares: row.get("principal_shares"),
                    repayment_mana: row.get("repayment_mana"),
                    repayment_shares: row.get("repayment_shares"),
                    due_at: parse_rfc3339_utc(&row.get::<String, _>("due_at"))?,
                    expires_at: parse_rfc3339_utc(&row.get::<String, _>("expires_at"))?,
                })
            })
            .collect()
    }

    pub async fn autocomplete_incoming_loans(
        &self,
        guild_id: &str,
        borrower_user_id: &str,
        partial: &str,
        limit: i64,
    ) -> AppResult<Vec<serenity::AutocompleteChoice>> {
        let like = format!("%{}%", partial.trim());
        let rows = sqlx::query(
            "SELECT
                l.id,
                l.asset_type,
                l.principal_mana,
                l.principal_shares,
                ga.display_name AS lender_display_name,
                l.lender_discord_user_id
             FROM loans l
             LEFT JOIN guild_accounts ga
               ON ga.guild_id = l.guild_id
              AND ga.discord_user_id = l.lender_discord_user_id
             WHERE l.guild_id = ?1
               AND l.borrower_discord_user_id = ?2
               AND l.status = 'pending'
               AND (?3 = '' OR CAST(l.id AS TEXT) LIKE ?4 OR l.asset_type LIKE ?4)
             ORDER BY l.expires_at ASC, l.id ASC
             LIMIT ?5",
        )
        .bind(guild_id)
        .bind(borrower_user_id)
        .bind(partial.trim())
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let lender_name = row
                    .get::<Option<String>, _>("lender_display_name")
                    .unwrap_or_else(|| row.get::<String, _>("lender_discord_user_id"));
                let quantity = if row.get::<String, _>("asset_type") == "money" {
                    row.get::<Option<i64>, _>("principal_mana")
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "?".to_string())
                } else {
                    row.get::<Option<f64>, _>("principal_shares")
                        .map(|value| format!("{value:.2} sh"))
                        .unwrap_or_else(|| "?".to_string())
                };
                serenity::AutocompleteChoice::new(
                    format!("#{} {} from {}", row.get::<i64, _>("id"), quantity, lender_name),
                    row.get::<i64, _>("id").to_string(),
                )
            })
            .collect())
    }

    pub async fn accept_loan(
        &self,
        guild_id: &str,
        loan_id: i64,
        borrower_user_id: &str,
        borrower_display_name: &str,
    ) -> AppResult<LoanActionReceipt> {
        self.ensure_account(guild_id, borrower_user_id, borrower_display_name)
            .await?;
        self.expire_pending_loans().await?;
        let loan = self.load_pending_loan(guild_id, loan_id).await?;
        if loan.borrower_discord_user_id != borrower_user_id {
            return Err(AppError::Conflict(
                "that loan offer is not addressed to you".to_string(),
            ));
        }

        let now = Utc::now();
        if now >= parse_rfc3339_utc(&loan.expires_at)? {
            return Err(AppError::Conflict(
                "that loan offer already expired".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        match loan.asset_type.as_str() {
            "money" => {
                let principal = loan.principal_mana.unwrap_or(0);
                let lender_balance = self.balance(guild_id, &loan.lender_discord_user_id).await?;
                if lender_balance < principal {
                    return Err(AppError::Conflict(
                        "lender no longer has enough balance".to_string(),
                    ));
                }
                self.adjust_balance(&mut tx, guild_id, &loan.lender_discord_user_id, -principal)
                    .await?;
                self.adjust_balance(&mut tx, guild_id, borrower_user_id, principal)
                    .await?;
                self.insert_money_event(
                    &mut tx,
                    guild_id,
                    &loan.lender_discord_user_id,
                    -principal,
                    "loan_issued_money",
                    None,
                    None,
                )
                .await?;
                self.insert_money_event(
                    &mut tx,
                    guild_id,
                    borrower_user_id,
                    principal,
                    "loan_received_money",
                    None,
                    None,
                )
                .await?;
            }
            "shares" => {
                let market_id = loan.market_id.ok_or_else(|| {
                    AppError::External("share loan is missing market id".to_string())
                })?;
                let option_id = loan.option_id.ok_or_else(|| {
                    AppError::External("share loan is missing option id".to_string())
                })?;
                let principal = loan.principal_shares.unwrap_or(0.0);
                let lender_shares = self
                    .position_shares(market_id, option_id, &loan.lender_discord_user_id)
                    .await?;
                if lender_shares + 1e-9 < principal {
                    return Err(AppError::Conflict(
                        "lender no longer has enough shares".to_string(),
                    ));
                }
                self.upsert_position(
                    &mut tx,
                    market_id,
                    option_id,
                    &loan.lender_discord_user_id,
                    -principal,
                    0,
                    0,
                )
                .await?;
                self.upsert_position(
                    &mut tx,
                    market_id,
                    option_id,
                    borrower_user_id,
                    principal,
                    0,
                    0,
                )
                .await?;
                self.insert_share_event(
                    &mut tx,
                    guild_id,
                    &loan.lender_discord_user_id,
                    market_id,
                    option_id,
                    -principal,
                    "loan_issued_shares",
                )
                .await?;
                self.insert_share_event(
                    &mut tx,
                    guild_id,
                    borrower_user_id,
                    market_id,
                    option_id,
                    principal,
                    "loan_received_shares",
                )
                .await?;
            }
            _ => return Err(AppError::External("unsupported loan asset type".to_string())),
        }

        sqlx::query(
            "UPDATE loans
             SET status = 'active', accepted_at = ?2, responded_at = ?2
             WHERE id = ?1",
        )
        .bind(loan.id)
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(LoanActionReceipt {
            loan_id: loan.id,
            counterparty_display_name: self
                .display_name(guild_id, &loan.lender_discord_user_id)
                .await?,
            asset_type: loan.asset_type,
            market_id: loan.market_id,
            option_label: self.option_label(loan.option_id).await?,
            principal_mana: loan.principal_mana,
            principal_shares: loan.principal_shares,
            repayment_mana: loan.repayment_mana,
            repayment_shares: loan.repayment_shares,
            status: "active".to_string(),
            due_at: parse_rfc3339_utc(&loan.due_at)?,
        })
    }

    pub async fn decline_loan(
        &self,
        guild_id: &str,
        loan_id: i64,
        borrower_user_id: &str,
    ) -> AppResult<LoanActionReceipt> {
        self.expire_pending_loans().await?;
        let loan = self.load_pending_loan(guild_id, loan_id).await?;
        if loan.borrower_discord_user_id != borrower_user_id {
            return Err(AppError::Conflict(
                "that loan offer is not addressed to you".to_string(),
            ));
        }

        sqlx::query(
            "UPDATE loans
             SET status = 'declined', responded_at = ?2
             WHERE id = ?1",
        )
        .bind(loan.id)
        .bind(now_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(LoanActionReceipt {
            loan_id: loan.id,
            counterparty_display_name: self
                .display_name(guild_id, &loan.lender_discord_user_id)
                .await?,
            asset_type: loan.asset_type,
            market_id: loan.market_id,
            option_label: self.option_label(loan.option_id).await?,
            principal_mana: loan.principal_mana,
            principal_shares: loan.principal_shares,
            repayment_mana: loan.repayment_mana,
            repayment_shares: loan.repayment_shares,
            status: "declined".to_string(),
            due_at: parse_rfc3339_utc(&loan.due_at)?,
        })
    }

    pub async fn loan_status(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> AppResult<Vec<LoanStatusLine>> {
        self.mark_overdue_loans().await?;
        let rows = sqlx::query(
            "SELECT
                l.id,
                l.asset_type,
                l.market_id,
                l.option_id,
                l.principal_mana,
                l.principal_shares,
                l.repayment_mana,
                l.repayment_shares,
                l.due_at,
                l.status,
                l.lender_discord_user_id,
                l.borrower_discord_user_id,
                lender.display_name AS lender_name,
                borrower.display_name AS borrower_name,
                COALESCE(SUM(r.amount_mana), 0) AS repaid_mana,
                COALESCE(SUM(r.amount_shares), 0.0) AS repaid_shares
             FROM loans l
             LEFT JOIN guild_accounts lender
               ON lender.guild_id = l.guild_id
              AND lender.discord_user_id = l.lender_discord_user_id
             LEFT JOIN guild_accounts borrower
               ON borrower.guild_id = l.guild_id
              AND borrower.discord_user_id = l.borrower_discord_user_id
             LEFT JOIN loan_repayments r ON r.loan_id = l.id
             WHERE l.guild_id = ?1
               AND (l.lender_discord_user_id = ?2 OR l.borrower_discord_user_id = ?2)
             GROUP BY l.id
             ORDER BY
                CASE l.status
                    WHEN 'active' THEN 0
                    WHEN 'pending' THEN 1
                    WHEN 'defaulted' THEN 2
                    ELSE 3
                END,
                l.due_at ASC,
                l.id ASC",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let lender_id = row.get::<String, _>("lender_discord_user_id");
                let borrower_id = row.get::<String, _>("borrower_discord_user_id");
                let direction = if lender_id == user_id {
                    "lent".to_string()
                } else {
                    "borrowed".to_string()
                };
                let counterparty = if lender_id == user_id {
                    row.get::<Option<String>, _>("borrower_name")
                        .unwrap_or(borrower_id)
                } else {
                    row.get::<Option<String>, _>("lender_name")
                        .unwrap_or(lender_id)
                };
                Ok(LoanStatusLine {
                    loan_id: row.get("id"),
                    direction,
                    counterparty_display_name: counterparty,
                    asset_type: row.get("asset_type"),
                    market_id: row.get("market_id"),
                    option_label: None,
                    principal_mana: row.get("principal_mana"),
                    principal_shares: row.get("principal_shares"),
                    repayment_mana: row.get("repayment_mana"),
                    repayment_shares: row.get("repayment_shares"),
                    repaid_mana: row.get("repaid_mana"),
                    repaid_shares: row.get("repaid_shares"),
                    status: row.get("status"),
                    due_at: parse_rfc3339_utc(&row.get::<String, _>("due_at"))?,
                })
            })
            .collect()
    }

    pub async fn repay_loan(
        &self,
        guild_id: &str,
        loan_id: i64,
        borrower_user_id: &str,
        amount_mana: Option<i64>,
        amount_shares: Option<f64>,
    ) -> AppResult<RepaymentReceipt> {
        let loan = self.load_active_loan(guild_id, loan_id).await?;
        if loan.borrower_discord_user_id != borrower_user_id {
            return Err(AppError::Conflict(
                "only the borrower can repay that loan".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        let (repaid_mana, repaid_shares) = self.repaid_totals_tx(&mut tx, loan.id).await?;

        let (paid_mana, paid_shares, remaining_mana, remaining_shares) = match loan.asset_type.as_str() {
            "money" => {
                let due = loan.repayment_mana.unwrap_or(0);
                let payment = amount_mana.ok_or_else(|| {
                    AppError::Validation("money repayment requires `amount_mana`".to_string())
                })?;
                if payment <= 0 {
                    return Err(AppError::Validation("repayment must be positive".to_string()));
                }
                let remaining = (due - repaid_mana).max(0);
                if !self.config.loans.allow_partial_repayment && payment < remaining {
                    return Err(AppError::Conflict(
                        "partial repayment is disabled by server policy".to_string(),
                    ));
                }
                let borrower_balance = self.balance(guild_id, borrower_user_id).await?;
                if borrower_balance < payment {
                    return Err(AppError::Conflict(
                        "borrower does not have enough balance".to_string(),
                    ));
                }
                self.adjust_balance(&mut tx, guild_id, borrower_user_id, -payment).await?;
                self.adjust_balance(&mut tx, guild_id, &loan.lender_discord_user_id, payment)
                    .await?;
                self.insert_money_event(
                    &mut tx,
                    guild_id,
                    borrower_user_id,
                    -payment,
                    "loan_repayment_sent",
                    None,
                    None,
                )
                .await?;
                self.insert_money_event(
                    &mut tx,
                    guild_id,
                    &loan.lender_discord_user_id,
                    payment,
                    "loan_repayment_received",
                    None,
                    None,
                )
                .await?;
                sqlx::query(
                    "INSERT INTO loan_repayments
                     (loan_id, guild_id, payer_discord_user_id, amount_mana, amount_shares, created_at)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
                )
                .bind(loan.id)
                .bind(guild_id)
                .bind(borrower_user_id)
                .bind(payment)
                .bind(now_rfc3339())
                .execute(&mut *tx)
                .await?;
                (Some(payment), None, Some((remaining - payment).max(0)), None)
            }
            "shares" => {
                let market_id = loan.market_id.unwrap_or(0);
                let option_id = loan.option_id.unwrap_or(0);
                let due = loan.repayment_shares.unwrap_or(0.0);
                let payment = amount_shares.ok_or_else(|| {
                    AppError::Validation("share repayment requires `shares`".to_string())
                })?;
                if payment <= 0.0 {
                    return Err(AppError::Validation("repayment must be positive".to_string()));
                }
                let remaining = (due - repaid_shares).max(0.0);
                if !self.config.loans.allow_partial_repayment && payment + 1e-9 < remaining {
                    return Err(AppError::Conflict(
                        "partial repayment is disabled by server policy".to_string(),
                    ));
                }
                let borrower_shares = self.position_shares(market_id, option_id, borrower_user_id).await?;
                if borrower_shares + 1e-9 < payment {
                    return Err(AppError::Conflict(
                        "borrower does not have enough shares".to_string(),
                    ));
                }
                self.upsert_position(
                    &mut tx,
                    market_id,
                    option_id,
                    borrower_user_id,
                    -payment,
                    0,
                    0,
                )
                .await?;
                self.upsert_position(
                    &mut tx,
                    market_id,
                    option_id,
                    &loan.lender_discord_user_id,
                    payment,
                    0,
                    0,
                )
                .await?;
                self.insert_share_event(
                    &mut tx,
                    guild_id,
                    borrower_user_id,
                    market_id,
                    option_id,
                    -payment,
                    "loan_repayment_sent_shares",
                )
                .await?;
                self.insert_share_event(
                    &mut tx,
                    guild_id,
                    &loan.lender_discord_user_id,
                    market_id,
                    option_id,
                    payment,
                    "loan_repayment_received_shares",
                )
                .await?;
                sqlx::query(
                    "INSERT INTO loan_repayments
                     (loan_id, guild_id, payer_discord_user_id, amount_mana, amount_shares, created_at)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                )
                .bind(loan.id)
                .bind(guild_id)
                .bind(borrower_user_id)
                .bind(payment)
                .bind(now_rfc3339())
                .execute(&mut *tx)
                .await?;
                (None, Some(payment), None, Some((remaining - payment).max(0.0)))
            }
            _ => return Err(AppError::External("unsupported loan asset type".to_string())),
        };

        let (new_repaid_mana, new_repaid_shares) = self.repaid_totals_tx(&mut tx, loan.id).await?;
        let closed = (loan.repayment_mana.is_some()
            && new_repaid_mana >= loan.repayment_mana.unwrap_or(0))
            || (loan.repayment_shares.is_some()
                && new_repaid_shares + 1e-9 >= loan.repayment_shares.unwrap_or(0.0));
        let status = if closed { "repaid" } else { "active" };
        if closed {
            sqlx::query(
                "UPDATE loans
                 SET status = 'repaid', closed_at = ?2
                 WHERE id = ?1",
            )
            .bind(loan.id)
            .bind(now_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        Ok(RepaymentReceipt {
            loan_id: loan.id,
            asset_type: loan.asset_type,
            paid_mana,
            paid_shares,
            remaining_mana,
            remaining_shares,
            status: status.to_string(),
        })
    }

    pub async fn expire_pending_loans(&self) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE loans
             SET status = 'expired', responded_at = ?1
             WHERE status = 'pending' AND expires_at <= ?1",
        )
        .bind(now_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn mark_overdue_loans(&self) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE loans
             SET status = 'defaulted', closed_at = ?1
             WHERE status = 'active' AND due_at <= ?1",
        )
        .bind(now_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn ensure_loan_parties(
        &self,
        guild_id: &str,
        lender_user_id: &str,
        lender_display_name: &str,
        borrower_user_id: &str,
        borrower_display_name: &str,
    ) -> AppResult<()> {
        if lender_user_id == borrower_user_id {
            return Err(AppError::Validation(
                "you cannot create a loan with yourself".to_string(),
            ));
        }
        self.ensure_account(guild_id, lender_user_id, lender_display_name)
            .await?;
        self.ensure_account(guild_id, borrower_user_id, borrower_display_name)
            .await?;
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

    async fn load_market_and_option(
        &self,
        guild_id: &str,
        market_id: i64,
        option_label: &str,
    ) -> AppResult<(MarketRecord, MarketOptionRecord)> {
        let market = sqlx::query_as::<_, MarketRecord>(
            "SELECT id, guild_id, channel_id, creator_discord_user_id, question, status, market_type, liquidity_b, close_time, resolved_option_id, created_at, resolved_at, updated_at, external_source, external_id, external_url, external_slug, last_external_sync_at, external_status, external_resolution
             FROM markets
             WHERE id = ?1 AND guild_id = ?2",
        )
        .bind(market_id)
        .bind(guild_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("market {market_id} was not found in this server")))?;
        let option = sqlx::query_as::<_, MarketOptionRecord>(
            "SELECT id, market_id, label, shares_outstanding, sort_order, external_option_id, external_probability
             FROM market_options
             WHERE market_id = ?1",
        )
        .bind(market_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .find(|option| option.label.eq_ignore_ascii_case(option_label))
        .ok_or_else(|| AppError::NotFound(format!("option `{option_label}` was not found")))?;
        Ok((market, option))
    }

    async fn position_shares(&self, market_id: i64, option_id: i64, user_id: &str) -> AppResult<f64> {
        let row = sqlx::query(
            "SELECT shares
             FROM positions
             WHERE market_id = ?1 AND option_id = ?2 AND discord_user_id = ?3",
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

    async fn insert_money_event(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        user_id: &str,
        amount_mana: i64,
        reason: &str,
        market_id: Option<i64>,
        option_id: Option<i64>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO economy_events
             (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
             VALUES (?1, ?2, ?3, ?4, 'money', ?5, NULL, ?6, NULL, ?7)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(market_id)
        .bind(option_id)
        .bind(amount_mana)
        .bind(reason)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_share_event(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        guild_id: &str,
        user_id: &str,
        market_id: i64,
        option_id: i64,
        amount_shares: f64,
        reason: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO economy_events
             (guild_id, discord_user_id, related_market_id, related_option_id, asset_type, amount_mana, amount_shares, reason, note, created_at)
             VALUES (?1, ?2, ?3, ?4, 'shares', NULL, ?5, ?6, NULL, ?7)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(market_id)
        .bind(option_id)
        .bind(amount_shares)
        .bind(reason)
        .bind(now_rfc3339())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn load_pending_loan(&self, guild_id: &str, loan_id: i64) -> AppResult<LoanRecord> {
        let loan = sqlx::query_as::<_, LoanRecord>(
            "SELECT id, guild_id, asset_type, market_id, option_id, lender_discord_user_id, borrower_discord_user_id, principal_mana, principal_shares, repayment_mana, repayment_shares, interest_bps, due_at, status, expires_at
             FROM loans
             WHERE guild_id = ?1 AND id = ?2",
        )
        .bind(guild_id)
        .bind(loan_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("loan offer {loan_id} was not found")))?;
        if loan.status != "pending" {
            return Err(AppError::Conflict(format!(
                "loan offer #{loan_id} is already {}",
                loan.status
            )));
        }
        Ok(loan)
    }

    async fn load_active_loan(&self, guild_id: &str, loan_id: i64) -> AppResult<LoanRecord> {
        let loan = sqlx::query_as::<_, LoanRecord>(
            "SELECT id, guild_id, asset_type, market_id, option_id, lender_discord_user_id, borrower_discord_user_id, principal_mana, principal_shares, repayment_mana, repayment_shares, interest_bps, due_at, status, expires_at
             FROM loans
             WHERE guild_id = ?1 AND id = ?2",
        )
        .bind(guild_id)
        .bind(loan_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("loan {loan_id} was not found")))?;
        if loan.status != "active" {
            return Err(AppError::Conflict(format!(
                "loan #{loan_id} is not active; current status is {}",
                loan.status
            )));
        }
        Ok(loan)
    }

    async fn repaid_totals_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        loan_id: i64,
    ) -> AppResult<(i64, f64)> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(amount_mana), 0) AS repaid_mana, COALESCE(SUM(amount_shares), 0.0) AS repaid_shares
             FROM loan_repayments
             WHERE loan_id = ?1",
        )
        .bind(loan_id)
        .fetch_one(&mut **tx)
        .await?;
        Ok((row.get("repaid_mana"), row.get("repaid_shares")))
    }

    async fn display_name(&self, guild_id: &str, user_id: &str) -> AppResult<String> {
        let row = sqlx::query(
            "SELECT display_name
             FROM guild_accounts
             WHERE guild_id = ?1 AND discord_user_id = ?2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row
            .and_then(|row| row.get::<Option<String>, _>("display_name"))
            .unwrap_or_else(|| user_id.to_string()))
    }

    async fn option_label(&self, option_id: Option<i64>) -> AppResult<Option<String>> {
        let Some(option_id) = option_id else {
            return Ok(None);
        };
        let row = sqlx::query("SELECT label FROM market_options WHERE id = ?1")
            .bind(option_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get("label")))
    }

    fn normalize_interest_bps(&self, interest_bps: Option<i64>) -> AppResult<i64> {
        let value = interest_bps.unwrap_or(self.config.loans.default_interest_bps);
        if !self.config.loans.allow_interest && value > 0 {
            return Err(AppError::Conflict(
                "interest is disabled by server policy".to_string(),
            ));
        }
        if value < 0 || value > self.config.loans.max_interest_bps {
            return Err(AppError::Validation(format!(
                "interest_bps must be between 0 and {}",
                self.config.loans.max_interest_bps
            )));
        }
        Ok(value)
    }

    fn normalize_duration(&self, duration_seconds: Option<i64>) -> AppResult<i64> {
        let value = duration_seconds.unwrap_or(self.config.loans.default_duration_seconds);
        if value <= 0 || value > self.config.loans.max_duration_seconds {
            return Err(AppError::Validation(format!(
                "loan duration must be between 1 and {} seconds",
                self.config.loans.max_duration_seconds
            )));
        }
        Ok(value)
    }

    fn loan_timestamps(&self, duration_seconds: i64) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(self.config.share_offer_expiration_seconds.max(30));
        let due_at = now + Duration::seconds(duration_seconds);
        (expires_at, due_at)
    }
}

fn parse_rfc3339_utc(value: &str) -> AppResult<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::Other(anyhow::anyhow!("invalid RFC3339 timestamp: {value}")))
}
