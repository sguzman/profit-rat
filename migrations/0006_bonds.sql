CREATE TABLE IF NOT EXISTS bond_issuances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    issuer_discord_user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    price_per_bond_mana INTEGER NOT NULL,
    payout_per_bond_mana INTEGER NOT NULL,
    total_bonds INTEGER NOT NULL,
    remaining_bonds INTEGER NOT NULL,
    escrow_reserved_mana INTEGER NOT NULL,
    escrow_remaining_mana INTEGER NOT NULL,
    yield_bps INTEGER NOT NULL,
    yield_period_seconds INTEGER NOT NULL,
    matures_at TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bond_issuances_guild_status
    ON bond_issuances (guild_id, status, matures_at);

CREATE INDEX IF NOT EXISTS idx_bond_issuances_issuer_status
    ON bond_issuances (guild_id, issuer_discord_user_id, status, matures_at);

CREATE TABLE IF NOT EXISTS bond_positions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    issuance_id INTEGER NOT NULL,
    guild_id TEXT NOT NULL,
    holder_discord_user_id TEXT NOT NULL,
    bonds_owned INTEGER NOT NULL,
    total_spent_mana INTEGER NOT NULL DEFAULT 0,
    total_redeemed_mana INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (issuance_id, holder_discord_user_id),
    FOREIGN KEY (issuance_id) REFERENCES bond_issuances(id)
);

CREATE INDEX IF NOT EXISTS idx_bond_positions_holder
    ON bond_positions (guild_id, holder_discord_user_id, issuance_id);
