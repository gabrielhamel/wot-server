CREATE TABLE accounts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username TEXT NOT NULL UNIQUE CHECK (length(username) BETWEEN 1 AND 128),
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 128),
    password_hash TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    credits BIGINT NOT NULL DEFAULT 100000000 CHECK (credits >= 0),
    gold BIGINT NOT NULL DEFAULT 1000000 CHECK (gold >= 0),
    bonds BIGINT NOT NULL DEFAULT 10000 CHECK (bonds >= 0),
    equipment_components BIGINT NOT NULL DEFAULT 0 CHECK (equipment_components >= 0),
    free_xp BIGINT NOT NULL DEFAULT 1000000 CHECK (free_xp >= 0),
    slots BIGINT NOT NULL DEFAULT 100 CHECK (slots >= 0),
    berths BIGINT NOT NULL DEFAULT 100 CHECK (berths >= 0),
    settings JSONB NOT NULL DEFAULT '{}',
    depot_shells JSONB NOT NULL DEFAULT '{}',
    progression JSONB NOT NULL DEFAULT '{"unlocks":[],"vehicle_xp":{},"depot":{},"installed":{}}',
    crew_roster JSONB
);

CREATE TABLE owned_vehicles (
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    inventory_id BIGINT NOT NULL CHECK (inventory_id > 0),
    catalog_key TEXT NOT NULL CHECK (catalog_key LIKE '%:%'),
    ammunition JSONB,
    has_crew BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (account_id, inventory_id)
);

CREATE UNIQUE INDEX owned_vehicles_catalog_key ON owned_vehicles(account_id, catalog_key);

CREATE INDEX accounts_display_name_prefix ON accounts (lower(display_name) text_pattern_ops);
CREATE VIEW account_directory AS SELECT id, display_name FROM accounts;

CREATE TABLE social_contacts (
    owner_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    peer_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    ignored BOOLEAN NOT NULL DEFAULT FALSE,
    friendship SMALLINT NOT NULL DEFAULT 2,
    PRIMARY KEY (owner_id, peer_id),
    CHECK (owner_id <> peer_id),
    CONSTRAINT social_contacts_friendship CHECK (friendship IN (0, 1, 2))
);
CREATE INDEX social_contacts_observers ON social_contacts(peer_id,owner_id) WHERE NOT ignored;
CREATE INDEX social_contacts_inbound_requests ON social_contacts (peer_id, owner_id)
    WHERE friendship = 1 AND NOT ignored;

CREATE TABLE private_conversations (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK (id > 0),
    first_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    second_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    first_open BOOLEAN NOT NULL DEFAULT FALSE,
    second_open BOOLEAN NOT NULL DEFAULT FALSE,
    last_used DOUBLE PRECISION NOT NULL DEFAULT extract(epoch FROM clock_timestamp()),
    UNIQUE(first_id, second_id),
    CHECK(first_id < second_id)
);
CREATE INDEX private_conversations_second_player ON private_conversations(second_id);

CREATE TABLE private_messages (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES private_conversations(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sent_at DOUBLE PRECISION NOT NULL DEFAULT extract(epoch FROM clock_timestamp()),
    body TEXT NOT NULL CHECK(octet_length(body) BETWEEN 1 AND 2048)
);
CREATE INDEX private_messages_history ON private_messages(conversation_id, id DESC);
