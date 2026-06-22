use sqlx::Row;
use tracing::{info, instrument};

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::error::{AppError, AppResult};

pub async fn maybe_run_from_args(config: &AppConfig, pool: &DbPool) -> AppResult<bool> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(false);
    }

    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" || flag == "help" => {
            print_global_help();
            Ok(true)
        }
        [group, flag]
            if group == "guilds" && (flag == "--help" || flag == "-h" || flag == "help") =>
        {
            print_guild_help();
            Ok(true)
        }
        [group, action] if group == "guilds" && action == "list" => {
            list_guilds(pool).await?;
            Ok(true)
        }
        [group, action, guild_id, confirm]
            if group == "guilds" && action == "delete" && confirm == "--confirm" =>
        {
            delete_guild(pool, guild_id).await?;
            println!("Deleted guild economy `{guild_id}` from `{}`.", config.database_path.display());
            Ok(true)
        }
        [group, action, confirm]
            if group == "guilds" && action == "delete-all" && confirm == "--confirm" =>
        {
            delete_all_guilds(pool).await?;
            println!(
                "Deleted all guild economies from `{}`.",
                config.database_path.display()
            );
            Ok(true)
        }
        [group, action, ..] if group == "guilds" && action == "delete" => Err(AppError::Validation(
            "refusing to delete a guild without `--confirm`.\nusage: profit-rat guilds delete <guild_id> --confirm".to_string(),
        )),
        [group, action, ..] if group == "guilds" && action == "delete-all" => Err(
            AppError::Validation(
                "refusing to delete all guild data without `--confirm`.\nusage: profit-rat guilds delete-all --confirm".to_string(),
            ),
        ),
        [group, ..] if group == "guilds" => Err(AppError::Validation(guild_help_text())),
        _ => Ok(false),
    }
}

fn print_global_help() {
    println!("Profit Rat CLI");
    println!();
    println!("Usage:");
    println!("  profit-rat guilds list");
    println!("  profit-rat guilds delete <guild_id> --confirm");
    println!("  profit-rat guilds delete-all --confirm");
    println!("  profit-rat guilds --help");
}

fn print_guild_help() {
    println!("{}", guild_help_text());
}

fn guild_help_text() -> String {
    "Guild admin usage:\n  profit-rat guilds list\n  profit-rat guilds delete <guild_id> --confirm\n  profit-rat guilds delete-all --confirm".to_string()
}

#[instrument(skip(pool))]
async fn list_guilds(pool: &DbPool) -> AppResult<()> {
    let rows = sqlx::query(
        "WITH guilds AS (
            SELECT guild_id FROM guild_accounts
            UNION
            SELECT guild_id FROM markets
            UNION
            SELECT guild_id FROM economy_events
            UNION
            SELECT guild_id FROM asset_offers
            UNION
            SELECT guild_id FROM loans
         )
         SELECT
            g.guild_id,
            (SELECT COUNT(*) FROM guild_accounts ga WHERE ga.guild_id = g.guild_id) AS accounts,
            (SELECT COUNT(*) FROM markets m WHERE m.guild_id = g.guild_id) AS markets,
            (SELECT COUNT(*) FROM positions p JOIN markets m ON m.id = p.market_id WHERE m.guild_id = g.guild_id) AS positions,
            (SELECT COUNT(*) FROM trades t JOIN markets m ON m.id = t.market_id WHERE m.guild_id = g.guild_id) AS trades,
            (SELECT COUNT(*) FROM asset_offers ao WHERE ao.guild_id = g.guild_id) AS asset_offers,
            (SELECT COUNT(*) FROM share_transfer_offers so JOIN markets m ON m.id = so.market_id WHERE m.guild_id = g.guild_id) AS legacy_share_offers,
            (SELECT COUNT(*) FROM loans l WHERE l.guild_id = g.guild_id) AS loans,
            (SELECT COUNT(*) FROM economy_events ee WHERE ee.guild_id = g.guild_id) AS economy_events
         FROM guilds g
         ORDER BY g.guild_id ASC",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        println!("No guild economies are present.");
        return Ok(());
    }

    println!("Present guild economies:");
    for row in rows {
        println!(
            "- guild_id={} accounts={} markets={} positions={} trades={} asset_offers={} legacy_share_offers={} loans={} economy_events={}",
            row.get::<String, _>("guild_id"),
            row.get::<i64, _>("accounts"),
            row.get::<i64, _>("markets"),
            row.get::<i64, _>("positions"),
            row.get::<i64, _>("trades"),
            row.get::<i64, _>("asset_offers"),
            row.get::<i64, _>("legacy_share_offers"),
            row.get::<i64, _>("loans"),
            row.get::<i64, _>("economy_events"),
        );
    }

    Ok(())
}

#[instrument(skip(pool), fields(guild_id))]
async fn delete_guild(pool: &DbPool, guild_id: &str) -> AppResult<()> {
    let exists = sqlx::query(
        "SELECT 1
         FROM (
            SELECT guild_id FROM guild_accounts
            UNION
            SELECT guild_id FROM markets
            UNION
            SELECT guild_id FROM economy_events
            UNION
            SELECT guild_id FROM asset_offers
            UNION
            SELECT guild_id FROM loans
         )
         WHERE guild_id = ?1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await?
    .is_some();

    if !exists {
        return Err(AppError::NotFound(format!(
            "guild economy `{guild_id}` was not found"
        )));
    }

    let mut tx = pool.begin().await?;
    delete_guild_in_tx(&mut tx, guild_id).await?;
    tx.commit().await?;
    info!(guild_id, "deleted guild economy");
    Ok(())
}

#[instrument(skip(pool))]
async fn delete_all_guilds(pool: &DbPool) -> AppResult<()> {
    let guild_rows = sqlx::query(
        "SELECT guild_id
         FROM (
            SELECT guild_id FROM guild_accounts
            UNION
            SELECT guild_id FROM markets
            UNION
            SELECT guild_id FROM economy_events
            UNION
            SELECT guild_id FROM asset_offers
            UNION
            SELECT guild_id FROM loans
         )
         ORDER BY guild_id ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    for row in guild_rows {
        let guild_id: String = row.get("guild_id");
        delete_guild_in_tx(&mut tx, &guild_id).await?;
    }
    tx.commit().await?;
    info!("deleted all guild economies");
    Ok(())
}

async fn delete_guild_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    guild_id: &str,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM loan_repayments
         WHERE guild_id = ?1
            OR loan_id IN (SELECT id FROM loans WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query("DELETE FROM economy_events WHERE guild_id = ?1")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM asset_offers WHERE guild_id = ?1")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM loans WHERE guild_id = ?1")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        "DELETE FROM balance_events
         WHERE related_market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "DELETE FROM share_transfer_offers
         WHERE market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "DELETE FROM trades
         WHERE market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "DELETE FROM positions
         WHERE market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "DELETE FROM external_market_snapshots
         WHERE market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "DELETE FROM market_options
         WHERE market_id IN (SELECT id FROM markets WHERE guild_id = ?1)",
    )
    .bind(guild_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query("DELETE FROM markets WHERE guild_id = ?1")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM guild_accounts WHERE guild_id = ?1")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
