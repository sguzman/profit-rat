use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), AppError> {
    ui::send_embed(
        ctx,
        "🐀 Profit Rat Online",
        "The rat is awake, logging hard, and hoarding fake mana for the server.",
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn balance(
    ctx: Context<'_>,
    #[description = "Optional user to inspect"] user: Option<serenity::User>,
) -> Result<(), AppError> {
    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let name = display_name(target);
    let summary = ctx
        .data()
        .services
        .users
        .balance(&target.id.to_string(), &name)
        .await?;
    let cooldown = format_claim_time(summary.next_claim_at);
    ui::send_embed(
        ctx,
        "💰 Balance Check",
        format!(
            "**{name}** has {}.\n**Total claimed:** {}\n{cooldown}",
            ui::money(summary.balance_mana),
            ui::money(summary.total_claimed_mana)
        ),
        poise::serenity_prelude::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn claim(ctx: Context<'_>) -> Result<(), AppError> {
    let name = display_name(ctx.author());
    let receipt = ctx
        .data()
        .services
        .users
        .claim(&ctx.author().id.to_string(), &name)
        .await?;
    ui::send_embed(
        ctx,
        "🪙 Claim Collected",
        format!(
            "The communal cope fountain paid out {}.\n**New balance:** {}\n**Next claim:** {}",
            ui::money(receipt.amount_mana),
            ui::money(receipt.balance_mana),
            discord_timestamp(receipt.next_claim_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
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
