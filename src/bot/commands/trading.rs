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
            option_label: option.clone(),
            amount_mana: amount,
        })
        .await?;
    ctx.say(format!(
        "Bought into market #{}.\nSource: {}\nOption: {}\nSpent: {} mana\nReceived: {:.4} shares\nPrice: {:.2}% -> {:.2}%\nBalance: {}",
        receipt.market_id,
        receipt.market_type,
        receipt.option_label,
        receipt.mana_amount,
        receipt.shares_delta,
        receipt.price_before * 100.0,
        receipt.price_after * 100.0,
        receipt.balance_mana
    ))
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
            option_label: option.clone(),
            shares,
        })
        .await?;
    ctx.say(format!(
        "Sold from market #{}.\nSource: {}\nOption: {}\nReceived: {} mana\nShares sold: {:.4}\nPrice: {:.2}% -> {:.2}%\nBalance: {}",
        receipt.market_id,
        receipt.market_type,
        receipt.option_label,
        receipt.mana_amount,
        receipt.shares_delta,
        receipt.price_before * 100.0,
        receipt.price_after * 100.0,
        receipt.balance_mana
    ))
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
        ctx.say("You do not hold any positions for that filter.")
            .await?;
        return Ok(());
    }

    let body = positions
        .into_iter()
        .map(|(position, market, option)| {
            format!(
                "#{} [{}|{}] {} -> {}: {:.4} shares (spent {}, received {})",
                market.id,
                market.market_type,
                market.status,
                market.question,
                option.label,
                position.shares,
                position.total_spent_mana,
                position.total_received_mana
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
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
        ctx.say("You do not hold any Manifold-tracked positions for that filter.")
            .await?;
        return Ok(());
    }

    let body = positions
        .into_iter()
        .map(|(position, market, option)| {
            format!(
                "#{} [{}|{}] {} -> {}: {:.4} shares (spent {}, received {})",
                market.id,
                market.market_type,
                market.status,
                market.question,
                option.label,
                position.shares,
                position.total_spent_mana,
                position.total_received_mana
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ctx.say(body).await?;
    Ok(())
}
