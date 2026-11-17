# Access

Access owns network admission, short-lived login handoff tickets, authenticated
sessions and the client-facing garage command adapter. Accounts is upstream:
Access consumes its public snapshot and application contracts and translates them
into the versioned BigWorld protocol. Social is upstream for contacts, durable private pairs and retained messages.
Access adapts the native XMPP messenger and keeps the BW/BW_CHAT2 compatibility
RPCs. It tracks presence, authenticates chat tokens through game sessions and
delivers committed native messages over XMPP and legacy events through Account. It does not issue SQL or own player inventory.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Bounded admission generations, expiry and stale completion rejection. |
| `application/` | Credential input policy, preferences, and ammunition/progression commit use cases, using Accounts' published application API. |
| `infrastructure/mercury/` | Mercury UDP framing, reliability, RSA and Blowfish, LoginApp/BaseApp wire sessions, and Account RPC binding from the [resource bundle](../../docs/resources.md). |
| `infrastructure/gateway/` | UDP event loop, bounded async workers, ticket/session lifecycle and garage command translation. |
| `infrastructure/gateway/ammunition.rs` | Shell request validation, bounded proposal/commit jobs and committed inventory, balance and receipt delivery. |
| `infrastructure/gateway/progression.rs` | Equipment, directive, module, tank and depot request validation, bounded progression jobs and committed cache delivery. |
| `infrastructure/gateway/grants.rs` | Schedule operator grants alongside garage writes, update handoff and session snapshots, and deliver committed cache diffs. |
| `infrastructure/gateway/expendables.rs` | Decode consumable and directive layouts and auto-resupply settings for the shared progression transaction. |
| `infrastructure/gateway/social.rs` | Messenger wire translation, bounded async Social requests, presence and private delivery. |
| `infrastructure/xmpp/` | Bounded TCP/XML handling, game-issued token admission, native roster, presence, offline/history and WG extension translation. |
| `infrastructure/credentials.rs` | Translate the basic-login JSON envelope into an account identity; reject official token methods. |
| `infrastructure/schema.rs` | Version-bound Account wire schema; separate from account domain models. |
| `infrastructure/bootstrap.rs` | Client bootstrap settings, account-specific GUI context and placeholder web-service token responses. |
| `infrastructure/endpoint.rs` | Resolve the BaseApp handoff: loopback for local peers when bound, then the configured destination or route to each client. |

Access traces game commands, XMPP requests, worker scheduling, completion
handling and response delivery. It retains request parents across the grant,
crew, completion and XMPP output queues. Spans identify operations and
wot.context without architecture layer labels. See the [telemetry contract](../../docs/telemetry.md).

The [server composition](../server/mod.rs) runs LoginApp and BaseApp together. Access owns the UDP protocol
implementation and shares the [client format codecs](../client_formats/README.md)
with Accounts.

| Term | Meaning |
| --- | --- |
| Mercury | BigWorld UDP protocol for login, handoff and Account traffic. |
| Admission | One bounded, asynchronous credential lookup for a network address. |
| Generation | Identifies a lookup so a timed-out completion cannot approve a later attempt. |
| Handoff ticket | Random, one-use key granting a same-IP client access to BaseApp for 30 seconds. |
| Session | One admitted client and its account projection; one session per account. |
| Account entity | Client protocol representation created after the client enables entities. |
| Resource stream | CRC-described, fragmented transfer correlated with an account command request ID. |

Disconnect and two minutes without incoming client traffic reclaim the session and
transport state. Idle heartbeats count as activity. Admission capacity is bounded
independently of the SQL connection pool; preferences and garage inventory commands share one pending write per
session. Native crew actions can queue while another crew action is writing, with
32 pending requests allowed per session. Inventory mutations also use global
worker permits and hold account admission until pending completion is processed.
The Admin grant queue is bounded. A grant reserves the account's write slot until
its commit and session update complete. The gateway updates outstanding handoff
tickets and refreshes any login lookup that could have read a pre-grant snapshot.
