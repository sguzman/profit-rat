use crate::bot::{Context, display_name};
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), AppError> {
    ctx.say("profit-rat is alive, logging, and hoarding fake mana.")
        .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn balance(ctx: Context<'_>) -> Result<(), AppError> {
    let name = display_name(ctx.author());
    let summary = ctx
        .data()
        .services
        .users
        .balance(&ctx.author().id.to_string(), &name)
        .await?;
    let cooldown = summary
        .next_claim_at
        .map(|value| format!("Next claim: {}", value.to_rfc3339()))
        .unwrap_or_else(|| "Claim is ready now.".to_string());
    ctx.say(format!(
        "{name} has {} mana.\nTotal claimed: {}\n{cooldown}",
        summary.balance_mana, summary.total_claimed_mana
    ))
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
    ctx.say(format!(
        "The communal cope fountain paid out {} mana.\nBalance: {}\nNext claim: {}",
        receipt.amount_mana,
        receipt.balance_mana,
        receipt.next_claim_at.to_rfc3339()
    ))
    .await?;
    Ok(())
}
