CREATE TABLE IF NOT EXISTS social_contacts (
    owner_id BIGINT NOT NULL,
    peer_id BIGINT NOT NULL,
    ignored BOOLEAN NOT NULL DEFAULT FALSE,
    friendship SMALLINT NOT NULL DEFAULT 2,
    PRIMARY KEY (owner_id, peer_id),
    CHECK (owner_id <> peer_id),
    CONSTRAINT social_contacts_friendship CHECK (friendship IN (0, 1, 2))
);
CREATE INDEX IF NOT EXISTS social_contacts_observers ON social_contacts(peer_id,owner_id) WHERE NOT ignored;
CREATE INDEX IF NOT EXISTS social_contacts_inbound_requests ON social_contacts (peer_id, owner_id)
    WHERE friendship = 1 AND NOT ignored;

CREATE TABLE IF NOT EXISTS private_conversations (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK (id > 0),
    first_id BIGINT NOT NULL,
    second_id BIGINT NOT NULL,
    first_open BOOLEAN NOT NULL DEFAULT FALSE,
    second_open BOOLEAN NOT NULL DEFAULT FALSE,
    last_used DOUBLE PRECISION NOT NULL DEFAULT extract(epoch FROM clock_timestamp()),
    UNIQUE(first_id, second_id),
    CHECK(first_id < second_id)
);
CREATE INDEX IF NOT EXISTS private_conversations_second_player ON private_conversations(second_id);

CREATE TABLE IF NOT EXISTS private_messages (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES private_conversations(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL,
    sent_at DOUBLE PRECISION NOT NULL DEFAULT extract(epoch FROM clock_timestamp()),
    body TEXT NOT NULL CHECK(octet_length(body) BETWEEN 1 AND 2048)
);
CREATE INDEX IF NOT EXISTS private_messages_history ON private_messages(conversation_id, id DESC);
