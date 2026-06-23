use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use poise::serenity_prelude as serenity;

use crate::bot::ui;
use crate::bot::{Context, display_name};
use crate::error::{AppError, AppResult};
use crate::services::bond_service::CreateBondRequest;

#[poise::command(slash_command)]
pub async fn create_bond(
    ctx: Context<'_>,
    #[description = "Short bond title"] title: String,
    #[description = "Optional bond description"] description: Option<String>,
    #[description = "Sale price per bond"] price_per_bond: i64,
    #[description = "How many bonds to issue"] total_bonds: i64,
    #[description = "Yield in bps per period, e.g. 500 = 5%"] yield_bps: i64,
    #[description = "Maturity time, e.g. 2026-06-23 18:00 or RFC3339"] maturity_time: String,
    #[description = "Optional yield period in seconds; defaults to 1 hour"]
    yield_period_seconds: Option<i64>,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("bonds can only be issued inside a server economy".to_string())
    })?;
    let matures_at = parse_optional_time(Some(maturity_time))?
        .ok_or_else(|| AppError::Validation("maturity time is required".to_string()))?;
    let receipt = ctx
        .data()
        .services
        .bonds
        .create_bond(CreateBondRequest {
            guild_id: guild_id.to_string(),
            issuer_user_id: ctx.author().id.to_string(),
            issuer_display_name: display_name(ctx.author()),
            title,
            description,
            price_per_bond_mana: price_per_bond,
            total_bonds,
            yield_bps,
            yield_period_seconds,
            matures_at,
        })
        .await?;

    ui::send_embed(
        ctx,
        "🧾 Bond Issued",
        format!(
            "**Bond:** **#{}** {}\n**Price/bond:** {}\n**Payout/bond at maturity:** {}\n**Supply:** **{}** bonds\n**Yield:** **{} bps** every {} seconds\n**Matures:** {}\n**Escrow reserved:** {}\n**Issuer balance now:** {}",
            receipt.issuance_id,
            receipt.title,
            ui::money(ctx.data().config.as_ref(), receipt.price_per_bond_mana),
            ui::money(ctx.data().config.as_ref(), receipt.payout_per_bond_mana),
            receipt.total_bonds,
            receipt.yield_bps,
            receipt.yield_period_seconds,
            ui::discord_timestamp(receipt.matures_at),
            ui::money(ctx.data().config.as_ref(), receipt.escrow_reserved_mana),
            ui::money(ctx.data().config.as_ref(), receipt.issuer_balance_mana),
        ),
        serenity::Colour::from_rgb(241, 196, 15),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn list_bonds(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("bonds only exist inside a server economy".to_string())
    })?;
    let bonds = ctx
        .data()
        .services
        .bonds
        .list_bonds(&guild_id.to_string())
        .await?;
    if bonds.is_empty() {
        ui::send_embed(
            ctx,
            "🏦 Bond Board",
            "No bonds have been issued in this server yet.",
            serenity::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = bonds
        .into_iter()
        .take(12)
        .map(|bond| {
            format!(
                "**#{}** {} [{}]\nIssuer **{}** • {} -> {} per bond\nLeft **{} / {}** • Yield **{} bps** / {}s\nMatures {}",
                bond.issuance_id,
                bond.title,
                bond.status,
                bond.issuer_display_name,
                ui::money(ctx.data().config.as_ref(), bond.price_per_bond_mana),
                ui::money(ctx.data().config.as_ref(), bond.payout_per_bond_mana),
                bond.remaining_bonds,
                bond.total_bonds,
                bond.yield_bps,
                bond.yield_period_seconds,
                ui::discord_timestamp(bond.matures_at),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    ui::send_embed(
        ctx,
        "🏦 Bond Board",
        body,
        serenity::Colour::from_rgb(52, 152, 219),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn buy_bond(
    ctx: Context<'_>,
    #[description = "Open bond issuance"]
    #[autocomplete = "autocomplete_open_bond"]
    bond: String,
    #[description = "How many bonds to buy"] quantity: i64,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("bonds only exist inside a server economy".to_string())
    })?;
    let issuance_id = parse_bond_id(&bond)?;
    let receipt = ctx
        .data()
        .services
        .bonds
        .buy_bond(
            &guild_id.to_string(),
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            issuance_id,
            quantity,
        )
        .await?;

    ui::send_embed(
        ctx,
        "💵 Bond Purchased",
        format!(
            "**Bond:** **#{}** {}\n**Quantity:** **{}**\n**Spent:** {}\n**Projected maturity payout:** {}\n**Remaining supply:** **{}**\n**Balance now:** {}",
            receipt.issuance_id,
            receipt.title,
            receipt.quantity,
            ui::money(ctx.data().config.as_ref(), receipt.spent_mana),
            ui::money(ctx.data().config.as_ref(), receipt.payout_at_maturity_mana),
            receipt.remaining_bonds,
            ui::money(ctx.data().config.as_ref(), receipt.buyer_balance_mana),
        ),
        serenity::Colour::from_rgb(46, 204, 113),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn sell_bond(
    ctx: Context<'_>,
    #[description = "Bond issuance to pitch to Profit Rat"]
    #[autocomplete = "autocomplete_open_bond"]
    bond: String,
) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("bonds only exist inside a server economy".to_string())
    })?;
    let issuance_id = parse_bond_id(&bond)?;
    let bot_user = ctx.serenity_context().cache.current_user().clone();
    let bot_display_name = bot_user
        .global_name
        .clone()
        .unwrap_or_else(|| bot_user.name.clone());
    let outcome = ctx
        .data()
        .services
        .bonds
        .sell_bond_to_bot(
            &guild_id.to_string(),
            &ctx.author().id.to_string(),
            &display_name(ctx.author()),
            &bot_user.id.to_string(),
            &bot_display_name,
            issuance_id,
        )
        .await?;

    if let Some(receipt) = outcome.receipt {
        ui::send_embed(
            ctx,
            "🤝 Bond Sold To Rat",
            format!(
                "**Bond:** **#{}** {}\n**Rat bought:** **{}** bond(s)\n**Rat spent:** {}\n**Rat projected payout:** {}\n**Remaining supply:** **{}**\n**Rat balance now:** {}\n\n✅ {}",
                receipt.issuance_id,
                receipt.title,
                receipt.quantity,
                ui::money(ctx.data().config.as_ref(), receipt.spent_mana),
                ui::money(
                    ctx.data().config.as_ref(),
                    receipt.payout_at_maturity_mana
                ),
                receipt.remaining_bonds,
                ui::money(ctx.data().config.as_ref(), receipt.buyer_balance_mana),
                outcome.reason,
            ),
            serenity::Colour::from_rgb(39, 174, 96),
        )
        .await?;
    } else {
        ui::send_embed(
            ctx,
            "🧾 Bond Rejected",
            format!(
                "**Bond:** **#{}**\nProfit Rat passed on this one.\n\n❌ {}",
                issuance_id, outcome.reason
            ),
            serenity::Colour::from_rgb(231, 76, 60),
        )
        .await?;
    }

    Ok(())
}

#[poise::command(slash_command)]
pub async fn my_bonds(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        AppError::Validation("bonds only exist inside a server economy".to_string())
    })?;
    let positions = ctx
        .data()
        .services
        .bonds
        .positions_for_holder(&guild_id.to_string(), &ctx.author().id.to_string())
        .await?;
    if positions.is_empty() {
        ui::send_embed(
            ctx,
            "📜 Your Bonds",
            "You are not holding any bonds in this server right now.",
            serenity::Colour::from_rgb(127, 140, 141),
        )
        .await?;
        return Ok(());
    }

    let body = positions
        .into_iter()
        .map(|position| {
            format!(
                "**#{}** {} [{}]\nIssuer **{}** • Bonds **{}**\nSpent {} • Projected payout {} • Redeemed {}\nPayout/bond {} • Matures {}",
                position.issuance_id,
                position.title,
                position.status,
                position.issuer_display_name,
                position.bonds_owned,
                ui::money(ctx.data().config.as_ref(), position.total_spent_mana),
                ui::money(ctx.data().config.as_ref(), position.projected_payout_mana),
                ui::money(ctx.data().config.as_ref(), position.total_redeemed_mana),
                ui::money(ctx.data().config.as_ref(), position.payout_per_bond_mana),
                ui::discord_timestamp(position.matures_at),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ui::send_embed(
        ctx,
        "📜 Your Bonds",
        body,
        serenity::Colour::from_rgb(155, 89, 182),
    )
    .await?;
    Ok(())
}

async fn autocomplete_open_bond(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let Some(guild_id) = ctx.guild_id() else {
        return Vec::new();
    };
    ctx.data()
        .services
        .bonds
        .autocomplete_open_bonds(&guild_id.to_string(), partial, 20)
        .await
        .unwrap_or_default()
}

fn parse_bond_id(value: &str) -> AppResult<i64> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| AppError::Validation("pick a bond from autocomplete or enter a numeric bond id".to_string()))
}

fn parse_optional_time(input: Option<String>) -> AppResult<Option<DateTime<Utc>>> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(Some(parsed.with_timezone(&Utc)));
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M") {
        return resolve_local_naive_datetime(parsed).map(Some);
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M") {
        return resolve_local_naive_datetime(parsed).map(Some);
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let local_midnight = parsed
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| AppError::Validation("date was not valid".to_string()))?;
        return resolve_local_naive_datetime(local_midnight).map(Some);
    }

    Err(AppError::Validation(
        "time must look like `2026-06-23 18:00`, `2026-06-23T18:00`, `2026-06-23`, or full RFC3339".to_string(),
    ))
}

fn resolve_local_naive_datetime(input: NaiveDateTime) -> AppResult<DateTime<Utc>> {
    match Local.from_local_datetime(&input) {
        LocalResult::Single(local) => Ok(local.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::Validation(
            "that local time does not exist in your timezone".to_string(),
        )),
    }
}
