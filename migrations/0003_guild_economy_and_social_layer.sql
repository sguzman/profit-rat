CREATE TABLE IF NOT EXISTS guild_accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    discord_user_id TEXT NOT NULL,
    display_name TEXT,
    balance_mana INTEGER NOT NULL DEFAULT 0,
    total_claimed_mana INTEGER NOT NULL DEFAULT 0,
    last_claim_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(guild_id, discord_user_id)
);

CREATE INDEX IF NOT EXISTS idx_guild_accounts_guild_balance
    ON guild_accounts (guild_id, balance_mana DESC, discord_user_id ASC);

CREATE TABLE IF NOT EXISTS economy_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    discord_user_id TEXT NOT NULL,
    related_market_id INTEGER,
    related_option_id INTEGER,
    asset_type TEXT NOT NULL,
    amount_mana INTEGER,
    amount_shares REAL,
    reason TEXT NOT NULL,
    note TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (related_market_id) REFERENCES markets(id),
    FOREIGN KEY (related_option_id) REFERENCES market_options(id)
);

CREATE TABLE IF NOT EXISTS asset_offers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    market_id INTEGER,
    option_id INTEGER,
    sender_discord_user_id TEXT NOT NULL,
    recipient_discord_user_id TEXT NOT NULL,
    quantity_mana INTEGER,
    quantity_shares REAL,
    price_mana INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    responded_at TEXT,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id)
);

CREATE INDEX IF NOT EXISTS idx_asset_offers_recipient_status
    ON asset_offers (guild_id, recipient_discord_user_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_asset_offers_sender_status
    ON asset_offers (guild_id, sender_discord_user_id, status, expires_at);

CREATE TABLE IF NOT EXISTS loans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    market_id INTEGER,
    option_id INTEGER,
    lender_discord_user_id TEXT NOT NULL,
    borrower_discord_user_id TEXT NOT NULL,
    principal_mana INTEGER,
    principal_shares REAL,
    repayment_mana INTEGER,
    repayment_shares REAL,
    interest_bps INTEGER NOT NULL DEFAULT 0,
    due_at TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    accepted_at TEXT,
    responded_at TEXT,
    closed_at TEXT,
    expires_at TEXT NOT NULL,
    FOREIGN KEY (market_id) REFERENCES markets(id),
    FOREIGN KEY (option_id) REFERENCES market_options(id)
);

CREATE INDEX IF NOT EXISTS idx_loans_borrower_status
    ON loans (guild_id, borrower_discord_user_id, status, due_at);

CREATE INDEX IF NOT EXISTS idx_loans_lender_status
    ON loans (guild_id, lender_discord_user_id, status, due_at);

CREATE TABLE IF NOT EXISTS loan_repayments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    loan_id INTEGER NOT NULL,
    guild_id TEXT NOT NULL,
    payer_discord_user_id TEXT NOT NULL,
    amount_mana INTEGER,
    amount_shares REAL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (loan_id) REFERENCES loans(id)
);
