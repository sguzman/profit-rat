use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::AppError;
use crate::services::trading_service::{BuyRequest, SellRequest};

#[poise::command(slash_command)]
pub async fn buy(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
    #[description = "Option label"] option: String,
    #[description = "Amount of fake mana to spend"] amount: i64,
) -> Result<(), AppError> {
    let receipt = ctx
        .data()
        .services
        .trading
        .buy(BuyRequest {
            user_id: ctx.author().id.to_string(),
            display_name: display_name(ctx.author()),
            market_id,
            option_label: option,
            amount_mana: amount,
        })
        .await?;
    ui::send_embed(
        ctx,
        "🛒 Position Bought",
        format!(
            "**Market:** {} **#{}**\n**Source:** {}\n**Option:** {} **{}**\n**Spent:** {}\n**Received:** {}\n**Price move:** {} → {}\n**Balance:** {}",
            if receipt.market_type == "manifold" {
                "🛰️"
            } else {
                "📈"
            },
            receipt.market_id,
            if receipt.market_type == "manifold" {
                "🛰️ **Manifold Mirror**"
            } else {
                "🐀 **Native Rat Market**"
            },
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            ui::money(receipt.mana_amount),
            ui::shares(receipt.shares_delta),
            ui::percent(receipt.price_before),
            ui::percent(receipt.price_after),
            ui::money(receipt.balance_mana)
        ),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn sell(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
    #[description = "Option label"] option: String,
    #[description = "Shares to sell"] shares: f64,
) -> Result<(), AppError> {
    let receipt = ctx
        .data()
        .services
        .trading
        .sell(SellRequest {
            user_id: ctx.author().id.to_string(),
            display_name: display_name(ctx.author()),
            market_id,
            option_label: option,
            shares,
        })
        .await?;
    ui::send_embed(
        ctx,
        "💸 Position Sold",
        format!(
            "**Market:** {} **#{}**\n**Source:** {}\n**Option:** {} **{}**\n**Received:** {}\n**Shares sold:** {}\n**Price move:** {} → {}\n**Balance:** {}",
            if receipt.market_type == "manifold" {
                "🛰️"
            } else {
                "📈"
            },
            receipt.market_id,
            if receipt.market_type == "manifold" {
                "🛰️ **Manifold Mirror**"
            } else {
                "🐀 **Native Rat Market**"
            },
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            ui::money(receipt.mana_amount),
            ui::shares(receipt.shares_delta),
            ui::percent(receipt.price_before),
            ui::percent(receipt.price_after),
            ui::money(receipt.balance_mana)
        ),
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn positions(
    ctx: Context<'_>,
    #[description = "Optional market filter"] market_id: Option<i64>,
) -> Result<(), AppError> {
    let positions = ctx
        .data()
        .services
        .markets
        .positions_for_user(&ctx.author().id.to_string(), market_id)
        .await?;
    if positions.is_empty() {
        ui::send_embed(
            ctx,
            "📦 Your Positions",
            "You do not hold any positions for that filter yet.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = positions
        .into_iter()
        .map(|(position, market, option)| {
            format!(
                "{} **#{}**  {}\n{} **{}** → {}\nSpent {} • Received {}",
                if market.market_type == "manifold" {
                    "🛰️"
                } else {
                    "📈"
                },
                market.id,
                market.question,
                match market.status.as_str() {
                    "open" => "🟢",
                    "settled" => "💸",
                    "resolved" => "🔨",
                    "cancelled" => "⚫",
                    _ => "🟡",
                },
                option.label,
                ui::shares(position.shares),
                ui::money(position.total_spent_mana),
                ui::money(position.total_received_mana)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "📦 Your Positions",
        body,
        poise::serenity_prelude::Colour::from_rgb(155, 89, 182),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn mpositions(
    ctx: Context<'_>,
    #[description = "Optional tracked market filter"] market_id: Option<i64>,
) -> Result<(), AppError> {
    let positions = ctx
        .data()
        .services
        .markets
        .positions_for_user(&ctx.author().id.to_string(), market_id)
        .await?
        .into_iter()
        .filter(|(_, market, _)| market.market_type == "manifold")
        .collect::<Vec<_>>();
    if positions.is_empty() {
        ui::send_embed(
            ctx,
            "🛰️ Mirror Positions",
            "You do not hold any Manifold-tracked positions for that filter yet.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = positions
        .into_iter()
        .map(|(position, market, option)| {
            format!(
                "🛰️ **#{}**  {}\n{} **{}** → {}\nSpent {} • Received {}",
                market.id,
                market.question,
                match market.status.as_str() {
                    "open" => "🟢",
                    "settled" => "💸",
                    "resolved" => "🔨",
                    "cancelled" => "⚫",
                    _ => "🟡",
                },
                option.label,
                ui::shares(position.shares),
                ui::money(position.total_spent_mana),
                ui::money(position.total_received_mana)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "🛰️ Mirror Positions",
        body,
        poise::serenity_prelude::Colour::from_rgb(26, 188, 156),
    )
    .await?;
    Ok(())
}
