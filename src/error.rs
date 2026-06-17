use std::num::TryFromIntError;

use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("discord configuration is missing `DISCORD_TOKEN`")]
    MissingDiscordToken,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("discord error: {0}")]
    Discord(#[from] poise::serenity_prelude::Error),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("number conversion error: {0}")]
    NumberConversion(#[from] TryFromIntError),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    External(String),
    #[error("unexpected error: {0}")]
    Other(#[from] anyhow::Error),
}

impl From<poise::FrameworkError<'_, crate::bot::Data, AppError>> for AppError {
    fn from(value: poise::FrameworkError<'_, crate::bot::Data, AppError>) -> Self {
        Self::Other(anyhow::anyhow!("{value}"))
    }
}
