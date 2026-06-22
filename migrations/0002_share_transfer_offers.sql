CREATE TABLE IF NOT EXISTS share_transfer_offers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    market_id INTEGER NOT NULL,
    option_id INTEGER NOT NULL,
    seller_discord_user_id TEXT NOT NULL,
    buyer_discord_user_id TEXT NOT NULL,
    shares REAL NOT NULL,
    price_mana INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    responded_at TEXT,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id)
);

CREATE INDEX IF NOT EXISTS idx_share_transfer_offers_buyer_status
    ON share_transfer_offers (buyer_discord_user_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_share_transfer_offers_seller_status
    ON share_transfer_offers (seller_discord_user_id, status, expires_at);
