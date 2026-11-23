# Player Sessions

The `wot-player-sessions` library publishes grants and Store requests through
`wot_player_sessions::contract`. Server composition supplies the Battles adapter
when binding the gateway.

Player Sessions owns the lifecycle of a player connection, from admission and
short-lived handoff tickets through authenticated play and disconnection.
Accounts owns durable player state and publishes the snapshots and operations
that the garage adapter translates into the versioned BigWorld protocol.

Social owns contacts, private conversations and retained messages. The gateway
adapts those operations for native XMPP and legacy BW/BW_CHAT2 clients, tracks
presence, authenticates chat tokens through game sessions and routes delivery.
Player Sessions owns the Training text channel through live room membership.
When a room is ready to launch, Player Sessions sends the selected map,
duration and equipped roster through the [Battles contract](../battles/src/contract.rs).
The room remains discoverable during the round. Player Sessions handles the
arena handoff and restores the same membership on garage return. It maps the
current admission generation to the live Mercury client for battle delivery and
for returns scheduled by Battles.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Bounded admission generations, expiry and stale completion rejection. |
| `domain/training.rs` | Training Room duration and comment limits. |
| `application/` | Credential input policy, preferences, and garage command translation and dispatch to Accounts' published application contract. |
| `infrastructure/gateway/` | UDP event loop, bounded async workers, ticket/session lifecycle and garage command translation. |
| `infrastructure/gateway/admission.rs` | Authorize handoff, replace a previous session or ticket, and hold logins while an account write finishes. |
| `infrastructure/gateway/training.rs` | Check map IDs through Battles and manage shared room settings, membership, roster, readiness and deletion. |
| `infrastructure/gateway/training/start.rs` | Validate team confirmation, prepare the roster through the Battles contract using participant IDs, and hand off eligible sessions. |
| `infrastructure/gateway/training/chat.rs` | Open and close the native Training channel and relay messages to the active room roster. |
| `infrastructure/gateway/training/invites.rs` | Keep active invitations by account ID and deliver them immediately or on garage sync. |
| `infrastructure/gateway/ammunition.rs` | Shell request validation, bounded proposal/commit jobs and committed inventory, balance and receipt delivery. |
| `infrastructure/gateway/progression.rs` | Equipment, directive, module, tank and depot request validation, bounded progression jobs and committed cache delivery. |
| `infrastructure/gateway/grants.rs` | Schedule operator grants alongside garage writes using the Accounts command contract. |
| `application/store.rs` | Store customer identity, garage session tokens and the bounded request channel between HTTP and the gateway. |
| `infrastructure/gateway/premium.rs` | Notify the live garage when an account’s Premium time expires. |
| `infrastructure/gateway/store.rs` | Look up live Store sessions, read persisted dashboard catalogs and coordinate account writes. |
| `infrastructure/gateway/account_updates.rs` | Update handoff and session snapshots and deliver committed cache diffs for external account writes. |
| `presentation/store/` | In-game and dashboard Store HTTP catalog and checkout endpoints. |
| `presentation/store/web/` | Store categories, vehicle filters and checkout state. |
| `application/store-contracts.ts` and `infrastructure/store-api.ts` | Typed catalog and order contracts and the HTTP adapter that traces browser purchases. |
| `infrastructure/gateway/cosmetics.rs` | Decode native cosmetic stock purchases and binary seasonal and common outfits for the shared progression transaction. |
| `infrastructure/gateway/expendables.rs` | Decode consumable and directive layouts and auto-resupply settings for the shared progression transaction. |
| `infrastructure/gateway/social.rs` | Messenger wire translation, bounded async Social requests, presence and private delivery. |
| `infrastructure/xmpp/` | Bounded TCP/XML handling, game-issued token admission, native roster, presence, offline/history and WG extension translation. |
| `infrastructure/credentials.rs` | Translate the basic-login JSON envelope into an account identity; reject official token methods. |
| `infrastructure/schema.rs` | Bind the Account wire schema and select its version property and default from the resource bundle at startup. |
| `infrastructure/bootstrap.rs` | Client bootstrap settings, the Store hostname allowlist, account-specific GUI context and placeholder web-service token responses. |
| `infrastructure/endpoint.rs` | Resolve the BaseApp handoff: loopback for local peers when bound, then the configured destination or route to each client. |

Player Sessions traces game commands, XMPP requests, worker scheduling, completion
handling and response delivery. It retains request parents across the grant,
crew, completion and XMPP output queues. Spans identify operations and
ddd.bounded_context.name without architecture layer labels. The gateway exports
its online session count, the active arena count from Battles, and shared battle
tick timings and skipped intervals. See the [telemetry contract](../../docs/telemetry.md).

The [server composition](../server-runtime/src/lib.rs) runs LoginApp and BaseApp together. Player Sessions uses [Platform Mercury](../shared-infrastructure/src/mercury) for shared UDP transport
and shares the [client format codecs](../shared-infrastructure/src/infrastructure/client_formats/README.md)
with Accounts.

| Term | Meaning |
| --- | --- |
| Mercury | BigWorld UDP protocol for login, handoff and Account traffic. |
| Admission | One bounded, asynchronous credential lookup for a network address. |
| Generation | Identifies a lookup so a timed-out completion cannot approve a later attempt. |
| Handoff ticket | Random, one-use key granting a same-IP client access to BaseApp for 30 seconds. |
| Session | One admitted client and its account projection; one session per account. |
| Store session token | Random credential issued in the Store URL fragment, kept for the game session and accepted while the garage is synchronized. |
| Account entity | Client protocol representation created after the client enables entities. |
| Resource stream | CRC-described, fragmented transfer correlated with an account command request ID. |
| Training Room | A discoverable lobby with one host and optional guests. Membership spans rounds and the host can close the room by leaving. |
| Training channel | Live text conversation shared by Training Room members in the garage. |
| Training invitation | An account-addressed invitation to join a specific active Training Room. |

A successful login replaces the previous session or pending handoff for that
account, including a session whose client crashed. The gateway waits for
an in-flight write and refreshes the account snapshot before issuing a
replacement ticket. A successful authentication request can reuse its account's
existing capacity slot. The gateway invalidates older tickets, removes the
previous transport and revokes its Store and XMPP access.

Disconnect and two minutes without incoming client traffic reclaim the session and
transport state. Idle heartbeats count as activity. Admission capacity is bounded
independently of the SQL connection pool; preferences and garage inventory commands share one pending write per
session. Native crew actions can queue while another crew action is writing, with
32 pending requests allowed per session. Inventory mutations also use global
worker permits and hold account admission until pending completion is processed.
The Admin grant queue is bounded. A grant reserves the account's write slot until
its commit and session update complete. The gateway updates outstanding handoff
tickets and refreshes any login lookup that could have read a pre-grant snapshot.

The [Store](../../docs/store.md) accepts in-game requests with a live garage
token and dashboard requests for the account authenticated by Player
Administration. Both use the same write slot and cache publication as other
external account changes.

Player Administration uses the [published grants and Store contract](src/contract.rs)
and the [Store browser entry point](src/web.ts). The embedded Store receives a
transport from the dashboard and uses it for catalog reads and orders. The
server injects Accounts and Social contracts into the gateway. The catalog,
account view, and committed changes expose the projections needed for client
delivery. Session state is held in memory.
