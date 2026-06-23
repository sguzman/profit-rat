use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::Context;
use crate::config::{AppConfig, CurrencyPosition, NegativeStyle};
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
    let config = ctx.data().config.as_ref();
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
                "{} **{}**\nPrice/share: {} | P(win): {} | {}\nPayout/share if correct: {} | Total payout if this wins: {}",
                option_emoji(option.label.as_str()),
                option.label,
                money_decimal(config, *probability, 4),
                percent(*probability),
                shares(option.shares_outstanding),
                money_decimal(config, 1.0, 4),
                money(config, option.shares_outstanding.round() as i64),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let embed = serenity::CreateEmbed::new()
        .title(format!("{title_prefix} {}", market_id_line(market.id)))
        .description(description)
        .field("Price Board", odds_board, false)
        .color(market_color(market.market_type(), market.status()));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

pub fn money(config: &AppConfig, amount: i64) -> String {
    format!("**{}**", money_plain(config, amount, true))
}

pub fn money_plain(config: &AppConfig, amount: i64, embed_context: bool) -> String {
    let sign = if amount < 0 { -1 } else { 1 };
    let absolute = amount.abs();
    let number = format_number(config, absolute, sign < 0);

    let mut pieces = Vec::new();
    if embed_context && config.currency.use_emoji_in_embeds && !currency_mark(config).is_empty() {
        pieces.push(currency_mark(config));
    } else if !embed_context
        && config.currency.use_emoji_in_plaintext
        && !currency_mark(config).is_empty()
    {
        pieces.push(currency_mark(config));
    }

    let label = if absolute == 1 {
        config.currency.singular.as_str()
    } else {
        config.currency.plural.as_str()
    };

    let textual = if config.currency.show_textual_symbol {
        config.currency.textual_symbol.as_str()
    } else {
        label
    };

    let suffix = if config.currency.show_code {
        config.currency.code.as_str()
    } else {
        textual
    };

    let space = if config.currency.space_between {
        " "
    } else {
        ""
    };
    let body = match config.currency.position {
        CurrencyPosition::Prefix
            if config.currency.show_symbol && !config.currency.symbol.is_empty() =>
        {
            format!("{}{}{}", config.currency.symbol, space, number)
        }
        CurrencyPosition::Suffix
            if config.currency.show_symbol && !config.currency.symbol.is_empty() =>
        {
            format!("{}{}{}", number, space, config.currency.symbol)
        }
        _ => format!("{}{}{}", number, space, suffix),
    };

    if pieces.is_empty() {
        body
    } else {
        format!("{} {}", pieces.join(" "), body)
    }
}

pub fn money_decimal(config: &AppConfig, amount: f64, precision: usize) -> String {
    let absolute = amount.abs();
    let formatted = format!("{absolute:.precision$}");
    let space = if config.currency.space_between {
        " "
    } else {
        ""
    };

    let unit = if config.currency.show_code {
        config.currency.code.as_str()
    } else if config.currency.show_textual_symbol {
        config.currency.textual_symbol.as_str()
    } else if (absolute - 1.0).abs() < f64::EPSILON {
        config.currency.singular.as_str()
    } else {
        config.currency.plural.as_str()
    };

    let body = match config.currency.position {
        CurrencyPosition::Prefix
            if config.currency.show_symbol && !config.currency.symbol.is_empty() =>
        {
            format!("{}{}{}", config.currency.symbol, space, formatted)
        }
        CurrencyPosition::Suffix
            if config.currency.show_symbol && !config.currency.symbol.is_empty() =>
        {
            format!("{}{}{}", formatted, space, config.currency.symbol)
        }
        _ => format!("{}{}{}", formatted, space, unit),
    };

    let signed = if amount.is_sign_negative() {
        match config.currency.negative_style {
            NegativeStyle::Minus => format!("-{body}"),
            NegativeStyle::Parens => format!("({body})"),
        }
    } else {
        body
    };

    if config.currency.use_emoji_in_embeds && !currency_mark(config).is_empty() {
        format!("{} {}", currency_mark(config), signed)
    } else {
        signed
    }
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

fn currency_mark(config: &AppConfig) -> String {
    if !config.currency.custom_emoji.is_empty() {
        return config.currency.custom_emoji.clone();
    }
    config.currency.emoji.clone()
}

fn format_number(config: &AppConfig, absolute: i64, negative: bool) -> String {
    let base = if config.currency.short_suffixes {
        shorten_number(absolute)
    } else {
        with_separator(absolute, &config.currency.thousands_separator)
    };

    if !negative {
        return base;
    }

    match config.currency.negative_style {
        NegativeStyle::Minus => format!("-{base}"),
        NegativeStyle::Parens => format!("({base})"),
    }
}

fn with_separator(value: i64, separator: &str) -> String {
    let digits = value.to_string();
    let chars = digits.chars().rev().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push_str(separator);
        }
        out.push(*ch);
    }
    out.chars().rev().collect()
}

fn shorten_number(value: i64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.1}k", value as f64 / 1_000.0),
        1_000_000..=999_999_999 => format!("{:.1}m", value as f64 / 1_000_000.0),
        _ => format!("{:.1}b", value as f64 / 1_000_000_000.0),
    }
}
