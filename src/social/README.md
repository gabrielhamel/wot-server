# Social

Social owns durable, directed contacts, native private conversations and retained
messages. Player Sessions supplies authenticated account IDs. Social does not authenticate
users, own socket connections, alter account identities, or implement battles.

| Layer | Responsibility |
| --- | --- |
| `domain/` | Player/contact concepts, typed changes, durable private-conversation/message models, legacy room subscriptions and request budgets. |
| `application/` | Validated search, contact, private-open/send/restore operations through `SocialRepository`. |
| `infrastructure/mod.rs` | Accounts directory adapter, PostgreSQL contacts and presence-observer queries. |
| `infrastructure/conversations.rs` | Durable pairs, atomic message writes, recent restoration and bounded history. |

Social owns the `social` PostgreSQL schema and its
[embedded migrations](infrastructure/migrations/). It resolves player names
and checks participants through Accounts' published `PlayerDirectory` before
writes. Reads load Social records and resolve names with bounded batches. The
directory calls run outside Social transactions, which permits a shared pool
with a single connection. Social stores numeric account IDs and retains its own
conversation-to-message foreign key. A missing identity is omitted from roster
and history responses. Native friendship changes lock both participants in ID
order. Legacy contact changes lock the owner, and private writes lock both
participants in ID order, keeping ignore checks and writes coherent.

Player Sessions consumes the [published Social contract](mod.rs). Its native XMPP adapter owns
XML/JID translation and presence/socket delivery. Its BW compatibility adapter
still uses the separate in-memory `Conversations` model; that legacy lifecycle
is not the durable native conversation contract.

| Term | Meaning |
| --- | --- |
| Player | Durable account ID and canonical display name, without credentials. |
| Contact | Owner's directed roster or ignored-player entry; never both. |
| Friendship request | Pending permission to share presence; approval adds both players to each other's rosters. |
| Friend | Saved player, independent of whether they saved the owner. |
| Private conversation | Durable, unique unordered pair of player IDs. |
| Message | Committed text with authenticated sender, timestamp and stable ID. |
| Recent conversation | Saved pair selected for bounded restoration on reconnect. |
| Legacy subscription | One initialized BW client window; closing leaves the peer alone. |
| Presence | Availability of a native chat resource, supplied by Player Sessions. |

Limits: 300 contacts and 300 saved pairs per account, 100 retained messages per
pair, 16 recent native conversations restored, 2048 message bytes. Recent-set
rotation preserves the pair's retained history. Offline delivery uses the same
committed history; it does not allocate an unbounded mailbox. Legacy rooms remain
capped at 16 per player. See [protocol findings](../../docs/social.md) for client
sources, endpoint/authentication contracts and validation limits.

Social application and PostgreSQL repository spans preserve the calling XMPP
or garage command trace. Inputs include participant IDs, conversation IDs,
message sizes and bounded operation metadata. Query spans carry complete
parameterized SQL and transaction boundaries. See the
[telemetry contract](../../docs/telemetry.md).
