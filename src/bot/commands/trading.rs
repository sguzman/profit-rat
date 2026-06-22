use crate::bot::commands::market::{
    autocomplete_market_option, autocomplete_open_market, parse_market_id,
};
use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::AppError;
use crate::services::trading_service::{BuyRequest, CreateShareOfferRequest, SellRequest};
use poise::serenity_prelude as serenity;

#[poise::command(slash_command)]
pub async fn buy(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_open_market"]
    market: String,
    #[description = "Option label"]
    #[autocomplete = "autocomplete_market_option"]
    option: String,
    #[description = "Amount of fake mana to spend"] amount: i64,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
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
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_open_market"]
    market: String,
    #[description = "Option label"]
    #[autocomplete = "autocomplete_market_option"]
    option: String,
    #[description = "Shares to sell"] shares: f64,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
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
pub async fn offer_shares(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_open_market"]
    market: String,
    #[description = "Option label"]
    #[autocomplete = "autocomplete_market_option"]
    option: String,
    #[description = "User who can accept this offer"] buyer: serenity::User,
    #[description = "Shares to offer"] shares: f64,
    #[description = "Total price in fake mana"] price_mana: i64,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
    let receipt = ctx
        .data()
        .services
        .trading
        .create_share_offer(CreateShareOfferRequest {
            seller_user_id: ctx.author().id.to_string(),
            seller_display_name: display_name(ctx.author()),
            buyer_user_id: buyer.id.to_string(),
            buyer_display_name: display_name(&buyer),
            market_id,
            option_label: option,
            shares,
            price_mana,
        })
        .await?;
    ui::send_embed(
        ctx,
        "ðŸ¤ Share Offer Sent",
        format!(
            "**Offer:** **#{}**\n**Market:** {} **#{}**\n**Option:** {} **{}**\n**Buyer:** <@{}> (**{}**)\n**Shares:** {}\n**Price:** {}\n**Expires:** {}",
            receipt.offer_id,
            if receipt.market_type == "manifold" {
                "ðŸ›°ï¸"
            } else {
                "ðŸ“ˆ"
            },
            receipt.market_id,
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            buyer.id,
            receipt.buyer_display_name,
            ui::shares(receipt.shares),
            ui::money(receipt.price_mana),
            ui::discord_timestamp(receipt.expires_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(230, 126, 34),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn incoming_share_offers(ctx: Context<'_>) -> Result<(), AppError> {
    let offers = ctx
        .data()
        .services
        .trading
        .incoming_share_offers(&ctx.author().id.to_string())
        .await?;
    if offers.is_empty() {
        ui::send_embed(
            ctx,
            "ðŸ“¨ Incoming Offers",
            "No pending share offers are waiting on you right now.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = offers
        .into_iter()
        .map(|offer| {
            format!(
                "**#{}**  ðŸŽ¯ **#{}**\n**{}**\nFrom **{}**  â€¢  {} **{}**\n{} for {}  â€¢  Expires {}",
                offer.offer_id,
                offer.market_id,
                offer.market_question,
                offer.seller_display_name,
                ui::option_emoji(&offer.option_label),
                offer.option_label,
                ui::shares(offer.shares),
                ui::money(offer.price_mana),
                ui::discord_timestamp(offer.expires_at)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    ui::send_embed(
        ctx,
        "ðŸ“¨ Incoming Offers",
        body,
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn accept_share_offer(
    ctx: Context<'_>,
    #[description = "Incoming offer to accept"]
    #[autocomplete = "autocomplete_incoming_share_offer"]
    offer: String,
) -> Result<(), AppError> {
    let offer_id = parse_offer_id(&offer)?;
    let receipt = ctx
        .data()
        .services
        .trading
        .accept_share_offer(
            offer_id,
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
        )
        .await?;
    ui::send_embed(
        ctx,
        "âœ… Share Offer Accepted",
        format!(
            "**Offer:** **#{}**\n**Market:** {} **#{}**\n**Seller:** **{}**\n**Option:** {} **{}**\n**Shares:** {}\n**Paid:** {}\n**New balance:** {}\n**Offer expired at:** {}",
            receipt.offer_id,
            if receipt.market_type == "manifold" {
                "ðŸ›°ï¸"
            } else {
                "ðŸ“ˆ"
            },
            receipt.market_id,
            receipt.counterparty_display_name,
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            ui::shares(receipt.shares),
            ui::money(receipt.price_mana),
            ui::money(receipt.buyer_balance_mana.unwrap_or(0)),
            ui::discord_timestamp(receipt.expires_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn decline_share_offer(
    ctx: Context<'_>,
    #[description = "Incoming offer to decline"]
    #[autocomplete = "autocomplete_incoming_share_offer"]
    offer: String,
) -> Result<(), AppError> {
    let offer_id = parse_offer_id(&offer)?;
    let receipt = ctx
        .data()
        .services
        .trading
        .decline_share_offer(offer_id, &ctx.author().id.to_string())
        .await?;
    ui::send_embed(
        ctx,
        "ðŸš« Share Offer Declined",
        format!(
            "**Offer:** **#{}**\n**Market:** {} **#{}**\n**Seller:** **{}**\n**Option:** {} **{}**\n**Shares:** {}\n**Price:** {}\n**Status:** **{}**\n**Would have expired at:** {}",
            receipt.offer_id,
            if receipt.market_type == "manifold" {
                "ðŸ›°ï¸"
            } else {
                "ðŸ“ˆ"
            },
            receipt.market_id,
            receipt.counterparty_display_name,
            ui::option_emoji(&receipt.option_label),
            receipt.option_label,
            ui::shares(receipt.shares),
            ui::money(receipt.price_mana),
            receipt.status,
            ui::discord_timestamp(receipt.expires_at)
        ),
        poise::serenity_prelude::Colour::from_rgb(231, 76, 60),
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

async fn autocomplete_incoming_share_offer(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .services
        .trading
        .autocomplete_incoming_share_offers(&ctx.author().id.to_string(), partial, 20)
        .await
        .unwrap_or_default()
}

fn parse_offer_id(value: &str) -> Result<i64, AppError> {
    value.trim().parse::<i64>().map_err(|_| {
        AppError::Validation(
            "pick an incoming offer from the autocomplete list or enter a numeric offer id"
                .to_string(),
        )
    })
}
