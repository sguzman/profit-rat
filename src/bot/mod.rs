pub mod commands;

use tracing::{error, info, instrument};

use crate::error::AppError;
use crate::services::Services;

pub type Context<'a> = poise::Context<'a, Data, AppError>;

#[derive(Clone)]
pub struct Data {
    pub services: Services,
}

pub fn build_framework(services: Services) -> poise::Framework<Data, AppError> {
    let options = poise::FrameworkOptions {
        commands: commands::all(),
        pre_command: |ctx| {
            Box::pin(async move {
                info!(
                    command = %ctx.command().qualified_name,
                    user_id = %ctx.author().id,
                    "received command"
                );
            })
        },
        post_command: |ctx| {
            Box::pin(async move {
                info!(
                    command = %ctx.command().qualified_name,
                    user_id = %ctx.author().id,
                    "finished command"
                );
            })
        },
        on_error: |error| Box::pin(handle_error(error)),
        ..Default::default()
    };

    poise::Framework::builder()
        .options(options)
        .setup(move |ctx, ready, framework| {
            let services = services.clone();
            Box::pin(async move {
                info!(bot_user = %ready.user.name, "discord bot connected");
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data { services })
            })
        })
        .build()
}

#[instrument(skip(error))]
async fn handle_error(error: poise::FrameworkError<'_, Data, AppError>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!(%error, command = %ctx.command().qualified_name, "command failed");
            if let Err(reply_error) = ctx.say(format!("Rat sabotage detected: {error}")).await {
                error!(%reply_error, "failed to send command error message");
            }
        }
        other => {
            error!(details = %other, "framework error");
        }
    }
}

pub fn display_name(user: &poise::serenity_prelude::User) -> String {
    user.global_name
        .clone()
        .unwrap_or_else(|| user.name.clone())
}
