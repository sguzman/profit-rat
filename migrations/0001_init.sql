CREATE TABLE IF NOT EXISTS users (
    discord_user_id TEXT PRIMARY KEY,
    display_name TEXT,
    balance_mana INTEGER NOT NULL DEFAULT 1000,
    total_claimed_mana INTEGER NOT NULL DEFAULT 0,
    last_claim_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS balance_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    discord_user_id TEXT NOT NULL,
    amount_mana INTEGER NOT NULL,
    reason TEXT NOT NULL,
    related_market_id INTEGER,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS markets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    creator_discord_user_id TEXT NOT NULL,
    question TEXT NOT NULL,
    status TEXT NOT NULL,
    market_type TEXT NOT NULL DEFAULT 'native',
    liquidity_b REAL NOT NULL DEFAULT 100.0,
    close_time TEXT,
    resolved_option_id INTEGER,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    updated_at TEXT NOT NULL,
    external_source TEXT,
    external_id TEXT,
    external_url TEXT,
    external_slug TEXT,
    last_external_sync_at TEXT,
    external_status TEXT,
    external_resolution TEXT
);

CREATE TABLE IF NOT EXISTS market_options (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    label TEXT NOT NULL,
    shares_outstanding REAL NOT NULL DEFAULT 0.0,
    sort_order INTEGER NOT NULL,
    external_option_id TEXT,
    external_probability REAL,
    FOREIGN KEY (market_id) REFERENCES markets(id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_market_options_unique
    ON market_options (market_id, label);

CREATE TABLE IF NOT EXISTS positions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    option_id INTEGER NOT NULL,
    discord_user_id TEXT NOT NULL,
    shares REAL NOT NULL DEFAULT 0.0,
    total_spent_mana INTEGER NOT NULL DEFAULT 0,
    total_received_mana INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    UNIQUE(market_id, option_id, discord_user_id),
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id)
);

CREATE TABLE IF NOT EXISTS trades (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    option_id INTEGER NOT NULL,
    discord_user_id TEXT NOT NULL,
    side TEXT NOT NULL,
    mana_amount INTEGER NOT NULL,
    shares_delta REAL NOT NULL,
    price_before REAL NOT NULL,
    price_after REAL NOT NULL,
    external_price_at_trade REAL,
    external_snapshot_id INTEGER,
    created_at TEXT NOT NULL,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id),
    FOREIGN KEY (external_snapshot_id) REFERENCES external_market_snapshots(id)
);

CREATE TABLE IF NOT EXISTS external_market_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    external_source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    probability REAL,
    raw_status TEXT,
    raw_resolution TEXT,
    raw_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    FOREIGN KEY (market_id) REFERENCES markets(id)
);
