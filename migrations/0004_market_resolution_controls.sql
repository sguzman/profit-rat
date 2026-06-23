ALTER TABLE markets ADD COLUMN resolution_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE markets ADD COLUMN resolved_by_discord_user_id TEXT;

ALTER TABLE economy_events ADD COLUMN resolution_revision INTEGER;

CREATE TABLE IF NOT EXISTS market_resolvers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    discord_user_id TEXT NOT NULL,
    granted_by_discord_user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(market_id, discord_user_id),
    FOREIGN KEY (market_id) REFERENCES markets(id)
);

CREATE INDEX IF NOT EXISTS idx_market_resolvers_market
    ON market_resolvers (market_id, discord_user_id);

CREATE TABLE IF NOT EXISTS market_resolution_audits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    guild_id TEXT NOT NULL,
    actor_discord_user_id TEXT NOT NULL,
    action_type TEXT NOT NULL,
    from_status TEXT,
    to_status TEXT NOT NULL,
    previous_option_id INTEGER,
    new_option_id INTEGER,
    revision INTEGER NOT NULL,
    note TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (previous_option_id) REFERENCES market_options(id),
    FOREIGN KEY (new_option_id) REFERENCES market_options(id)
);
