use crate::bot::Context;
use crate::bot::ui;
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn leaderboard(ctx: Context<'_>) -> Result<(), AppError> {
    let entries = ctx.data().services.leaderboards.top_balances(10).await?;
    if entries.is_empty() {
        ui::send_embed(
            ctx,
            "🏆 Leaderboard",
            "Nobody has fake money yet. The rat economy is still asleep.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry
                .display_name
                .as_deref()
                .unwrap_or(entry.discord_user_id.as_str());
            let medal = match index {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "🐀",
            };
            format!(
                "{} **{}. {}** — {}",
                medal,
                index + 1,
                name,
                ui::money(entry.balance_mana)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui::send_embed(
        ctx,
        "🏆 Richest Rats",
        body,
        poise::serenity_prelude::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}
