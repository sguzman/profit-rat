CREATE TABLE IF NOT EXISTS bond_transfer_offers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    issuance_id INTEGER NOT NULL,
    seller_discord_user_id TEXT NOT NULL,
    buyer_discord_user_id TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    price_mana INTEGER NOT NULL,
    status TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    responded_at TEXT,
    FOREIGN KEY (issuance_id) REFERENCES bond_issuances(id)
);

CREATE INDEX IF NOT EXISTS idx_bond_transfer_offers_buyer_status
    ON bond_transfer_offers (guild_id, buyer_discord_user_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_bond_transfer_offers_seller_status
    ON bond_transfer_offers (guild_id, seller_discord_user_id, status, expires_at);
