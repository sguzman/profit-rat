mod bot;
mod config;
mod db;
mod domain;
mod error;
mod integrations;
mod logging;
mod services;

use std::sync::Arc;

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

    info!("bootstrapping profit-rat");
    config.validate_for_runtime()?;
    let pool = db::connect(&config).await?;
    let manifold = Arc::new(integrations::manifold::ManifoldClient::new(
        config.manifold_api_base_url.clone(),
    ));
    let services = services::Services::new(config.clone(), pool, manifold);
    let framework = bot::build_framework(config, services);

    framework.start().await?;
    Ok(())
}
