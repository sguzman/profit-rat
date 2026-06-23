use crate::bot::Context;
use crate::bot::ui;
use crate::error::AppError;

#[poise::command(slash_command)]
pub async fn help(ctx: Context<'_>) -> Result<(), AppError> {
    ui::send_embed(
        ctx,
        "📚 Profit Rat Help",
        [
            "**Getting started**",
            "`/tutorial` gives you a quick walkthrough.",
            "",
            "**Core commands**",
            "`/balance` • `/claim` • `/leaderboard`",
            "`/create_market` • `/markets` • `/market` • `/market_holders`",
            "`/buy` • `/sell` • `/positions` • `/mpositions`",
            "",
            "**Social commands**",
            "`/donate_money` • `/donate_shares`",
            "`/offer_shares` • `/incoming_share_offers`",
            "`/offer_loan_money` • `/offer_loan_shares`",
            "`/incoming_loans` • `/accept_loan` • `/decline_loan` • `/loan_status` • `/repay_loan`",
            "",
            "**Tracked market commands**",
            "`/track_manifold` • `/manifold_market` • `/msync`",
            "",
            "**Discovery**",
            "`/list_commands` shows the full command catalog.",
        ]
        .join("\n"),
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn tutorial(ctx: Context<'_>) -> Result<(), AppError> {
    ui::send_embed(
        ctx,
        "🐀 Profit Rat Tutorial",
        [
            "**1. Get startup money**",
            "Use `/balance` to see your wallet, then `/claim` to grab your periodic payout.",
            "",
            "**2. Open a market**",
            "Use `/create_market question:<text> options:YES,NO` to create a native server market.",
            "",
            "**3. Browse markets**",
            "Use `/markets` to list them, then `/market` to inspect one.",
            "",
            "**4. Trade**",
            "Use `/buy` and `/sell`. The market field supports autocomplete, and the option field autocompletes from the selected market.",
            "",
            "**5. Check positions**",
            "Use `/positions` for all your holdings, or `/market_holders` to see who holds what in one market.",
            "",
            "**6. Social play**",
            "Use `/donate_money`, `/donate_shares`, or loan/share-offer commands to interact with other rats.",
            "",
            "**7. Mirror Manifold**",
            "Use `/track_manifold` with a Manifold URL, then `/buy` or `/sell` on the tracked market without placing real-world bets.",
        ]
        .join("\n"),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn list_commands(ctx: Context<'_>) -> Result<(), AppError> {
    ui::send_embed(
        ctx,
        "🧭 Command Catalog",
        [
            "**Utility**",
            "`/ping` `/help` `/tutorial` `/list_commands`",
            "",
            "**Economy**",
            "`/balance` `/claim` `/leaderboard`",
            "",
            "**Markets**",
            "`/create_market` `/markets` `/list_markets` `/market` `/market_holders` `/resolve_market`",
            "",
            "**Trading**",
            "`/buy` `/sell` `/positions` `/mpositions`",
            "",
            "**Peer interaction**",
            "`/donate_money` `/donate_shares`",
            "`/offer_shares` `/incoming_share_offers` `/accept_share_offer` `/decline_share_offer`",
            "",
            "**Loans**",
            "`/offer_loan_money` `/offer_loan_shares` `/incoming_loans` `/accept_loan` `/decline_loan` `/loan_status` `/repay_loan`",
            "",
            "**Manifold**",
            "`/track_manifold` `/manifold_market` `/msync`",
        ]
        .join("\n"),
        poise::serenity_prelude::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}
