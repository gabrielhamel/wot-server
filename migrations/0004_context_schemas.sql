CREATE SCHEMA accounts;
CREATE SCHEMA social;

DROP VIEW account_directory;

ALTER TABLE social_contacts
    DROP CONSTRAINT social_contacts_owner_id_fkey,
    DROP CONSTRAINT social_contacts_peer_id_fkey;
ALTER TABLE private_conversations
    DROP CONSTRAINT private_conversations_first_id_fkey,
    DROP CONSTRAINT private_conversations_second_id_fkey;
ALTER TABLE private_messages
    DROP CONSTRAINT private_messages_sender_id_fkey;

ALTER TABLE accounts SET SCHEMA accounts;
ALTER TABLE owned_vehicles SET SCHEMA accounts;
ALTER TABLE web_sessions SET SCHEMA accounts;

ALTER TABLE social_contacts SET SCHEMA social;
ALTER TABLE private_conversations SET SCHEMA social;
ALTER TABLE private_messages SET SCHEMA social;
