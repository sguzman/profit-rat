# Profit Rat

A silly Discord prediction market bot written in Rust.

Profit Rat lets a server create fake-money markets, trade positions, claim hourly mana, and mirror real Manifold markets as paper trades. Native markets use an internal market maker. Tracked Manifold markets use Manifold as a price and resolution oracle only. No real money is involved, and the bot never places real trades on Manifold.

## Features

- Native fake-money markets with binary or multi-option outcomes
- Hourly claim faucet and per-user fake balances
- `/buy`, `/sell`, `/positions`, `/balance`, and `/leaderboard`
- Manifold shadow markets with cached prices and sync-based settlement
- Structured JSON logging to `.cache/logs/`
- SQLite persistence under `.cache/`
- Slash-command autocomplete for markets and options

## Tech Stack

- Rust 2024
- [poise](https://crates.io/crates/poise) + Serenity for Discord slash commands
- Tokio for async runtime
- SQLx + SQLite for persistence
- Reqwest for Manifold API calls
- Tracing + tracing-subscriber for structured logging

## Project Layout

```text
src/
  main.rs
  config.rs
  error.rs
  bot/
  db/
  domain/
  integrations/
  jobs/
  logging.rs
  services/

migrations/
docs/
tmp/
```

The current roadmap lives in [docs/bootstrap-roadmap.md](docs/bootstrap-roadmap.md).

## Runtime Data

All mutable runtime data lives under `.cache/`:

- `.cache/discord-bot.sqlite`
- `.cache/logs/`
- any local snapshots and caches

This means normal code/UI changes should not touch your stored bot data unless you change schema or migration behavior.

## Commands

### Core

- `/ping`
- `/balance`
- `/balance user:@someone`
- `/claim`
- `/leaderboard`

### Native Markets

- `/create_market`
- `/markets`
- `/list_markets`
- `/market`
- `/market_holders`
- `/buy`
- `/sell`
- `/offer_shares`
- `/incoming_share_offers`
- `/accept_share_offer`
- `/decline_share_offer`
- `/positions`
- `/resolve_market`

### Manifold Mirrors

- `/track_manifold`
- `/manifold_market`
- `/msync`
- `/mpositions`

`/buy` and `/sell` are market-type aware. If you point them at a native market, they use internal LMSR-style pricing. If you point them at a tracked Manifold market, they use the latest cached external probability instead.

## Native vs. Manifold Markets

### Native

- Profit Rat controls pricing
- Profit Rat controls resolution
- Buying moves the market price

### Tracked Manifold

- Manifold controls pricing
- Manifold controls resolution
- Discord users make fake paper trades only
- Buying does not move the external market price

Tracked Manifold markets are meant to feel like paper trading on a live public market without needing users to place real bets.

## Setup

### 1. Create a Discord application

Go to the [Discord Developer Portal](https://discord.com/developers/applications), create an application, add a bot user, and copy the bot token.

You also need to invite the bot to your server with:

- `bot`
- `applications.commands`

Typical bot permissions for local testing are enough to:

- view channels
- send messages
- embed links
- use slash commands

### 2. Configure `profit-rat.toml`

The bot now reads its policies and runtime settings from [profit-rat.toml](profit-rat.toml).

For personal overrides and secrets, create an untracked `profit-rat.local.toml`:

```toml
discord_token = "your_bot_token_here"
```

You can also override the config file path with `PROFIT_RAT_CONFIG` or the local override path with `PROFIT_RAT_LOCAL_CONFIG`.

### 3. Optional `.env`

`DISCORD_TOKEN` still works as a fallback, so an `.env` like this is also valid:

```env
DISCORD_TOKEN=your_bot_token_here
RUST_LOG=profit_rat=debug,info
```

### 4. Run the bot

```powershell
cargo run
```

On boot, the bot will:

- create `.cache/` if missing
- initialize logging
- apply database migrations
- connect to Discord
- register slash commands

## Development Notes

### Command updates

The bot currently registers commands globally. Discord can take a little while to refresh global slash commands, so autocomplete or command-shape changes may not appear instantly after a restart.

### Autocomplete

Autocomplete is currently wired for:

- market selection on market/trading commands
- option selection after you choose a market for `/buy`, `/sell`, and `/resolve_market`
- incoming offer selection for `/accept_share_offer` and `/decline_share_offer`

In Discord, start a slash command, click into the field, and type a little. The dropdown appears while you type.

### Logging

Logs are written to `.cache/logs/` in structured JSON format. They include startup/session information, command activity, background sync work, and error details.

### Database safety

Runtime data is stored in SQLite under `.cache/discord-bot.sqlite`. Most presentation-only changes, like changing copy, emoji, or embed colors, do not affect stored balances or markets.

## Manifold URL Support

`/track_manifold` accepts:

- a full Manifold market URL
- a hostless Manifold-style path
- a raw Manifold contract ID

The bot normalizes that input, fetches the external market, stores a local mirror, and refreshes snapshots on demand or through the background poller.

## Share Offers

You can now sell shares directly to another user without going through the market maker.

Flow:

- seller runs `/offer_shares`
- buyer sees the pending offer with `/incoming_share_offers`
- buyer accepts with `/accept_share_offer` or declines with `/decline_share_offer`
- pending offers auto-expire after the configured timeout

The expiry window and cleanup interval live in `profit-rat.toml` under `[policies]`:

```toml
[policies]
share_offer_expiration_seconds = 60
share_offer_cleanup_interval_seconds = 15
```

## Testing

Run:

```powershell
cargo fmt
cargo check
cargo test
```

The current codebase includes tests for:

- config defaults and cache directory creation
- SQLite migrations on a fresh database
- native pricing behavior
- claim cooldown enforcement
- Manifold slug parsing

## Current Status

Bootstrap milestones are complete for:

- native fake-money markets
- balances and claims
- positions and leaderboard
- Manifold tracking and sync-based settlement
- background resolution polling

See [docs/bootstrap-roadmap.md](docs/bootstrap-roadmap.md) for the tracked roadmap with checkbox status.
