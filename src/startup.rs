use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::bot;
use crate::config::AppConfig;
use crate::error::AppResult;
use crate::jobs;
use crate::services::Services;

const COMMAND_MANIFEST_FILE: &str = "command-manifest.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CommandManifest {
    version: String,
    commands: Vec<String>,
    recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandDiff {
    previous_version: Option<String>,
    current_version: String,
    added_commands: Vec<String>,
    removed_commands: Vec<String>,
    changed: bool,
    first_boot: bool,
}

#[instrument(skip(ctx, ready, commands, config, services), fields(guild_count = ready.guilds.len()))]
pub async fn sync_commands_and_announce(
    ctx: &serenity::Context,
    ready: &serenity::Ready,
    commands: &[poise::Command<bot::Data, crate::error::AppError>],
    config: &AppConfig,
    services: &Services,
) -> AppResult<()> {
    let guild_ids: Vec<serenity::GuildId> = ready.guilds.iter().map(|guild| guild.id).collect();
    jobs::spawn_bot_behavior_jobs(
        Arc::new(config.clone()),
        services.clone(),
        ready.user.id.to_string(),
        ready.user.global_name.clone().unwrap_or_else(|| ready.user.name.clone()),
        guild_ids.iter().map(ToString::to_string).collect(),
    );

    for guild_id in &guild_ids {
        poise::builtins::register_in_guild(ctx, commands, *guild_id).await?;
        info!(guild_id = %guild_id, "registered commands in guild for immediate sync");
    }

    poise::builtins::register_globally(ctx, commands).await?;
    info!("registered commands globally for eventual consistency");

    let current_manifest = CommandManifest::from_commands(commands);
    let diff = load_and_store_manifest(config, &current_manifest)?;
    if !diff.changed {
        info!(version = %diff.current_version, "command manifest unchanged after sync");
        return Ok(());
    }

    info!(
        version = %diff.current_version,
        added = diff.added_commands.len(),
        removed = diff.removed_commands.len(),
        first_boot = diff.first_boot,
        "command manifest changed after sync"
    );

    for guild_id in guild_ids {
        if let Some(channel_id) = pick_announcement_channel(ctx, services, config, guild_id).await?
        {
            if let Err(error) = announce_command_changes(ctx, channel_id, &diff).await {
                warn!(guild_id = %guild_id, %error, "failed to announce command changes");
            }
        } else {
            warn!(guild_id = %guild_id, "skipping startup command announcement because no suitable channel was found");
        }
    }

    Ok(())
}

impl CommandManifest {
    fn from_commands(commands: &[poise::Command<bot::Data, crate::error::AppError>]) -> Self {
        let mut names = BTreeSet::new();
        for command in commands {
            collect_command_names(command, &mut names);
        }

        Self {
            version: CURRENT_VERSION.to_string(),
            commands: names.into_iter().collect(),
            recorded_at: Utc::now().to_rfc3339(),
        }
    }
}

impl CommandDiff {
    fn unchanged(current_version: &str) -> Self {
        Self {
            previous_version: Some(current_version.to_string()),
            current_version: current_version.to_string(),
            added_commands: Vec::new(),
            removed_commands: Vec::new(),
            changed: false,
            first_boot: false,
        }
    }
}

fn collect_command_names(
    command: &poise::Command<bot::Data, crate::error::AppError>,
    names: &mut BTreeSet<String>,
) {
    if command.slash_action.is_some() {
        names.insert(format!("/{}", command.name));
    }
    if let Some(context_menu_name) = command.context_menu_name.as_ref() {
        names.insert(context_menu_name.clone());
    }
    for subcommand in &command.subcommands {
        collect_command_names(subcommand, names);
    }
}

fn load_and_store_manifest(
    config: &AppConfig,
    current: &CommandManifest,
) -> AppResult<CommandDiff> {
    let path = manifest_path(config);
    let previous = if path.exists() {
        let contents = fs::read_to_string(&path)?;
        Some(serde_json::from_str::<CommandManifest>(&contents)?)
    } else {
        None
    };

    fs::write(&path, serde_json::to_vec_pretty(current)?)?;

    Ok(match previous {
        Some(previous) => {
            let previous_commands: BTreeSet<_> = previous.commands.into_iter().collect();
            let current_commands: BTreeSet<_> = current.commands.iter().cloned().collect();
            let added_commands = current_commands
                .difference(&previous_commands)
                .cloned()
                .collect::<Vec<_>>();
            let removed_commands = previous_commands
                .difference(&current_commands)
                .cloned()
                .collect::<Vec<_>>();
            let changed = previous.version != current.version
                || !added_commands.is_empty()
                || !removed_commands.is_empty();

            if !changed {
                CommandDiff::unchanged(&current.version)
            } else {
                CommandDiff {
                    previous_version: Some(previous.version),
                    current_version: current.version.clone(),
                    added_commands,
                    removed_commands,
                    changed: true,
                    first_boot: false,
                }
            }
        }
        None => CommandDiff {
            previous_version: None,
            current_version: current.version.clone(),
            added_commands: current.commands.clone(),
            removed_commands: Vec::new(),
            changed: true,
            first_boot: true,
        },
    })
}

fn manifest_path(config: &AppConfig) -> PathBuf {
    config.cache_dir.join(COMMAND_MANIFEST_FILE)
}

async fn pick_announcement_channel(
    ctx: &serenity::Context,
    services: &Services,
    config: &AppConfig,
    guild_id: serenity::GuildId,
) -> AppResult<Option<serenity::ChannelId>> {
    if let Some(channel_id) = preferred_named_channel(
        ctx,
        guild_id,
        &config.bot.startup_announcement_channel_name,
    )
    .await?
    {
        return Ok(Some(channel_id));
    }

    if let Some(channel_id) = preferred_named_channel(
        ctx,
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
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    channel_name: &str,
) -> AppResult<Option<serenity::ChannelId>> {
    let channel_name = channel_name.trim();
    if channel_name.is_empty() {
        return Ok(None);
    }

    let channels = guild_id.channels(ctx).await?;
    Ok(channels
        .into_values()
        .find(|channel| {
            channel.kind == serenity::ChannelType::Text
                && channel.name.eq_ignore_ascii_case(channel_name)
        })
        .map(|channel| channel.id))
}

#[instrument(skip(ctx, diff), fields(channel_id = %channel_id))]
async fn announce_command_changes(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    diff: &CommandDiff,
) -> AppResult<()> {
    let title = if diff.first_boot {
        "Profit Rat is online"
    } else {
        "Profit Rat command update"
    };

    let mut lines = vec![format!("**Version:** `{}`", diff.current_version)];
    if let Some(previous_version) = diff.previous_version.as_ref() {
        if previous_version != &diff.current_version {
            lines.push(format!("**Previous version:** `{previous_version}`"));
        }
    }

    if diff.first_boot {
        lines.push(format!(
            "Synced **{}** slash commands for this server.",
            diff.added_commands.len()
        ));
        lines.push("Use `/list_commands` or `/tutorial` to explore the bot.".to_string());
    } else {
        if !diff.added_commands.is_empty() {
            lines.push(format!(
                "**New commands:** {}",
                diff.added_commands.join(", ")
            ));
        }
        if !diff.removed_commands.is_empty() {
            lines.push(format!(
                "**Removed commands:** {}",
                diff.removed_commands.join(", ")
            ));
        }
        lines.push("Use `/list_commands` to see the refreshed catalog.".to_string());
    }

    let embed = serenity::CreateEmbed::new()
        .title(title)
        .description(lines.join("\n"))
        .color(serenity::Colour::from_rgb(241, 196, 15));

    channel_id
        .send_message(ctx, serenity::CreateMessage::new().embed(embed))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CommandManifest, load_and_store_manifest};
    use crate::config::{
        AppConfig, BondPolicyConfig, BotPolicyConfig, CurrencyConfig, CurrencyPosition,
        LoanPolicyConfig, ManifoldConfig, NegativeStyle, PolicyConfig, TransferPolicyConfig,
    };
    use std::sync::Arc;

    fn test_config(cache_dir: std::path::PathBuf) -> Arc<AppConfig> {
        Arc::new(AppConfig {
            discord_token: "token".to_string(),
            cache_dir: cache_dir.clone(),
            log_dir: cache_dir.join("logs"),
            database_path: cache_dir.join("discord-bot.sqlite"),
            database_url: format!(
                "sqlite://{}",
                cache_dir.join("discord-bot.sqlite").display()
            ),
            policies: PolicyConfig {
                starting_balance: 1_000,
                claim_amount: 10_000,
                claim_period_seconds: 43_200,
                claim_period_name: "twice-daily login".to_string(),
                default_liquidity_b: 100.0,
                share_offer_expiration_seconds: 60,
                share_offer_cleanup_interval_seconds: 15,
            },
            transfers: TransferPolicyConfig {
                allow_money_donations: true,
                allow_share_donations: true,
                allow_money_offers: true,
                allow_share_offers: true,
                min_money_transfer: 1,
                min_share_transfer: 0.01,
                max_open_offers_per_user: 25,
            },
            loans: LoanPolicyConfig {
                allow_money_loans: true,
                allow_share_loans: true,
                allow_partial_repayment: true,
                allow_early_repayment: true,
                allow_interest: true,
                default_interest_bps: 0,
                max_interest_bps: 2_500,
                default_duration_seconds: 86_400,
                max_duration_seconds: 2_592_000,
                max_open_loans_per_user: 10,
            },
            bot: BotPolicyConfig {
                auto_claim: true,
                auto_accept_loans: true,
                startup_announcement_channel_name: "bots".to_string(),
                startup_announcement_fallback_channel_name: "general".to_string(),
                max_loan_interest_bps: 500,
                min_loan_duration_seconds: 3_600,
                auto_buy_bonds: true,
                min_bond_yield_bps: 100,
                max_bond_yield_bps: 500,
                min_bond_maturity_seconds: 3_600,
                max_bond_maturity_seconds: 86_400,
                max_bond_price_mana: 5_000,
                max_bond_purchase_quantity: 1,
                max_total_bond_exposure_mana: 20_000,
                worker_interval_seconds: 60,
            },
            bonds: BondPolicyConfig {
                enabled: true,
                default_yield_period_seconds: 3_600,
                max_yield_bps: 5_000,
                min_maturity_seconds: 3_600,
                max_maturity_seconds: 7_776_000,
                max_open_issuances_per_user: 10,
                worker_interval_seconds: 60,
            },
            manifold: ManifoldConfig {
                api_base_url: "https://api.manifold.markets/v0".to_string(),
                snapshot_ttl_seconds: 60,
                poll_interval_seconds: 120,
            },
            currency: CurrencyConfig {
                code: "MANA".to_string(),
                display_name: "Fake Mana".to_string(),
                singular: "mana".to_string(),
                plural: "mana".to_string(),
                symbol: String::new(),
                textual_symbol: "mana".to_string(),
                emoji: "money".to_string(),
                custom_emoji: String::new(),
                image_symbol_path: String::new(),
                image_symbol_url: String::new(),
                position: CurrencyPosition::Suffix,
                space_between: true,
                show_symbol: false,
                show_textual_symbol: true,
                show_code: false,
                use_emoji_in_embeds: true,
                use_emoji_in_plaintext: false,
                decimals: 0,
                thousands_separator: ",".to_string(),
                negative_style: NegativeStyle::Minus,
                short_suffixes: true,
            },
            starting_balance: 1_000,
            claim_amount: 10_000,
            claim_period_seconds: 43_200,
            claim_period_name: "twice-daily login".to_string(),
            default_liquidity_b: 100.0,
            manifold_api_base_url: "https://api.manifold.markets/v0".to_string(),
            manifold_snapshot_ttl_seconds: 60,
            manifold_poll_interval_seconds: 120,
            share_offer_expiration_seconds: 60,
            share_offer_cleanup_interval_seconds: 15,
        })
    }

    #[test]
    fn manifest_diff_detects_added_commands() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = test_config(temp.path().join(".cache"));
        std::fs::create_dir_all(&config.cache_dir).expect("cache dir");

        let initial = CommandManifest {
            version: "0.1.0".to_string(),
            commands: vec!["/ping".to_string()],
            recorded_at: "2026-06-23T00:00:00Z".to_string(),
        };
        let diff = load_and_store_manifest(config.as_ref(), &initial).expect("first write");
        assert!(diff.changed);
        assert!(diff.first_boot);

        let updated = CommandManifest {
            version: "0.1.1".to_string(),
            commands: vec!["/balance".to_string(), "/ping".to_string()],
            recorded_at: "2026-06-23T00:05:00Z".to_string(),
        };
        let diff = load_and_store_manifest(config.as_ref(), &updated).expect("second write");
        assert!(diff.changed);
        assert_eq!(diff.previous_version.as_deref(), Some("0.1.0"));
        assert_eq!(diff.added_commands, vec!["/balance".to_string()]);
        assert!(diff.removed_commands.is_empty());
    }

    #[test]
    fn manifest_diff_stays_quiet_when_only_timestamp_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = test_config(temp.path().join(".cache"));
        std::fs::create_dir_all(&config.cache_dir).expect("cache dir");

        let initial = CommandManifest {
            version: "0.1.0".to_string(),
            commands: vec!["/balance".to_string(), "/ping".to_string()],
            recorded_at: "2026-06-23T00:00:00Z".to_string(),
        };
        load_and_store_manifest(config.as_ref(), &initial).expect("first write");

        let updated = CommandManifest {
            version: "0.1.0".to_string(),
            commands: vec!["/balance".to_string(), "/ping".to_string()],
            recorded_at: "2026-06-23T00:10:00Z".to_string(),
        };
        let diff = load_and_store_manifest(config.as_ref(), &updated).expect("second write");
        assert!(!diff.changed);
        assert!(!diff.first_boot);
        assert!(diff.added_commands.is_empty());
        assert!(diff.removed_commands.is_empty());
    }
}
