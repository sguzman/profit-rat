use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::{AppError, AppResult};
use crate::services::market_service::CreateMarketRequest;

/// Create a native fake-money market.
///
/// `options` should be comma-separated like `YES,NO` or `cats,dogs,birds`.
/// `close_time` accepts RFC3339 or simpler local formats like `2026-06-22 21:00`,
/// `2026-06-22T21:00`, or `2026-06-22`.
#[poise::command(slash_command)]
pub async fn create_market(
    ctx: Context<'_>,
    #[description = "The market question"] question: String,
    #[description = "Comma-separated options, e.g. YES,NO"] options: String,
    #[description = "Optional close time, e.g. 2026-06-22 21:00 or RFC3339"] close_time: Option<
        String,
    >,
    #[description = "Optional liquidity parameter; higher moves prices more slowly"]
    liquidity_b: Option<f64>,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("markets can only be created inside a guild".to_string())
    })?;
    let channel_id = ctx.channel_id();
    let close_time = parse_optional_time(close_time)?;
    let options = options
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let view = ctx
        .data()
        .services
        .markets
        .create_native_market(CreateMarketRequest {
            guild_id: guild_id.to_string(),
            channel_id: channel_id.to_string(),
            creator_user_id: ctx.author().id.to_string(),
            question,
            options,
            liquidity_b,
            close_time,
        })
        .await?;
    ui::send_market_embed(
        ctx,
        "📈 Market Opened",
        &view,
        Some(format!(
            "🎯 Use `/buy market:<pick from dropdown> option:<pick from dropdown> amount:<mana>` to make your first move. Market ID: **#{}**.",
            view.detail.market.id
        )),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn list_markets(
    ctx: Context<'_>,
    #[description = "open, settled, resolved, cancelled, all"] status: Option<String>,
) -> Result<(), AppError> {
    send_markets_list(ctx, status).await
}

#[poise::command(slash_command)]
pub async fn markets(
    ctx: Context<'_>,
    #[description = "open, settled, resolved, cancelled, all"] status: Option<String>,
) -> Result<(), AppError> {
    send_markets_list(ctx, status).await
}

async fn send_markets_list(ctx: Context<'_>, status: Option<String>) -> Result<(), AppError> {
    let markets = ctx.data().services.markets.list_markets(status).await?;
    if markets.is_empty() {
        ui::send_embed(
            ctx,
            "🗂️ Market List",
            "No markets matched that filter. The rat found nothing worth fake-betting on.",
            poise::serenity_prelude::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = markets
        .into_iter()
        .map(|market| {
            format!(
                "• **#{}**  {}  {}\n  **{}**",
                market.id,
                if market.market_type == "manifold" {
                    "🛰️"
                } else {
                    "📈"
                },
                match market.status.as_str() {
                    "open" => "🟢 **Open**",
                    "settled" => "💸 **Settled**",
                    "resolved" => "🔨 **Resolved**",
                    "cancelled" => "⚫ **Cancelled**",
                    _ => "🟡 **Other**",
                },
                market.question
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "🗂️ Market List",
        body,
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn market(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_any_market"]
    market: String,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
    let view = ctx.data().services.markets.market_view(market_id).await?;
    ui::send_market_embed(ctx, "🔎 Market View", &view, None).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn resolve_market(
    ctx: Context<'_>,
    #[description = "Pick a native market to resolve"]
    #[autocomplete = "autocomplete_open_native_market"]
    market: String,
    #[description = "Winning option label"]
    #[autocomplete = "autocomplete_market_option"]
    winning_option: String,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
    let payout = ctx
        .data()
        .services
        .markets
        .resolve_native_market(market_id, &winning_option)
        .await?;
    ui::send_embed(
        ctx,
        "🔨 Market Resolved",
        format!(
            "Market **#{}** has been settled.\n**Winning option:** {} **{}**\n**Total payout:** {}",
            market_id,
            ui::option_emoji(&winning_option),
            winning_option,
            ui::money(payout)
        ),
        poise::serenity_prelude::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn track_manifold(
    ctx: Context<'_>,
    #[description = "Manifold market URL or contract ID"] url_or_id: String,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("tracked markets can only be created inside a guild".to_string())
    })?;
    let channel_id = ctx.channel_id();
    let view = ctx
        .data()
        .services
        .markets
        .track_manifold_market(
            &guild_id.to_string(),
            &channel_id.to_string(),
            &ctx.author().id.to_string(),
            &url_or_id,
        )
        .await?;
    ui::send_market_embed(
        ctx,
        "🛰️ Manifold Market Tracked",
        &view,
        Some(format!(
            "Tracked by **{}**. Your fake bets will mirror Manifold prices without placing real trades.",
            display_name(ctx.author())
        )),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn manifold_market(
    ctx: Context<'_>,
    #[description = "Pick a tracked market"]
    #[autocomplete = "autocomplete_manifold_market"]
    market: String,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
    let view = ctx.data().services.markets.market_view(market_id).await?;
    ui::send_market_embed(ctx, "🛰️ Manifold View", &view, None).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn msync(
    ctx: Context<'_>,
    #[description = "Pick a tracked market to sync"]
    #[autocomplete = "autocomplete_manifold_market"]
    market: String,
) -> Result<(), AppError> {
    let market_id = parse_market_id(&market)?;
    let view = ctx
        .data()
        .services
        .markets
        .sync_manifold_market(market_id)
        .await?;
    ui::send_market_embed(
        ctx,
        "🔄 Manifold Synced",
        &view,
        Some("Fresh snapshot pulled from Manifold.".to_string()),
    )
    .await?;
    Ok(())
}

pub(crate) async fn autocomplete_any_market(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .services
        .markets
        .autocomplete_markets(partial, None, None, 20)
        .await
        .unwrap_or_default()
}

pub(crate) async fn autocomplete_open_market(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .services
        .markets
        .autocomplete_markets(partial, Some("open"), None, 20)
        .await
        .unwrap_or_default()
}

pub(crate) async fn autocomplete_open_native_market(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .services
        .markets
        .autocomplete_markets(partial, Some("open"), Some("native"), 20)
        .await
        .unwrap_or_default()
}

pub(crate) async fn autocomplete_manifold_market(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    ctx.data()
        .services
        .markets
        .autocomplete_markets(partial, None, Some("manifold"), 20)
        .await
        .unwrap_or_default()
}

pub(crate) async fn autocomplete_market_option(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let Some(market_id) = selected_market_id(ctx) else {
        return Vec::new();
    };

    ctx.data()
        .services
        .markets
        .autocomplete_market_options(market_id, partial, 20)
        .await
        .unwrap_or_default()
}

pub(crate) fn parse_market_id(value: &str) -> AppResult<i64> {
    value.trim().parse::<i64>().map_err(|_| {
        AppError::Validation(
            "pick a market from the autocomplete list or enter a numeric market id".to_string(),
        )
    })
}

fn selected_market_id(ctx: Context<'_>) -> Option<i64> {
    let poise::Context::Application(app) = ctx else {
        return None;
    };

    app.args.iter().find_map(|arg| {
        if arg.name != "market" {
            return None;
        }

        match &arg.value {
            serenity::ResolvedValue::String(value) => parse_market_id(value).ok(),
            serenity::ResolvedValue::Autocomplete { value, .. } => parse_market_id(value).ok(),
            _ => None,
        }
    })
}

fn parse_optional_time(value: Option<String>) -> AppResult<Option<DateTime<Utc>>> {
    value.map(|value| parse_human_time(&value)).transpose()
}

fn parse_human_time(value: &str) -> AppResult<DateTime<Utc>> {
    let trimmed = value.trim();

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(parsed.with_timezone(&Utc));
    }

    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y/%m/%d %H:%M"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, format) {
            return local_naive_to_utc(parsed);
        }
    }

    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let parsed = date
            .and_hms_opt(23, 59, 0)
            .ok_or_else(|| AppError::Validation("could not construct close_time".to_string()))?;
        return local_naive_to_utc(parsed);
    }

    Err(AppError::Validation(
        "close_time must look like `2026-06-22 21:00`, `2026-06-22T21:00`, `2026-06-22`, or full RFC3339".to_string(),
    ))
}

fn local_naive_to_utc(value: NaiveDateTime) -> AppResult<DateTime<Utc>> {
    match Local.from_local_datetime(&value) {
        LocalResult::Single(parsed) => Ok(parsed.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::Validation(
            "close_time could not be resolved in your local timezone".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};

    use super::parse_human_time;

    #[test]
    fn parses_rfc3339_close_time() {
        let parsed = parse_human_time("2026-06-22T21:00:00-06:00").expect("parse");
        assert_eq!(parsed.year(), 2026);
    }

    #[test]
    fn parses_simple_local_close_time() {
        let parsed = parse_human_time("2026-06-22 21:00").expect("parse");
        assert_eq!(parsed.year(), 2026);
        assert_eq!(parsed.minute(), 0);
    }

    #[test]
    fn parses_date_only_close_time() {
        let parsed = parse_human_time("2026-06-22").expect("parse");
        assert_eq!(parsed.year(), 2026);
    }
}
