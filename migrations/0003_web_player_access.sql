ALTER TABLE accounts ALTER COLUMN bonds SET DEFAULT 0,
                     ALTER COLUMN free_xp SET DEFAULT 0;

CREATE INDEX accounts_display_name_folded_idx ON accounts (lower(display_name));
CREATE INDEX accounts_username_folded_idx ON accounts (lower(username));

ALTER TABLE admin_sessions RENAME TO web_sessions;
ALTER INDEX admin_sessions_account RENAME TO web_sessions_account;
ALTER INDEX admin_sessions_expiry RENAME TO web_sessions_expiry;
ALTER TABLE web_sessions ADD COLUMN role TEXT NOT NULL DEFAULT 'admin'
    CHECK (role IN ('player', 'admin'));
