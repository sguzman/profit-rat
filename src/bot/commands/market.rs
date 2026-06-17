use chrono::{DateTime, Utc};

use crate::bot::{Context, display_name};
use crate::error::{AppError, AppResult};
use crate::services::market_service::CreateMarketRequest;

#[poise::command(slash_command)]
pub async fn create_market(
    ctx: Context<'_>,
    #[description = "The market question"] question: String,
    #[description = "Comma-separated options, e.g. YES,NO"] options: String,
    #[description = "Optional close time in RFC3339"] close_time: Option<String>,
    #[description = "Optional liquidity parameter"] liquidity_b: Option<f64>,
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
    ctx.say(render_market_view(&view)).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn list_markets(
    ctx: Context<'_>,
    #[description = "open, settled, resolved, cancelled, all"] status: Option<String>,
) -> Result<(), AppError> {
    let markets = ctx.data().services.markets.list_markets(status).await?;
    if markets.is_empty() {
        ctx.say("No markets matched that filter.").await?;
        return Ok(());
    }

    let body = markets
        .into_iter()
        .map(|market| {
            format!(
                "#{} [{}|{}] {}",
                market.id, market.market_type, market.status, market.question
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn market(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
) -> Result<(), AppError> {
    let view = ctx.data().services.markets.market_view(market_id).await?;
    ctx.say(render_market_view(&view)).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn resolve_market(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
    #[description = "Winning option label"] winning_option: String,
) -> Result<(), AppError> {
    let payout = ctx
        .data()
        .services
        .markets
        .resolve_native_market(market_id, &winning_option)
        .await?;
    ctx.say(format!(
        "Market #{market_id} settled.\nWinning option: {winning_option}\nTotal payout: {payout} mana"
    ))
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
    ctx.say(format!(
        "Tracking Manifold market.\nCreated by: {}\n{}",
        display_name(ctx.author()),
        render_market_view(&view)
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn manifold_market(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
) -> Result<(), AppError> {
    let view = ctx.data().services.markets.market_view(market_id).await?;
    ctx.say(render_market_view(&view)).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn msync(
    ctx: Context<'_>,
    #[description = "Internal market id"] market_id: i64,
) -> Result<(), AppError> {
    let view = ctx
        .data()
        .services
        .markets
        .sync_manifold_market(market_id)
        .await?;
    ctx.say(format!(
        "Synced tracked market.\n{}",
        render_market_view(&view)
    ))
    .await?;
    Ok(())
}

fn parse_optional_time(value: Option<String>) -> AppResult<Option<DateTime<Utc>>> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| {
                    AppError::Validation("close_time must be RFC3339 if provided".to_string())
                })
        })
        .transpose()
}

fn render_market_view(view: &crate::services::market_service::MarketView) -> String {
    let header = format!(
        "Market #{} [{}|{}]\n{}",
        view.detail.market.id,
        view.detail.market.market_type,
        view.detail.market.status,
        view.detail.market.question
    );
    let body = view
        .detail
        .options
        .iter()
        .zip(view.probabilities.iter())
        .map(|(option, probability)| {
            format!(
                "{}: {:.2}% | shares {:.2}",
                option.label,
                probability * 100.0,
                option.shares_outstanding
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let footer = view
        .detail
        .market
        .external_url
        .as_ref()
        .map(|value| format!("\nSource: {value}"))
        .unwrap_or_default();
    format!("{header}\n{body}{footer}")
}
