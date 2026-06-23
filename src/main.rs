mod admin;
mod bot;
mod config;
mod db;
mod domain;
mod error;
mod integrations;
mod jobs;
mod logging;
mod services;
mod startup;

use std::sync::Arc;

use poise::serenity_prelude as serenity;
use tracing::{info, info_span};
use uuid::Uuid;

use crate::error::AppResult;

#[tokio::main]
async fn main() -> AppResult<()> {
    let config = Arc::new(config::AppConfig::from_env()?);
    config.ensure_runtime_dirs()?;
    let _guards = logging::init(&config)?;
    let session_id = Uuid::new_v4();
    let span = info_span!("boot", %session_id);
    let _entered = span.enter();

    info!(
        database_path = %config.database_path.display(),
        cache_dir = %config.cache_dir.display(),
        log_dir = %config.log_dir.display(),
        "bootstrapping profit-rat"
    );
    let pool = db::connect(&config).await?;
    if admin::maybe_run_from_args(&config, &pool).await? {
        return Ok(());
    }

    config.validate_for_runtime()?;
    let discord_token = config.discord_token.clone();
    let manifold = Arc::new(integrations::manifold::ManifoldClient::new(
        config.manifold_api_base_url.clone(),
    ));
    let services = services::Services::new(config.clone(), pool, manifold);
    let framework = bot::build_framework(services.clone(), config.clone());
    let intents = serenity::GatewayIntents::non_privileged();
    let mut client = serenity::ClientBuilder::new(discord_token, intents)
        .framework(framework)
        .await?;
    jobs::spawn_background_jobs(config.clone(), services, client.http.clone());
    client.start().await?;
    Ok(())
}
