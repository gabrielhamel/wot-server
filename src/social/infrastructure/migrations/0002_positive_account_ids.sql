ALTER TABLE social.social_contacts ADD CHECK (owner_id > 0 AND peer_id > 0);
ALTER TABLE social.private_conversations ADD CHECK (first_id > 0 AND second_id > 0);
ALTER TABLE social.private_messages ADD CHECK (sender_id > 0);
