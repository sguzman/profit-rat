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

#[derive(Clone, Debug)]
pub struct AutoRepaySummary {
    pub repaid_loans: u64,
    pub total_paid_mana: i64,
    pub loan_ids: Vec<i64>,
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

    #[instrument(
        skip(self),
        fields(guild_id, sender_user_id, recipient_user_id, amount_mana)
    )]
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
        self.adjust_balance(&mut tx, guild_id, sender_user_id, -amount_mana)
            .await?;
        self.adjust_balance(&mut tx, guild_id, recipient_user_id, amount_mana)
            .await?;
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

    #[instrument(
        skip(self),
        fields(guild_id, sender_user_id, recipient_user_id, market_id, shares)
    )]
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

        let (market, option) = self
            .load_market_and_option(guild_id, market_id, option_label)
            .await?;
        let sender_shares = self
            .position_shares(market_id, option.id, sender_user_id)
            .await?;
        if sender_shares + 1e-9 < shares {
            return Err(AppError::Conflict(
                "you do not have enough shares for that donation".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        self.upsert_position(&mut tx, market_id, option.id, sender_user_id, -shares, 0, 0)
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
    #[instrument(
        skip(self),
        fields(guild_id, lender_user_id, borrower_user_id, principal_mana)
    )]
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
            return Err(AppError::Validation(
                "loan principal is too small".to_string(),
            ));
        }

        let interest_bps = self.normalize_interest_bps(interest_bps)?;
        let duration_seconds = self.normalize_duration(duration_seconds)?;
        let lender_balance = self.balance(guild_id, lender_user_id).await?;
        if lender_balance < principal_mana {
            return Err(AppError::Conflict(
                "lender does not currently have enough balance".to_string(),
            ));
        }

        let repayment_mana = principal_mana + ((principal_mana * interest_bps + 9_999) / 10_000);
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
    #[instrument(
        skip(self),
        fields(guild_id, lender_user_id, borrower_user_id, market_id, shares)
    )]
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
            return Err(AppError::Validation(
                "loan share principal is too small".to_string(),
            ));
        }

        let interest_bps = self.normalize_interest_bps(interest_bps)?;
        let duration_seconds = self.normalize_duration(duration_seconds)?;
        let (_, option) = self
            .load_market_and_option(guild_id, market_id, option_label)
            .await?;
        let lender_shares = self
            .position_shares(market_id, option.id, lender_user_id)
            .await?;
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
                    format!(
                        "#{} {} from {}",
                        row.get::<i64, _>("id"),
                        quantity,
                        lender_name
                    ),
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
            _ => {
                return Err(AppError::External(
                    "unsupported loan asset type".to_string(),
                ));
            }
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

        let (paid_mana, paid_shares, remaining_mana, remaining_shares) = match loan
            .asset_type
            .as_str()
        {
            "money" => {
                let due = loan.repayment_mana.unwrap_or(0);
                let payment = amount_mana.ok_or_else(|| {
                    AppError::Validation("money repayment requires `amount_mana`".to_string())
                })?;
                if payment <= 0 {
                    return Err(AppError::Validation(
                        "repayment must be positive".to_string(),
                    ));
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
                self.adjust_balance(&mut tx, guild_id, borrower_user_id, -payment)
                    .await?;
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
                (
                    Some(payment),
                    None,
                    Some((remaining - payment).max(0)),
                    None,
                )
            }
            "shares" => {
                let market_id = loan.market_id.unwrap_or(0);
                let option_id = loan.option_id.unwrap_or(0);
                let due = loan.repayment_shares.unwrap_or(0.0);
                let payment = amount_shares.ok_or_else(|| {
                    AppError::Validation("share repayment requires `shares`".to_string())
                })?;
                if payment <= 0.0 {
                    return Err(AppError::Validation(
                        "repayment must be positive".to_string(),
                    ));
                }
                let remaining = (due - repaid_shares).max(0.0);
                if !self.config.loans.allow_partial_repayment && payment + 1e-9 < remaining {
                    return Err(AppError::Conflict(
                        "partial repayment is disabled by server policy".to_string(),
                    ));
                }
                let borrower_shares = self
                    .position_shares(market_id, option_id, borrower_user_id)
                    .await?;
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
                (
                    None,
                    Some(payment),
                    None,
                    Some((remaining - payment).max(0.0)),
                )
            }
            _ => {
                return Err(AppError::External(
                    "unsupported loan asset type".to_string(),
                ));
            }
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

    #[instrument(skip(self), fields(guild_id, borrower_user_id, horizon_seconds))]
    pub async fn auto_repay_due_money_loans(
        &self,
        guild_id: &str,
        borrower_user_id: &str,
        horizon_seconds: i64,
    ) -> AppResult<AutoRepaySummary> {
        let horizon = Utc::now() + Duration::seconds(horizon_seconds.max(0));
        let rows = sqlx::query(
            "SELECT id
             FROM loans
             WHERE guild_id = ?1
               AND borrower_discord_user_id = ?2
               AND asset_type = 'money'
               AND status = 'active'
               AND due_at <= ?3
             ORDER BY due_at ASC, id ASC",
        )
        .bind(guild_id)
        .bind(borrower_user_id)
        .bind(horizon.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut summary = AutoRepaySummary {
            repaid_loans: 0,
            total_paid_mana: 0,
            loan_ids: Vec::new(),
        };
        for row in rows {
            let loan_id = row.get::<i64, _>("id");
            let loan = self.load_active_loan(guild_id, loan_id).await?;
            if loan.asset_type != "money" {
                continue;
            }

            let already_repaid = self.repaid_totals(loan.id).await?.0;
            let remaining = (loan.repayment_mana.unwrap_or(0) - already_repaid).max(0);
            if remaining <= 0 {
                continue;
            }

            let available_balance = self.balance(guild_id, borrower_user_id).await?;
            if available_balance <= 0 {
                continue;
            }

            let payment = if self.config.loans.allow_partial_repayment {
                available_balance.min(remaining)
            } else if available_balance >= remaining {
                remaining
            } else {
                0
            };
            if payment <= 0 {
                continue;
            }

            let receipt = self
                .repay_loan(guild_id, loan_id, borrower_user_id, Some(payment), None)
                .await?;
            summary.repaid_loans += 1;
            summary.total_paid_mana += receipt.paid_mana.unwrap_or(0);
            summary.loan_ids.push(loan_id);
        }

        Ok(summary)
    }

    #[instrument(skip(self), fields(guild_id, borrower_user_id))]
    pub async fn auto_repay_defaulted_money_loans(
        &self,
        guild_id: &str,
        borrower_user_id: &str,
    ) -> AppResult<AutoRepaySummary> {
        let rows = sqlx::query(
            "SELECT id
             FROM loans
             WHERE guild_id = ?1
               AND borrower_discord_user_id = ?2
               AND asset_type = 'money'
               AND status = 'defaulted'
             ORDER BY due_at ASC, id ASC",
        )
        .bind(guild_id)
        .bind(borrower_user_id)
        .fetch_all(&self.pool)
        .await?;

        let mut summary = AutoRepaySummary {
            repaid_loans: 0,
            total_paid_mana: 0,
            loan_ids: Vec::new(),
        };
        for row in rows {
            let loan_id = row.get::<i64, _>("id");
            let loan = self.load_loan_any_status(guild_id, loan_id).await?;
            if loan.asset_type != "money" || loan.status != "defaulted" {
                continue;
            }

            let already_repaid = self.repaid_totals(loan.id).await?.0;
            let remaining = (loan.repayment_mana.unwrap_or(0) - already_repaid).max(0);
            if remaining <= 0 {
                continue;
            }

            let available_balance = self.balance(guild_id, borrower_user_id).await?;
            if available_balance <= 0 {
                continue;
            }

            let payment = if self.config.loans.allow_partial_repayment {
                available_balance.min(remaining)
            } else if available_balance >= remaining {
                remaining
            } else {
                0
            };
            if payment <= 0 {
                continue;
            }

            let mut tx = self.pool.begin().await?;
            self.adjust_balance(&mut tx, guild_id, borrower_user_id, -payment)
                .await?;
            self.adjust_balance(&mut tx, guild_id, &loan.lender_discord_user_id, payment)
                .await?;
            self.insert_money_event(
                &mut tx,
                guild_id,
                borrower_user_id,
                -payment,
                "loan_repayment_sent_after_default",
                None,
                None,
            )
            .await?;
            self.insert_money_event(
                &mut tx,
                guild_id,
                &loan.lender_discord_user_id,
                payment,
                "loan_repayment_received_after_default",
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

            let new_repaid = already_repaid + payment;
            if new_repaid >= loan.repayment_mana.unwrap_or(0) {
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

            summary.repaid_loans += 1;
            summary.total_paid_mana += payment;
            summary.loan_ids.push(loan_id);
        }

        Ok(summary)
    }

    #[instrument(skip(self), fields(guild_id, borrower_user_id))]
    pub async fn auto_accept_eligible_loans(
        &self,
        guild_id: &str,
        borrower_user_id: &str,
        borrower_display_name: &str,
        max_interest_bps: i64,
        min_duration_seconds: i64,
    ) -> AppResult<u64> {
        self.expire_pending_loans().await?;
        let rows = sqlx::query(
            "SELECT id, asset_type, interest_bps, created_at, due_at
             FROM loans
             WHERE guild_id = ?1
               AND borrower_discord_user_id = ?2
               AND status = 'pending'
             ORDER BY created_at ASC, id ASC",
        )
        .bind(guild_id)
        .bind(borrower_user_id)
        .fetch_all(&self.pool)
        .await?;

        let mut accepted = 0;
        for row in rows {
            let loan_id = row.get::<i64, _>("id");
            let asset_type = row.get::<String, _>("asset_type");
            let interest_bps = row.get::<i64, _>("interest_bps");
            let created_at = parse_rfc3339_utc(&row.get::<String, _>("created_at"))?;
            let due_at = parse_rfc3339_utc(&row.get::<String, _>("due_at"))?;
            let duration_seconds = (due_at - created_at).num_seconds();

            if asset_type != "money" || interest_bps > max_interest_bps || duration_seconds < min_duration_seconds {
                continue;
            }

            if self
                .accept_loan(guild_id, loan_id, borrower_user_id, borrower_display_name)
                .await
                .is_ok()
            {
                accepted += 1;
            }
        }

        Ok(accepted)
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

    async fn position_shares(
        &self,
        market_id: i64,
        option_id: i64,
        user_id: &str,
    ) -> AppResult<f64> {
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

    async fn load_loan_any_status(&self, guild_id: &str, loan_id: i64) -> AppResult<LoanRecord> {
        sqlx::query_as::<_, LoanRecord>(
            "SELECT id, guild_id, asset_type, market_id, option_id, lender_discord_user_id, borrower_discord_user_id, principal_mana, principal_shares, repayment_mana, repayment_shares, interest_bps, due_at, status, expires_at
             FROM loans
             WHERE guild_id = ?1 AND id = ?2",
        )
        .bind(guild_id)
        .bind(loan_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("loan {loan_id} was not found")))
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

    async fn repaid_totals(&self, loan_id: i64) -> AppResult<(i64, f64)> {
        let row = sqlx::query(
            "SELECT
                COALESCE(SUM(amount_mana), 0) AS repaid_mana,
                COALESCE(SUM(amount_shares), 0.0) AS repaid_shares
             FROM loan_repayments
             WHERE loan_id = ?1",
        )
        .bind(loan_id)
        .fetch_one(&self.pool)
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

    fn loan_timestamps(
        &self,
        duration_seconds: i64,
    ) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
        let now = Utc::now();
        let expires_at =
            now + Duration::seconds(self.config.share_offer_expiration_seconds.max(30));
        let due_at = now + Duration::seconds(duration_seconds);
        (expires_at, due_at)
    }
}

fn parse_rfc3339_utc(value: &str) -> AppResult<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::Other(anyhow::anyhow!("invalid RFC3339 timestamp: {value}")))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::config::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };
    use crate::db;
    use crate::db::now_rfc3339;

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
            starting_balance: 10_000,
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
                starting_balance: 10_000,
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
    async fn auto_repay_due_money_loans_reduces_bot_debt() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(test_config(cache_dir.clone()));
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::SocialService::new(config.clone(), pool);

        let offer = service
            .offer_loan_money(
                "guild-a",
                "lender",
                "Lender",
                "bot",
                "Profit Rat",
                1_000,
                Some(500),
                Some(3_600),
            )
            .await
            .expect("offer");
        service
            .accept_loan("guild-a", offer.loan_id, "bot", "Profit Rat")
            .await
            .expect("accept");

        let summary = service
            .auto_repay_due_money_loans("guild-a", "bot", 4_000)
            .await
            .expect("auto repay");

        assert_eq!(summary.repaid_loans, 1);
        assert!(summary.total_paid_mana >= 1_050);

        let lender_view = service
            .loan_status("guild-a", "lender")
            .await
            .expect("loan status");
        assert_eq!(lender_view.len(), 1);
        assert_eq!(lender_view[0].status, "repaid");
        assert_eq!(lender_view[0].repaid_mana, 1_050);
    }

    #[tokio::test]
    async fn auto_repay_defaulted_money_loans_restores_old_bot_debt() {
        let temp = tempdir().expect("tempdir");
        let cache_dir = temp.path().join(".cache");
        let config = Arc::new(test_config(cache_dir.clone()));
        config.ensure_runtime_dirs().expect("dirs");
        let pool = db::connect(&config).await.expect("pool");
        let service = super::SocialService::new(config.clone(), pool);

        let offer = service
            .offer_loan_money(
                "guild-a",
                "lender",
                "Lender",
                "bot",
                "Profit Rat",
                1_000,
                Some(500),
                Some(3_600),
            )
            .await
            .expect("offer");
        service
            .accept_loan("guild-a", offer.loan_id, "bot", "Profit Rat")
            .await
            .expect("accept");

        sqlx::query(
            "UPDATE loans
             SET status = 'defaulted', closed_at = ?2
             WHERE id = ?1",
        )
        .bind(offer.loan_id)
        .bind(now_rfc3339())
        .execute(&service.pool)
        .await
        .expect("mark defaulted");

        let summary = service
            .auto_repay_defaulted_money_loans("guild-a", "bot")
            .await
            .expect("auto repay defaulted");

        assert_eq!(summary.repaid_loans, 1);
        assert!(summary.total_paid_mana >= 1_050);

        let lender_view = service
            .loan_status("guild-a", "lender")
            .await
            .expect("loan status");
        assert_eq!(lender_view.len(), 1);
        assert_eq!(lender_view[0].status, "repaid");
        assert_eq!(lender_view[0].repaid_mana, 1_050);
    }
}
