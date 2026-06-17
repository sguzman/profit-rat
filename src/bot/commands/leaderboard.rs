use crate::bot::Context;
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn leaderboard(ctx: Context<'_>) -> Result<(), AppError> {
    let entries = ctx.data().services.leaderboards.top_balances(10).await?;
    if entries.is_empty() {
        ctx.say("Nobody has fake money yet.").await?;
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
            format!("{}. {} - {} mana", index + 1, name, entry.balance_mana)
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
    Ok(())
}
