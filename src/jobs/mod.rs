use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude as serenity;
use tracing::{error, info, instrument};

use crate::bot::ui;
use crate::config::AppConfig;
use crate::services::Services;

pub fn spawn_background_jobs(
    config: Arc<AppConfig>,
    services: Services,
    http: Arc<serenity::Http>,
) {
    let poll_every = Duration::from_secs(config.manifold_poll_interval_seconds.max(30) as u64);
    let poll_services = services.clone();
    let poll_config = config.clone();
    tokio::spawn(async move {
        info!(
            poll_seconds = poll_every.as_secs(),
            "starting manifold resolution poller"
        );

        let mut interval = tokio::time::interval(poll_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            if let Err(error) = poll_and_announce(poll_config.as_ref(), &poll_services, &http).await
            {
                error!(%error, "manifold resolution poller failed");
            }
        }
    });

    let cleanup_every =
        Duration::from_secs(config.share_offer_cleanup_interval_seconds.max(5) as u64);
    let cleanup_services = services.clone();
    tokio::spawn(async move {
        info!(
            cleanup_seconds = cleanup_every.as_secs(),
            "starting share offer expiry worker"
        );

        let mut interval = tokio::time::interval(cleanup_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            match cleanup_services.trading.expire_pending_share_offers().await {
                Ok(expired) if expired > 0 => {
                    info!(expired, "expired pending share offers");
                }
                Ok(_) => {}
                Err(error) => {
                    error!(%error, "share offer expiry worker failed");
                }
            }
            match cleanup_services.bonds.expire_pending_bond_transfer_offers().await {
                Ok(expired) if expired > 0 => {
                    info!(expired, "expired pending bond offers");
                }
                Ok(_) => {}
                Err(error) => {
                    error!(%error, "bond offer expiry worker failed");
                }
            }
        }
    });

    let loan_every =
        Duration::from_secs(config.share_offer_cleanup_interval_seconds.max(10) as u64);
    let loan_services = services.clone();
    tokio::spawn(async move {
        info!(
            cleanup_seconds = loan_every.as_secs(),
            "starting loan maintenance worker"
        );

        let mut interval = tokio::time::interval(loan_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            match loan_services.social.expire_pending_loans().await {
                Ok(expired) if expired > 0 => info!(expired, "expired pending loans"),
                Ok(_) => {}
                Err(error) => error!(%error, "loan expiry worker failed"),
            }
            match loan_services.social.mark_overdue_loans().await {
                Ok(defaulted) if defaulted > 0 => {
                    info!(defaulted, "marked overdue loans as defaulted")
                }
                Ok(_) => {}
                Err(error) => error!(%error, "loan default worker failed"),
            }
        }
    });
}

pub fn spawn_bot_behavior_jobs(
    config: Arc<AppConfig>,
    services: Services,
    http: Arc<serenity::Http>,
    bot_user_id: String,
    bot_display_name: String,
    guild_ids: Vec<String>,
) {
    let behavior_every = Duration::from_secs(config.bot.worker_interval_seconds.max(15) as u64);
    let behavior_config = config.clone();
    let behavior_services = services.clone();
    let behavior_http = http.clone();
    let behavior_bot_user_id = bot_user_id.clone();
    let behavior_bot_display_name = bot_display_name.clone();
    let behavior_guild_ids = guild_ids.clone();
    tokio::spawn(async move {
        info!(
            worker_seconds = behavior_every.as_secs(),
            guilds = behavior_guild_ids.len(),
            bot_user_id = behavior_bot_user_id,
            "starting rat behavior worker"
        );

        let mut interval = tokio::time::interval(behavior_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            for guild_id in &behavior_guild_ids {
                if behavior_config.bot.auto_claim {
                    match behavior_services
                        .users
                        .claim(
                            guild_id,
                            &behavior_bot_user_id,
                            &behavior_bot_display_name,
                        )
                        .await
                    {
                        Ok(receipt) => info!(
                            guild_id,
                            amount = receipt.amount_mana,
                            balance = receipt.balance_mana,
                            "bot auto-claimed its periodic payout"
                        ),
                        Err(crate::error::AppError::Conflict(_)) => {}
                        Err(error) => error!(guild_id, %error, "bot auto-claim failed"),
                    }
                }

                if behavior_config.bot.auto_accept_loans {
                    match behavior_services
                        .social
                        .auto_accept_eligible_loans(
                            guild_id,
                            &behavior_bot_user_id,
                            &behavior_bot_display_name,
                            behavior_config.bot.max_loan_interest_bps,
                            behavior_config.bot.min_loan_duration_seconds,
                        )
                        .await
                    {
                        Ok(accepted) if accepted > 0 => info!(
                            guild_id,
                            accepted,
                            max_interest_bps = behavior_config.bot.max_loan_interest_bps,
                            min_duration_seconds = behavior_config.bot.min_loan_duration_seconds,
                            "bot auto-accepted eligible loans"
                        ),
                        Ok(_) => {}
                        Err(error) => error!(guild_id, %error, "bot auto-loan acceptance failed"),
                    }
                }

                let due_repayment = behavior_services
                    .social
                    .auto_repay_due_money_loans(
                        guild_id,
                        &behavior_bot_user_id,
                        behavior_config.bot.worker_interval_seconds + 5,
                    )
                    .await;
                match due_repayment {
                    Ok(summary) if summary.repaid_loans > 0 => {
                        info!(
                            guild_id,
                            repaid_loans = summary.repaid_loans,
                            total_paid_mana = summary.total_paid_mana,
                            "bot auto-repaid money loans"
                        );
                        if let Ok(parsed_guild_id) = guild_id.parse::<u64>() {
                            if let Err(error) = announce_loan_repayment(
                                &behavior_http,
                                &behavior_services,
                                behavior_config.as_ref(),
                                serenity::GuildId::new(parsed_guild_id),
                                "🤝 Rat Repaid Loans",
                                &summary,
                            )
                            .await
                            {
                                error!(guild_id, %error, "failed to announce bot loan repayment");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => error!(guild_id, %error, "bot auto-loan repayment failed"),
                }

                let defaulted_repayment = behavior_services
                    .social
                    .auto_repay_defaulted_money_loans(guild_id, &behavior_bot_user_id)
                    .await;
                match defaulted_repayment {
                    Ok(summary) if summary.repaid_loans > 0 => {
                        info!(
                            guild_id,
                            repaid_loans = summary.repaid_loans,
                            total_paid_mana = summary.total_paid_mana,
                            "bot repaid defaulted money loans"
                        );
                        if let Ok(parsed_guild_id) = guild_id.parse::<u64>() {
                            if let Err(error) = announce_loan_repayment(
                                &behavior_http,
                                &behavior_services,
                                behavior_config.as_ref(),
                                serenity::GuildId::new(parsed_guild_id),
                                "💸 Rat Cleared Defaulted Loans",
                                &summary,
                            )
                            .await
                            {
                                error!(guild_id, %error, "failed to announce bot defaulted-loan repayment");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => error!(guild_id, %error, "bot defaulted-loan repayment failed"),
                }

                if behavior_config.bot.auto_buy_bonds {
                    match behavior_services
                        .bonds
                        .auto_buy_eligible_bonds(
                            guild_id,
                            &behavior_bot_user_id,
                            &behavior_bot_display_name,
                            behavior_config.bot.min_bond_yield_bps,
                            behavior_config.bot.max_bond_yield_bps,
                            behavior_config.bot.min_bond_maturity_seconds,
                            behavior_config.bot.max_bond_maturity_seconds,
                            behavior_config.bot.max_bond_price_mana,
                            behavior_config.bot.max_bond_purchase_quantity,
                            behavior_config.bot.max_total_bond_exposure_mana,
                        )
                        .await
                    {
                        Ok(purchased) if purchased > 0 => info!(
                            guild_id,
                            purchased,
                            min_yield_bps = behavior_config.bot.min_bond_yield_bps,
                            max_yield_bps = behavior_config.bot.max_bond_yield_bps,
                            max_price_mana = behavior_config.bot.max_bond_price_mana,
                            "bot auto-bought eligible bonds"
                        ),
                        Ok(_) => {}
                        Err(error) => error!(guild_id, %error, "bot auto-bond buying failed"),
                    }
                }
            }
        }
    });

    let bond_every = Duration::from_secs(config.bonds.worker_interval_seconds.max(30) as u64);
    tokio::spawn(async move {
        info!(
            worker_seconds = bond_every.as_secs(),
            "starting bond maturity worker"
        );

        let mut interval = tokio::time::interval(bond_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            match services.bonds.mature_due_bonds().await {
                Ok(matured) => {
                    for issuance in matured {
                        info!(
                            issuance_id = issuance.issuance_id,
                            guild_id = issuance.guild_id,
                            title = issuance.title,
                            holders_paid = issuance.holders_paid,
                            total_paid = issuance.total_paid_mana,
                            issuer_refund = issuance.issuer_refund_mana,
                            "bond issuance matured"
                        );
                    }
                }
                Err(error) => error!(%error, "bond maturity worker failed"),
            }
        }
    });
}

#[instrument(skip(config, services, http))]
async fn poll_and_announce(
    config: &AppConfig,
    services: &Services,
    http: &Arc<serenity::Http>,
) -> Result<(), crate::error::AppError> {
    let announcements = services.markets.poll_manifold_resolutions().await?;
    for announcement in announcements {
        let channel = serenity::ChannelId::new(announcement.channel_id);
        let mut description = format!(
            "🛰️ **{}**\n**Market:** {}\n**Status:** {}",
            announcement.question,
            ui::market_id_line(announcement.market_id),
            ui::status_badge(announcement.status)
        );
        if let Some(winner) = announcement.winning_option.as_ref() {
            description.push_str(&format!(
                "\n**Winning option:** {} **{}**",
                ui::option_emoji(winner),
                winner
            ));
        }
        if announcement.total_payout > 0 {
            description.push_str(&format!(
                "\n**Total payout:** {}",
                ui::money(config, announcement.total_payout)
            ));
        }
        if let Some(source) = announcement.external_url.as_ref() {
            description.push_str(&format!("\n**Source:** {source}"));
        }

        let embed = serenity::CreateEmbed::new()
            .title(match announcement.status {
                crate::domain::market::MarketStatus::Settled => {
                    format!(
                        "🔔 Tracked Market Settled {}",
                        ui::market_id_line(announcement.market_id)
                    )
                }
                crate::domain::market::MarketStatus::Cancelled => {
                    format!(
                        "⚫ Tracked Market Cancelled {}",
                        ui::market_id_line(announcement.market_id)
                    )
                }
                crate::domain::market::MarketStatus::NeedsManualReview => {
                    format!(
                        "🟠 Tracked Market Needs Review {}",
                        ui::market_id_line(announcement.market_id)
                    )
                }
                _ => format!(
                    "🔄 Tracked Market Update {}",
                    ui::market_id_line(announcement.market_id)
                ),
            })
            .description(description)
            .color(ui::market_color(
                announcement.market_type,
                announcement.status,
            ));

        if let Err(error) = channel
            .send_message(http, serenity::CreateMessage::new().embed(embed))
            .await
        {
            error!(
                market_id = announcement.market_id,
                channel_id = announcement.channel_id,
                %error,
                "failed to send market resolution announcement"
            );
        }
    }
    Ok(())
}

#[instrument(skip(http, services, config, summary), fields(guild_id = %guild_id))]
async fn announce_loan_repayment(
    http: &Arc<serenity::Http>,
    services: &Services,
    config: &AppConfig,
    guild_id: serenity::GuildId,
    title: &str,
    summary: &crate::services::social_service::AutoRepaySummary,
) -> Result<(), crate::error::AppError> {
    let Some(channel_id) = pick_announcement_channel(http, services, config, guild_id).await? else {
        return Ok(());
    };

    let embed = serenity::CreateEmbed::new()
        .title(title)
        .description(format!(
            "**Loans repaid:** `{}`\n**Loan IDs:** {}\n**Total paid:** {}",
            summary.repaid_loans,
            summary
                .loan_ids
                .iter()
                .map(|id| format!("#{}", id))
                .collect::<Vec<_>>()
                .join(", "),
            ui::money(config, summary.total_paid_mana)
        ))
        .color(serenity::Colour::from_rgb(46, 204, 113));

    channel_id
        .send_message(http, serenity::CreateMessage::new().embed(embed))
        .await?;
    Ok(())
}

async fn pick_announcement_channel(
    http: &Arc<serenity::Http>,
    services: &Services,
    config: &AppConfig,
    guild_id: serenity::GuildId,
) -> Result<Option<serenity::ChannelId>, crate::error::AppError> {
    if let Some(channel_id) = preferred_named_channel(
        http,
        guild_id,
        &config.bot.startup_announcement_channel_name,
    )
    .await?
    {
        return Ok(Some(channel_id));
    }

    if let Some(channel_id) = preferred_named_channel(
        http,
        guild_id,
        &config.bot.startup_announcement_fallback_channel_name,
    )
    .await?
    {
        return Ok(Some(channel_id));
    }

    let fallback = services
        .markets
        .latest_channel_for_guild(&guild_id.to_string())
        .await?;
    Ok(fallback.map(serenity::ChannelId::new))
}

async fn preferred_named_channel(
    http: &Arc<serenity::Http>,
    guild_id: serenity::GuildId,
    channel_name: &str,
) -> Result<Option<serenity::ChannelId>, crate::error::AppError> {
    let channel_name = channel_name.trim();
    if channel_name.is_empty() {
        return Ok(None);
    }

    let channels = guild_id.channels(http).await?;
    Ok(channels
        .into_values()
        .find(|channel| {
            channel.kind == serenity::ChannelType::Text
                && channel.name.eq_ignore_ascii_case(channel_name)
        })
        .map(|channel| channel.id))
}
