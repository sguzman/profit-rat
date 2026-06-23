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
