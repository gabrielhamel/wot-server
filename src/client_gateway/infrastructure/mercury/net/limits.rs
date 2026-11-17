//! Hard bounds for untrusted network work and retained transport state.

pub const MAX_PEER_STATES: usize = 512;
pub const MAX_BUNDLE_PACKETS: usize = 256;
pub const MAX_FRAGMENT_ASSEMBLIES: usize = 4;
pub const MAX_FRAGMENT_SLOTS: usize = 4096;
pub const MAX_BUFFERED_PACKETS: usize = 4096;
pub const MAX_CHANNEL_BUFFERED_PACKETS: usize = 256;
pub const MAX_QUEUED_BUNDLES: usize = 16;
pub const MAX_RELIABLE_OUTGOING: usize = 2048;
pub const MAX_GLOBAL_RELIABLE_OUTGOING: usize = 16384;
pub const MAX_PACKETS_PER_SECOND: usize = 8192;
pub const MAX_POLL_PACKETS: usize = 32;
pub const MAX_PENDING_HANDSHAKES: usize = 256;
pub const MAX_LOGIN_REQUESTS_PER_SECOND: usize = 32;
pub const MAX_LOGIN_REQUESTS_PER_IP: usize = 8;
pub const MAX_LOGIN_CIPHERTEXT_BYTES: usize = 2048;
pub const MAX_COLLECTION_ITEMS: usize = 4096;
pub const MAX_PIGGYBACK_DEPTH: usize = 8;
pub const MAX_ELEMENTS_PER_BUNDLE: usize = 256;
