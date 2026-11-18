ALTER TABLE accounts ADD COLUMN role TEXT NOT NULL DEFAULT 'player'
    CHECK (role IN ('player', 'admin'));

CREATE TABLE admin_sessions (
    secret_hash BYTEA PRIMARY KEY CHECK (octet_length(secret_hash) = 32),
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX admin_sessions_account ON admin_sessions(account_id);
CREATE INDEX admin_sessions_expiry ON admin_sessions(expires_at);
