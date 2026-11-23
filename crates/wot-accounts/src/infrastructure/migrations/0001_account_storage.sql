CREATE TABLE IF NOT EXISTS accounts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    role TEXT NOT NULL DEFAULT 'player' CHECK (role IN ('player', 'admin')),
    username TEXT NOT NULL UNIQUE CHECK (length(username) BETWEEN 1 AND 128),
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 128),
    password_hash TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    credits BIGINT NOT NULL DEFAULT 100000000 CHECK (credits >= 0),
    gold BIGINT NOT NULL DEFAULT 1000000 CHECK (gold >= 0),
    bonds BIGINT NOT NULL DEFAULT 0 CHECK (bonds >= 0),
    equipment_components BIGINT NOT NULL DEFAULT 0 CHECK (equipment_components >= 0),
    free_xp BIGINT NOT NULL DEFAULT 0 CHECK (free_xp >= 0),
    slots BIGINT NOT NULL DEFAULT 100 CHECK (slots >= 0),
    berths BIGINT NOT NULL DEFAULT 100 CHECK (berths >= 0),
    settings JSONB NOT NULL DEFAULT '{}',
    depot_shells JSONB NOT NULL DEFAULT '{}',
    progression JSONB NOT NULL DEFAULT '{"unlocks":[],"vehicle_xp":{},"depot":{},"installed":{}}',
    crew_roster JSONB
);

CREATE TABLE IF NOT EXISTS owned_vehicles (
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    inventory_id BIGINT NOT NULL CHECK (inventory_id > 0),
    catalog_key TEXT NOT NULL CHECK (catalog_key LIKE '%:%'),
    ammunition JSONB,
    has_crew BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (account_id, inventory_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS owned_vehicles_catalog_key ON owned_vehicles(account_id, catalog_key);

CREATE INDEX IF NOT EXISTS accounts_display_name_prefix ON accounts (lower(display_name) text_pattern_ops);

CREATE TABLE IF NOT EXISTS web_sessions (
    secret_hash BYTEA PRIMARY KEY CHECK (octet_length(secret_hash) = 32),
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    role TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('player', 'admin'))
);
CREATE INDEX IF NOT EXISTS web_sessions_account ON web_sessions(account_id);
CREATE INDEX IF NOT EXISTS web_sessions_expiry ON web_sessions(expires_at);
CREATE INDEX IF NOT EXISTS accounts_display_name_folded_idx ON accounts (lower(display_name));
CREATE INDEX IF NOT EXISTS accounts_username_folded_idx ON accounts (lower(username));
