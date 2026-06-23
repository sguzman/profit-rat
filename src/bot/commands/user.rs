use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::ui;
use crate::bot::{display_name, Context};
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), AppError> {
    ui::send_embed(
        ctx,
        "🐀 Profit Rat Online",
        "The rat is awake, logging hard, and hoarding fake money for the server.",
        serenity::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn balance(
    ctx: Context<'_>,
    #[description = "Optional user to inspect"] user: Option<serenity::User>,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("balances only exist inside a server economy".to_string())
    })?;
    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let name = display_name(target);
    let summary = ctx
        .data()
        .services
        .users
        .balance(&guild_id.to_string(), &target.id.to_string(), &name)
        .await?;
    let cooldown = format_claim_time(summary.next_claim_at);
    let config = ctx.data().config.as_ref();
    ui::send_embed(
        ctx,
        "💰 Balance Check",
        format!(
            "**{name}** has {}.\n**Total claimed:** {}\n{cooldown}",
            ui::money(config, summary.balance_mana),
            ui::money(config, summary.total_claimed_mana)
        ),
        serenity::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn claim(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("claims only exist inside a server economy".to_string())
    })?;
    let name = display_name(ctx.author());
    let receipt = ctx
        .data()
        .services
        .users
        .claim(&guild_id.to_string(), &ctx.author().id.to_string(), &name)
        .await?;
    let config = ctx.data().config.as_ref();
    ui::send_embed(
        ctx,
        "🎁 Login Claim Collected",
        format!(
            "The rat approved your {} payout of {}.\n**New balance:** {}\n**Next claim:** {}",
            ctx.data().config.claim_period_name,
            ui::money(config, receipt.amount_mana),
            ui::money(config, receipt.balance_mana),
            discord_timestamp(receipt.next_claim_at)
        ),
        serenity::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

fn format_claim_time(next_claim_at: Option<DateTime<Utc>>) -> String {
    match next_claim_at {
        Some(value) if value > Utc::now() => format!("Next claim: {}", discord_timestamp(value)),
        _ => "Claim is ready now.".to_string(),
    }
}

fn discord_timestamp(value: DateTime<Utc>) -> String {
    format!("<t:{}:F> (<t:{}:R>)", value.timestamp(), value.timestamp())
}
