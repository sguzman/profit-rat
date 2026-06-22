use poise::serenity_prelude as serenity;

use crate::bot::commands::market::{autocomplete_market_option, autocomplete_open_market, parse_market_id};
use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn donate_money(
    ctx: Context<'_>,
    #[description = "Recipient"] user: serenity::User,
    #[description = "Amount to send"] amount: i64,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let receipt = ctx
        .data()
        .services
        .social
        .donate_money(
            &guild_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            &user.id.to_string(),
            &display_name(&user),
            amount,
        )
        .await?;
    ui::send_embed(
        ctx,
        "🎁 Donation Sent",
        format!(
            "Sent {} to **{}**.\n**Your new balance:** {}",
            ui::money(ctx.data().config.as_ref(), receipt.amount_mana),
            receipt.recipient_display_name,
            ui::money(ctx.data().config.as_ref(), receipt.sender_balance_mana)
        ),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn donate_shares(
    ctx: Context<'_>,
    #[description = "Recipient"] user: serenity::User,
    #[description = "Market"]
    #[autocomplete = "autocomplete_open_market"]
    market: String,
    #[description = "Option"]
    #[autocomplete = "autocomplete_market_option"]
    option: String,
    #[description = "Shares to send"] shares: f64,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let receipt = ctx
        .data()
        .services
        .social
        .donate_shares(
            &guild_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            &user.id.to_string(),
            &display_name(&user),
            market_id,
            &option,
            shares,
        )
        .await?;
    ui::send_embed(
        ctx,
        "📦 Shares Donated",
        format!(
            "Sent {} of {} **{}** in market **#{}** to **{}**.",
            ui::shares(receipt.shares),
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            receipt.market_id,
            receipt.recipient_display_name
        ),
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn offer_loan_money(
    ctx: Context<'_>,
    #[description = "Borrower"] user: serenity::User,
    #[description = "Principal amount"] amount: i64,
    #[description = "Optional interest in bps"] interest_bps: Option<i64>,
    #[description = "Optional duration in hours"] duration_hours: Option<i64>,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let receipt = ctx
        .data()
        .services
        .social
        .offer_loan_money(
            &guild_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            &user.id.to_string(),
            &display_name(&user),
            amount,
            interest_bps,
            duration_hours.map(|hours| hours * 3600),
        )
        .await?;
    ui::send_embed(
        ctx,
        "🧾 Money Loan Offered",
        format!(
            "**Loan:** **#{}**\n**Borrower:** **{}**\n**Principal:** {}\n**Repayment:** {}\n**Interest:** {} bps\n**Offer expires:** {}\n**Due:** {}",
            receipt.loan_id,
            receipt.borrower_display_name,
            ui::money(ctx.data().config.as_ref(), receipt.principal_mana.unwrap_or(0)),
            ui::money(ctx.data().config.as_ref(), receipt.repayment_mana.unwrap_or(0)),
            receipt.interest_bps,
            ui::discord_timestamp(receipt.expires_at),
            ui::discord_timestamp(receipt.due_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(230, 126, 34),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn offer_loan_shares(
    ctx: Context<'_>,
    #[description = "Borrower"] user: serenity::User,
    #[description = "Market"]
    #[autocomplete = "autocomplete_open_market"]
    market: String,
    #[description = "Option"]
    #[autocomplete = "autocomplete_market_option"]
    option: String,
    #[description = "Shares to lend"] shares: f64,
    #[description = "Optional interest in bps"] interest_bps: Option<i64>,
    #[description = "Optional duration in hours"] duration_hours: Option<i64>,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let receipt = ctx
        .data()
        .services
        .social
        .offer_loan_shares(
            &guild_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            &user.id.to_string(),
            &display_name(&user),
            market_id,
            &option,
            shares,
            interest_bps,
            duration_hours.map(|hours| hours * 3600),
        )
        .await?;
    ui::send_embed(
        ctx,
        "🧾 Share Loan Offered",
        format!(
            "**Loan:** **#{}**\n**Borrower:** **{}**\n**Market:** **#{}**\n**Option:** {} **{}**\n**Principal:** {}\n**Repayment:** {}\n**Interest:** {} bps\n**Offer expires:** {}\n**Due:** {}",
            receipt.loan_id,
            receipt.borrower_display_name,
            receipt.market_id.unwrap_or(0),
            ui::option_emoji(receipt.option_label.as_deref().unwrap_or("?")),
            receipt.option_label.clone().unwrap_or_else(|| "?".to_string()),
            ui::shares(receipt.principal_shares.unwrap_or(0.0)),
            ui::shares(receipt.repayment_shares.unwrap_or(0.0)),
            receipt.interest_bps,
            ui::discord_timestamp(receipt.expires_at),
            ui::discord_timestamp(receipt.due_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(155, 89, 182),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn incoming_loans(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let loans = ctx
        .data()
        .services
        .social
        .incoming_loans(&guild_id, &ctx.author().id.to_string())
        .await?;
    if loans.is_empty() {
        ui::send_embed(
            ctx,
            "📨 Incoming Loans",
            "No pending loan offers are waiting on you right now.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = loans
        .into_iter()
        .map(|loan| {
            let principal = if loan.asset_type == "money" {
                ui::money(ctx.data().config.as_ref(), loan.principal_mana.unwrap_or(0))
            } else {
                ui::shares(loan.principal_shares.unwrap_or(0.0))
            };
            let repayment = if loan.asset_type == "money" {
                ui::money(ctx.data().config.as_ref(), loan.repayment_mana.unwrap_or(0))
            } else {
                ui::shares(loan.repayment_shares.unwrap_or(0.0))
            };
            format!(
                "**#{}** from **{}**\nPrincipal: {} • Repayment: {}\nExpires {} • Due {}",
                loan.loan_id,
                loan.lender_display_name,
                principal,
                repayment,
                ui::discord_timestamp(loan.expires_at),
                ui::discord_timestamp(loan.due_at)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "📨 Incoming Loans",
        body,
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn accept_loan(
    ctx: Context<'_>,
    #[description = "Incoming loan offer"]
    #[autocomplete = "autocomplete_incoming_loan"]
    loan: String,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let loan_id = parse_id(&loan, "loan offer")?;
    let receipt = ctx
        .data()
        .services
        .social
        .accept_loan(
            &guild_id,
            loan_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
        )
        .await?;
    ui::send_embed(
        ctx,
        "✅ Loan Accepted",
        format!(
            "**Loan:** **#{}**\n**Counterparty:** **{}**\n**Status:** **{}**\n**Due:** {}",
            receipt.loan_id,
            receipt.counterparty_display_name,
            receipt.status,
            ui::discord_timestamp(receipt.due_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn decline_loan(
    ctx: Context<'_>,
    #[description = "Incoming loan offer"]
    #[autocomplete = "autocomplete_incoming_loan"]
    loan: String,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let loan_id = parse_id(&loan, "loan offer")?;
    let receipt = ctx
        .data()
        .services
        .social
        .decline_loan(&guild_id, loan_id, &ctx.author().id.to_string())
        .await?;
    ui::send_embed(
        ctx,
        "🚫 Loan Declined",
        format!(
            "**Loan:** **#{}**\n**Counterparty:** **{}**\n**Status:** **{}**",
            receipt.loan_id,
            receipt.counterparty_display_name,
            receipt.status
        ),
        poise::serenity_prelude::Colour::from_rgb(231, 76, 60),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn loan_status(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let loans = ctx
        .data()
        .services
        .social
        .loan_status(&guild_id, &ctx.author().id.to_string())
        .await?;
    if loans.is_empty() {
        ui::send_embed(
            ctx,
            "📚 Loan Status",
            "You have no loans in this server economy right now.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = loans
        .into_iter()
        .map(|loan| {
            let principal = if loan.asset_type == "money" {
                ui::money(ctx.data().config.as_ref(), loan.principal_mana.unwrap_or(0))
            } else {
                ui::shares(loan.principal_shares.unwrap_or(0.0))
            };
            let remaining = if loan.asset_type == "money" {
                ui::money(
                    ctx.data().config.as_ref(),
                    (loan.repayment_mana.unwrap_or(0) - loan.repaid_mana).max(0),
                )
            } else {
                ui::shares((loan.repayment_shares.unwrap_or(0.0) - loan.repaid_shares).max(0.0))
            };
            format!(
                "**#{}** • {} with **{}**\nPrincipal: {} • Remaining: {}\nStatus: **{}** • Due {}",
                loan.loan_id,
                loan.direction,
                loan.counterparty_display_name,
                principal,
                remaining,
                loan.status,
                ui::discord_timestamp(loan.due_at)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "📚 Loan Status",
        body,
        poise::serenity_prelude::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn repay_loan(
    ctx: Context<'_>,
    #[description = "Active loan"]
    loan: String,
    #[description = "Money repayment amount"] amount_mana: Option<i64>,
    #[description = "Share repayment amount"] shares: Option<f64>,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let loan_id = parse_id(&loan, "loan")?;
    let receipt = ctx
        .data()
        .services
        .social
        .repay_loan(
            &guild_id,
            loan_id,
            &ctx.author().id.to_string(),
            amount_mana,
            shares,
        )
        .await?;
    let paid = if receipt.asset_type == "money" {
        ui::money(ctx.data().config.as_ref(), receipt.paid_mana.unwrap_or(0))
    } else {
        ui::shares(receipt.paid_shares.unwrap_or(0.0))
    };
    let remaining = if receipt.asset_type == "money" {
        ui::money(ctx.data().config.as_ref(), receipt.remaining_mana.unwrap_or(0))
    } else {
        ui::shares(receipt.remaining_shares.unwrap_or(0.0))
    };
    ui::send_embed(
        ctx,
        "💳 Loan Repaid",
        format!(
            "**Loan:** **#{}**\n**Paid:** {}\n**Remaining:** {}\n**Status:** **{}**",
            receipt.loan_id, paid, remaining, receipt.status
        ),
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

async fn autocomplete_incoming_loan(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let Ok(guild_id) = require_guild(ctx) else {
        return Vec::new();
    };
    ctx.data()
        .services
        .social
        .autocomplete_incoming_loans(&guild_id, &ctx.author().id.to_string(), partial, 20)
        .await
        .unwrap_or_default()
}

fn require_guild(ctx: Context<'_>) -> Result<String, AppError> {
    ctx.guild_id()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::Validation("this command only works inside a server".to_string()))
}

fn parse_id(value: &str, label: &str) -> Result<i64, AppError> {
    value.trim().parse::<i64>().map_err(|_| {
        AppError::Validation(format!(
            "pick a {label} from the autocomplete list or enter a numeric id"
        ))
    })
}
