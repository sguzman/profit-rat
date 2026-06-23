# Profit Rat

A fun Rust Discord bot for fake-money prediction markets.

Profit Rat lets each Discord server run its own isolated paper economy with native markets, Manifold-tracked mirror markets, donations, share transfers, and social loans. No real money is involved, and Manifold integration is price-oracle and resolution-oracle only.

## What It Does

- Runs a separate economy per Discord server
- Lets users claim fake currency, trade positions, and climb a leaderboard
- Supports native multi-option markets with LMSR pricing
- Supports tracked Manifold mirror markets with cached snapshots and sync-based settlement
- Supports direct money donations and share donations
- Supports peer-to-peer share offers
- Supports money loans and share loans with acceptance, repayment, expiry, and default tracking
- Can generate chart images directly in Discord for prices, holder concentration, exposure, and price history
- Stores runtime state in SQLite under `.cache/`
- Writes structured logs under `.cache/logs/`

## Invite URL

Use this invite URL to add the bot to a server:

[Invite Profit Rat](https://discord.com/oauth2/authorize?client_id=1518414103859695647&permissions=2147567616&integration_type=0&scope=bot+applications.commands)

Notes:
- To add the bot to a server, the person using the invite link usually needs `Manage Server` or administrator-level permission in that server.
- To restrict the bot to one channel like `#market`, configure Discord channel permissions on the server side.

## Current Command Surface

### Help and Discovery

- `/help`
- `/tutorial`
- `/list_commands`
- `/ping`

### Economy

- `/balance`
- `/claim`
- `/leaderboard`

### Markets

- `/create_market`
- `/markets`
- `/list_markets`
- `/market`
- `/market_holders`
- `/resolve_market`

### Trading

- `/buy`
- `/sell`
- `/positions`
- `/mpositions`

### Charts

- `/histogram_prices`
- `/histogram_holders`
- `/histogram_position`
- `/histogram_time`

### Share Offers

- `/offer_shares`
- `/incoming_share_offers`
- `/accept_share_offer`
- `/decline_share_offer`

### Donations

- `/donate_money`
- `/donate_shares`

### Loans

- `/offer_loan_money`
- `/offer_loan_shares`
- `/incoming_loans`
- `/accept_loan`
- `/decline_loan`
- `/loan_status`
- `/repay_loan`

### Manifold Mirrors

- `/track_manifold`
- `/manifold_market`
- `/msync`

## Native vs. Manifold Markets

### Native Markets

- Profit Rat controls pricing
- Profit Rat controls resolution
- Buying and selling move price through LMSR

### Tracked Manifold Markets

- Manifold controls price
- Manifold controls resolution
- Profit Rat stores local paper positions only
- Buying and selling do not affect the external market

## Guild Isolation

Profit Rat is guild-scoped by default:

- The same Discord user gets a separate balance in each server
- Claim cooldowns are separate per server
- Leaderboards are separate per server
- Donations, positions, offers, and loans are separate per server
- One server cannot read or mutate another server's economy state

## Project Layout

```text
src/
  main.rs
  admin.rs
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

The roadmap lives in [docs/bootstrap-roadmap.md](docs/bootstrap-roadmap.md).

## Runtime Data

All mutable runtime data lives under `.cache/`:

- `.cache/discord-bot.sqlite`
- `.cache/logs/`
- database backups and runtime snapshots

This means presentation-only changes usually do not touch stored balances or markets.

## Configuration

Profit Rat is TOML-first.

### Main Config

The tracked base config lives in [profit-rat.toml](profit-rat.toml).

That file controls things like:

- starting balance
- faucet amount and cooldown
- liquidity defaults
- offer expiry
- loan policies
- currency formatting
- Manifold polling behavior

### Local Secrets

Use `.env` for the Discord token fallback:

```env
DISCORD_TOKEN=your_bot_token_here
```

Keep `.env` minimal. Policy and economy settings should live in `profit-rat.toml`.

You can also use an untracked `profit-rat.local.toml` for local overrides:

```toml
discord_token = "your_bot_token_here"
```

Optional path overrides:

- `PROFIT_RAT_CONFIG`
- `PROFIT_RAT_LOCAL_CONFIG`

## Setup

### 1. Create the Discord App

Go to the [Discord Developer Portal](https://discord.com/developers/applications), create an application, add a bot user, and generate a bot token.

### 2. Configure the Token

Put the token in `.env`:

```env
DISCORD_TOKEN=your_bot_token_here
```

### 3. Review `profit-rat.toml`

Adjust policy, currency, and runtime defaults there if you want custom behavior.

### 4. Run the Bot

```powershell
cargo run
```

Or:

```powershell
cargo run --release
```

On boot, the bot will:

- create `.cache/` if needed
- initialize logging
- apply migrations
- migrate legacy single-guild global users where possible
- connect to Discord
- register slash commands globally

## Guild Admin CLI

You can inspect or wipe guild-scoped data without starting the bot:

```powershell
cargo run --release -- guilds list
cargo run --release -- guilds delete <guild_id> --confirm
cargo run --release -- guilds delete-all --confirm
cargo run --release -- guilds --help
```

Notes:
- `guilds list` shows present guild economies in the SQLite DB
- `guilds delete` removes one guild economy
- `guilds delete-all` removes all guild economies
- destructive commands require `--confirm`

## Autocomplete

Autocomplete is wired for:

- market selection on market and trading commands
- option selection after market selection
- incoming share-offer selection
- incoming loan selection

In Discord, start typing in the slash command field to get the dropdown.

## Logging

Logs are written to `.cache/logs/` in structured JSON format.

They include:

- boot session information
- command entry and exit
- database and transaction activity
- market sync work
- settlement and background job activity
- errors and warnings

## Testing

Run:

```powershell
cargo fmt
cargo check
cargo test
```

Current tests cover:

- config defaults
- runtime directory creation
- fresh SQLite migrations
- LMSR pricing behavior
- claim cooldown enforcement
- Manifold slug parsing

## Current Status

Implemented:

- guild-isolated server economies
- balances, claims, and leaderboards
- native market creation, trading, and settlement
- Manifold tracking and sync-based settlement
- donations
- share offers
- money and share loans
- background expiry/default jobs
- guild admin CLI

Still open on the roadmap:

- generalized `asset_offers` replacing legacy share-offer storage
- money offers
- broader integration and migration coverage tests
