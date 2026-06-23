use std::collections::HashMap;

use poise::serenity_prelude as serenity;
use tracing::info;

use crate::bot::charts;
use crate::bot::commands::market::{autocomplete_any_market, parse_market_id};
use crate::bot::{display_name, Context};
use crate::error::{AppError, AppResult};

#[poise::command(slash_command)]
pub async fn histogram_prices(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_any_market"]
    market: String,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let view = ctx
        .data()
        .services
        .markets
        .market_view_for_guild(&guild_id.to_string(), market_id)
        .await?;
    let artifact = charts::render_option_price_histogram(ctx.data().config.as_ref(), &view)?;
    info!(
        market_id,
        filename = %artifact.filename,
        "rendered price histogram"
    );
    send_chart(
        ctx,
        artifact,
        format!("📊 Price Histogram · #{}", market_id),
        format!(
            "{}\nShows current price/share for every option in this market.",
            view.detail.market.question
        ),
    )
    .await
}

#[poise::command(slash_command)]
pub async fn histogram_holders(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_any_market"]
    market: String,
    #[description = "shares or value"] metric: Option<String>,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let metric = parse_holder_metric(metric.as_deref())?;
    let (view, mut holders) = ctx
        .data()
        .services
        .markets
        .market_holders(&guild_id.to_string(), market_id)
        .await?;
    if holders.is_empty() {
        return Err(AppError::Validation(
            "that market does not have any holders yet".to_string(),
        ));
    }

    holders.sort_by(|left, right| {
        let left_value = match metric {
            HolderMetric::Shares => left.shares,
            HolderMetric::Value => left.current_value_mana as f64,
        };
        let right_value = match metric {
            HolderMetric::Shares => right.shares,
            HolderMetric::Value => right.current_value_mana as f64,
        };
        right_value.total_cmp(&left_value)
    });

    let bars = holders
        .iter()
        .take(12)
        .map(|holder| {
            let value = match metric {
                HolderMetric::Shares => holder.shares,
                HolderMetric::Value => holder.current_value_mana as f64,
            };
            let detail = match metric {
                HolderMetric::Shares => {
                    format!("{} on {}", holder.option_label, holder.display_name)
                }
                HolderMetric::Value => {
                    format!("{} value · {}", holder.option_label, holder.display_name)
                }
            };
            (holder.display_name.clone(), value, detail)
        })
        .collect::<Vec<_>>();

    let artifact = charts::render_holder_concentration_histogram(
        ctx.data().config.as_ref(),
        market_id,
        &view.detail.market.question,
        metric.label(),
        &bars,
    )?;
    info!(
        market_id,
        metric = %metric.label(),
        filename = %artifact.filename,
        "rendered holder histogram"
    );
    send_chart(
        ctx,
        artifact,
        format!("👥 Holder Histogram · #{}", market_id),
        format!(
            "{}\nShows the top holders in this market ranked by {}.",
            view.detail.market.question,
            metric.label()
        ),
    )
    .await
}

#[poise::command(slash_command)]
pub async fn histogram_position(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_any_market"]
    market: String,
    #[description = "Optional user to inspect"] user: Option<serenity::User>,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let target_user = user.as_ref().unwrap_or(ctx.author());
    let target_user_id = target_user.id.to_string();
    let target_display_name = display_name(target_user);
    let positions = ctx
        .data()
        .services
        .markets
        .position_summaries_for_user(&guild_id.to_string(), &target_user_id, Some(market_id))
        .await?;
    if positions.is_empty() {
        return Err(AppError::Validation(if user.is_some() {
            "that user does not hold any shares in that market yet".to_string()
        } else {
            "you do not hold any shares in that market yet".to_string()
        }));
    }

    let question = positions[0].market_question.clone();
    let mut by_option = HashMap::<String, (f64, i64, i64)>::new();
    for position in positions {
        let entry = by_option.entry(position.option_label).or_insert((0.0, 0, 0));
        entry.0 += position.shares;
        entry.1 += position.current_value_mana;
        entry.2 += position.payout_if_correct_mana;
    }

    let mut bars = by_option
        .into_iter()
        .map(|(label, (shares, value, payout))| {
            (label, shares, format!("Value {value} · Payout {payout}"))
        })
        .collect::<Vec<_>>();
    bars.sort_by(|left, right| right.1.total_cmp(&left.1));

    let artifact =
        charts::render_position_histogram(ctx.data().config.as_ref(), market_id, &question, &bars)?;
    info!(
        market_id,
        user_id = %target_user_id,
        filename = %artifact.filename,
        "rendered position histogram"
    );
    send_chart(
        ctx,
        artifact,
        format!("🧺 Position Histogram · #{}", market_id),
        format!(
            "{}\nShows {}'s current exposure by option.",
            question, target_display_name
        ),
    )
    .await
}

#[poise::command(slash_command)]
pub async fn histogram_time(
    ctx: Context<'_>,
    #[description = "Pick a market"]
    #[autocomplete = "autocomplete_any_market"]
    market: String,
) -> Result<(), AppError> {
    let guild_id = require_guild(ctx)?;
    let market_id = parse_market_id(&market)?;
    let history = ctx
        .data()
        .services
        .markets
        .market_time_series_for_guild(&guild_id.to_string(), market_id)
        .await?;
    let artifact = charts::render_time_series_chart(ctx.data().config.as_ref(), &history)?;
    info!(
        market_id,
        filename = %artifact.filename,
        "rendered time histogram"
    );
    send_chart(
        ctx,
        artifact,
        format!("📈 Time Chart · #{}", market_id),
        format!(
            "{}\nShows how each option's implied price has moved over time.",
            history.question
        ),
    )
    .await
}

async fn send_chart(
    ctx: Context<'_>,
    artifact: charts::ChartArtifact,
    title: String,
    description: String,
) -> AppResult<()> {
    let image_url = format!("attachment://{}", artifact.filename);
    let embed = serenity::CreateEmbed::new()
        .title(title)
        .description(description)
        .image(image_url)
        .color(serenity::Colour::from_rgb(32, 178, 170));
    let attachment = serenity::CreateAttachment::bytes(artifact.bytes, artifact.filename);
    ctx.send(
        poise::CreateReply::default()
            .embed(embed)
            .attachment(attachment),
    )
    .await?;
    Ok(())
}

fn require_guild(ctx: Context<'_>) -> AppResult<serenity::GuildId> {
    ctx.guild_id().ok_or_else(|| {
        AppError::Validation("charts only make sense inside a server economy".to_string())
    })
}

fn parse_holder_metric(value: Option<&str>) -> AppResult<HolderMetric> {
    match value.unwrap_or("shares").trim().to_ascii_lowercase().as_str() {
        "shares" => Ok(HolderMetric::Shares),
        "value" => Ok(HolderMetric::Value),
        _ => Err(AppError::Validation(
            "metric must be either `shares` or `value`".to_string(),
        )),
    }
}

enum HolderMetric {
    Shares,
    Value,
}

impl HolderMetric {
    fn label(&self) -> &'static str {
        match self {
            Self::Shares => "shares",
            Self::Value => "value",
        }
    }
}
