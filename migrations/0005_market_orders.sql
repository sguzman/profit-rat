CREATE TABLE IF NOT EXISTS market_orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    market_id INTEGER NOT NULL,
    option_id INTEGER NOT NULL,
    discord_user_id TEXT NOT NULL,
    side TEXT NOT NULL,
    quantity_shares REAL NOT NULL,
    trigger_price REAL NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    executed_at TEXT,
    cancelled_at TEXT,
    failure_note TEXT,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id)
);

CREATE INDEX IF NOT EXISTS idx_market_orders_market_status
    ON market_orders (market_id, status, side, trigger_price, id);

CREATE INDEX IF NOT EXISTS idx_market_orders_user_status
    ON market_orders (guild_id, discord_user_id, status, id);
