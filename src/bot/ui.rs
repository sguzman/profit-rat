use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::Context;
use crate::domain::market::{MarketStatus, MarketType};
use crate::error::AppError;
use crate::services::market_service::MarketView;

pub async fn send_embed(
    ctx: Context<'_>,
    title: impl Into<String>,
    description: impl Into<String>,
    color: serenity::Colour,
) -> Result<(), AppError> {
    let embed = serenity::CreateEmbed::new()
        .title(title.into())
        .description(description.into())
        .color(color);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

pub async fn send_market_embed(
    ctx: Context<'_>,
    title_prefix: &str,
    view: &MarketView,
    extra_description: Option<String>,
) -> Result<(), AppError> {
    let market = &view.detail.market;
    let mut description = format!(
        "{} **{}**\n**Market:** {}\n**Status:** {}\n**Type:** {}",
        market_emoji(market.market_type()),
        market.question,
        market_id_line(market.id),
        status_badge(market.status()),
        type_badge(market.market_type())
    );

    if let Some(close_time) = market.close_time() {
        description.push_str(&format!("\n**Closes:** {}", discord_timestamp(close_time)));
    }

    if let Some(source) = market.external_url.as_ref() {
        description.push_str(&format!("\n**Source:** {source}"));
    }

    if let Some(extra) = extra_description {
        description.push_str(&format!("\n\n{extra}"));
    }

    let odds_board = view
        .detail
        .options
        .iter()
        .zip(view.probabilities.iter())
        .map(|(option, probability)| {
            format!(
                "{} **{}**  •  {}  •  {}",
                option_emoji(option.label.as_str()),
                option.label,
                percent(*probability),
                shares(option.shares_outstanding)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let embed = serenity::CreateEmbed::new()
        .title(format!("{title_prefix} {}", market_id_line(market.id)))
        .description(description)
        .field("📊 Odds Board", odds_board, false)
        .color(market_color(market.market_type(), market.status()));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

pub fn money(amount: i64) -> String {
    format!("**{} mana**", amount)
}

pub fn shares(amount: f64) -> String {
    format!("**{amount:.4} shares**")
}

pub fn percent(value: f64) -> String {
    format!("**{:.2}%**", value * 100.0)
}

pub fn discord_timestamp(value: DateTime<Utc>) -> String {
    format!("<t:{}:F> (<t:{}:R>)", value.timestamp(), value.timestamp())
}

pub fn market_id_line(market_id: i64) -> String {
    format!("#{}", market_id)
}

pub fn status_badge(status: MarketStatus) -> &'static str {
    match status {
        MarketStatus::Open => "🟢 **Open**",
        MarketStatus::Closed => "🟡 **Closed**",
        MarketStatus::Resolved => "🔨 **Resolved**",
        MarketStatus::Settled => "💸 **Settled**",
        MarketStatus::Cancelled => "⚫ **Cancelled**",
        MarketStatus::NeedsManualReview => "🟠 **Needs Review**",
    }
}

pub fn type_badge(market_type: MarketType) -> &'static str {
    match market_type {
        MarketType::Native => "🐀 **Native Rat Market**",
        MarketType::Manifold => "🛰️ **Manifold Mirror**",
    }
}

pub fn market_emoji(market_type: MarketType) -> &'static str {
    match market_type {
        MarketType::Native => "📈",
        MarketType::Manifold => "🛰️",
    }
}

pub fn option_emoji(label: &str) -> &'static str {
    match label.to_ascii_uppercase().as_str() {
        "YES" => "🟢",
        "NO" => "🔴",
        _ => "🔹",
    }
}

pub fn market_color(market_type: MarketType, status: MarketStatus) -> serenity::Colour {
    match (market_type, status) {
        (_, MarketStatus::Open) => serenity::Colour::from_rgb(46, 204, 113),
        (_, MarketStatus::Closed) => serenity::Colour::from_rgb(241, 196, 15),
        (MarketType::Native, MarketStatus::Resolved | MarketStatus::Settled) => {
            serenity::Colour::from_rgb(52, 152, 219)
        }
        (MarketType::Manifold, MarketStatus::Resolved | MarketStatus::Settled) => {
            serenity::Colour::from_rgb(26, 188, 156)
        }
        (_, MarketStatus::Cancelled) => serenity::Colour::from_rgb(127, 140, 141),
        (_, MarketStatus::NeedsManualReview) => serenity::Colour::from_rgb(230, 126, 34),
    }
}
