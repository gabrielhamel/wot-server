# Span attribute reference

This is the inventory of all 109 fields explicitly declared by server spans,
including the two OpenTelemetry control fields described below. It documents
current instrumentation, rather than promising that every field appears on
every span. See the [telemetry contract](telemetry.md) for tracing requirements
and [server configuration](server.md#logging-and-telemetry) for export setup.

## Reading attributes

- `input.*` describes the data seen by that operation; rejected requests can
  contain invalid values. `output.*` describes the recorded result, with delivery
  limitations noted below. A child span does not automatically inherit its
  parent's attributes; follow the trace ancestry to find them.
- Fields declared as `tracing::field::Empty` appear only once recorded. Optional
  values are omitted when absent. Early decode, validation, or authentication
  failures can therefore produce spans with fewer attributes.
- `*.bytes` and `messaging.message.body.size` use bytes (UTF-8 for text).
  `*.count` and other counts use entries/items, not bytes. IDs are local to their
  described entity; collectors may display numeric IDs as strings.
- Debug-form enums and grant descriptions are diagnostic Rust representations,
  not a stable wire serialization. Do not parse them as protocol data.
- Names and email addresses are recorded only when available as described below.
  No database queries are added solely to enrich telemetry. Credentials, password
  hashes, tokens, message bodies and raw authentication envelopes are excluded.
- Architecture layer segments in span names are intentionally absent.
  Source-location metadata from the tracing library is separate.

Each attribute links to an emitting source file. Shared keys can have different
meanings on different operations; those differences are listed explicitly.

## Identity, context and client

| Attribute | Meaning and emission |
| --- | --- |
| [`wot.context`](../src/access/application/grants.rs) | Owning bounded context: `access`, `accounts`, `social`, `admin`, or `server`. |
| [`user.id`](../src/access/application/mod.rs) | Local account ID when known: authenticated login, session, account operation, or authenticated XMPP handler. It identifies the affected account, not necessarily an admin caller. Login records it only after authentication succeeds. |
| [`user.name`](../src/access/infrastructure/gateway/mod.rs) | Nonempty player display name from the account snapshot already returned by successful game authentication. Never copied from the login email; omitted if unavailable. No extra database query. |
| [`user.email`](../src/access/infrastructure/gateway/mod.rs) | Email-shaped login supplied by the current game authentication payload, recorded after authentication succeeds. Requires nonempty local part, a dot in the domain, and no whitespace; this is a shape check, not proof of ownership. Omitted for non-email logins and requests without an email payload. |
| [`network.protocol.name`](../src/access/infrastructure/gateway/mod.rs) | `mercury` for game authentication/commands, `xmpp` for incoming XMPP requests. |
| [`rpc.method`](../src/access/infrastructure/gateway/mod.rs) | Known incoming game entity method name on `game.command`. |
| [`wot.client.protocol_version`](../src/access/infrastructure/gateway/mod.rs) | Protocol number supplied by the game login request; not a confirmed game release version. |
| [`wot.client.resource_digest`](../src/access/infrastructure/gateway/mod.rs) | Optional login digest encoded as hexadecimal. Omitted when the request does not supply a digest. |
| [`wot.client.language`](../src/access/infrastructure/gateway/mod.rs) | Login context `lang_id`, when valid JSON provides a string of at most 32 bytes; otherwise omitted. |
| [`outcome`](../src/access/infrastructure/gateway/mod.rs) | Game authentication result: `authenticated`, `rejected` (credentials did not authenticate), or `error` (operation failed). |

## HTTP and XMPP

| Attribute | Meaning and emission |
| --- | --- |
| [`http.request.method`](../src/admin/presentation/mod.rs) | HTTP method on the root admin request span. |
| [`url.path`](../src/admin/presentation/mod.rs) | Admin request URI path, excluding its query string. |
| [`http.response.status_code`](../src/admin/presentation/mod.rs) | HTTP response status, recorded after the admin handler returns a response. |
| [`input.method`](../src/admin/presentation/mod.rs) | HTTP method checked by admin authentication. |
| [`input.id.bytes`](../src/admin/presentation/garage.rs) | Byte length of the admin account-ID path parameter before parsing. |
| [`input.page`](../src/admin/presentation/players.rs) | Requested player-list page, or default `1`; recorded after query decoding. |
| [`input.per_page`](../src/admin/presentation/players.rs) | Requested page size, or default `25`; recorded after query decoding. |
| [`input.offset`](../src/accounts/application/players.rs) | Validated zero-based pagination offset passed to the player repository. |
| [`input.limit`](../src/accounts/application/players.rs) | Validated maximum player rows requested by pagination. |
| [`input.grant`](../src/access/application/grants.rs) | Debug representation of the parsed `PlayerGrant`: currency and amount, or vehicle type descriptor. Recorded after admin validation, and on the Access grant use case. |
| [`xmpp.stanza.name`](../src/access/infrastructure/xmpp/protocol.rs) | Parsed XML element local name on `access.xmpp.handle`; unavailable if XML parsing fails. |
| [`xmpp.stanza.id`](../src/access/infrastructure/xmpp/protocol.rs) | Optional XML stanza `id` when at most 128 bytes; omitted when absent or longer. |
| [`messaging.destination.id`](../src/access/infrastructure/xmpp/protocol.rs) | Resolved recipient account ID on an XMPP private-message handler; recorded after recipient resolution. |
| [`messaging.message.body.size`](../src/access/infrastructure/xmpp/protocol.rs) | UTF-8 byte length of the private-message body. The body text is not recorded. |
| [`input.bytes`](../src/access/infrastructure/xmpp/mod.rs) | Incoming XMPP stanza byte length on `xmpp.request`. |
| [`input.stanza.bytes`](../src/access/infrastructure/xmpp/mod.rs) | XMPP stanza byte length on handling or push operations. |
| [`input.available`](../src/access/infrastructure/xmpp/mod.rs) | Presence availability boolean requested by `access.presence`. |

## Dispatch, social operations and identifiers

| Attribute | Meaning and emission |
| --- | --- |
| [`input.client.address`](../src/access/infrastructure/gateway/mod.rs) | Client socket address (IP and port) for connection, gateway enable, or social dispatch/work/completion operations. |
| [`input.request_id`](../src/access/infrastructure/gateway/ammunition.rs) | Decoded Mercury command or legacy chat request correlation ID; also retained by resource responses and garage completions. Unused fields remain absent if decoding fails. |
| [`input.command_id`](../src/access/infrastructure/gateway/commands.rs) | Decoded account command number on `access.commands.command`. |
| [`input.arg0`](../src/access/infrastructure/gateway/commands.rs) | First decoded scalar argument on supported scalar command forms; absent for array-only forms. |
| [`input.arg1`](../src/access/infrastructure/gateway/commands.rs) | Second decoded scalar argument, when the command form supplies it. |
| [`input.arg2`](../src/access/infrastructure/gateway/commands.rs) | Third decoded scalar argument, when the command form supplies it. |
| [`input.arg3`](../src/access/infrastructure/gateway/commands.rs) | Fourth decoded scalar argument, when the command form supplies it. |
| [`input.args.count`](../src/access/infrastructure/gateway/commands.rs) | Decoded array argument length on array command forms; array contents are not recorded. |
| [`input.has_command`](../src/access/infrastructure/gateway/progression.rs) | Whether progression dispatch received a parsed progression command. |
| [`input.id`](../src/access/application/mod.rs) | Operation-specific ID: account ID for account snapshot/settings and Social player lookup; target account for XMPP pushes/presence; crew member ID for crew dismissal/restoration; conversation ID for `social.conversations.load`. Interpret with the span name. |
| [`input.account_id`](../src/access/application/grants.rs) | Target account ID passed to the Access admin-grant use case. |
| [`input.owner`](../src/access/infrastructure/xmpp/protocol.rs) | Account owning a social operation, roster, friendship, or private conversation; authenticated account for XMPP roster/IQ operations. |
| [`input.peer`](../src/access/infrastructure/xmpp/protocol.rs) | Other account in a friendship, roster-state, or private-conversation operation. |
| [`input.player`](../src/social/application/mod.rs) | Account whose presence observers are being queried. |
| [`input.first`](../src/social/infrastructure/conversations.rs) | First account ID used to acquire a private-conversation pair lock. |
| [`input.second`](../src/social/infrastructure/conversations.rs) | Second account ID used to acquire a private-conversation pair lock. |
| [`input.conversation`](../src/social/application/mod.rs) | Private-conversation ID for send, close, history loading, or retention operations. |
| [`input.action`](../src/access/infrastructure/gateway/social.rs) | Legacy chat action number on `access.social.send_chat`; debug-form friendship action (`Save`, `Request`, `Accept`, `Decline`, `Cancel`, `Remove`) on Social friendship operations. Interpret with the span name. |
| [`input.change`](../src/social/application/mod.rs) | Debug-form contact change (`Friend`, `Ignore`, `RemoveFriend`, `RemoveIgnored`) requested through the Social use case/repository. |
| [`input.request`](../src/access/infrastructure/gateway/social.rs) | Legacy chat correlation ID passed to the protocol error-response builder. |
| [`input.event`](../src/access/infrastructure/gateway/social.rs) | Legacy chat event/action byte passed to the protocol error-response builder. |
| [`input.channel`](../src/access/infrastructure/gateway/social.rs) | Legacy chat channel ID passed to the protocol error-response builder. |
| [`input.response`](../src/access/infrastructure/gateway/social.rs) | Legacy chat response code passed to the protocol error-response builder. |
| [`input.prefix.bytes`](../src/social/application/mod.rs) | UTF-8 byte length of a player-search prefix; prefix text is omitted. |
| [`input.text.bytes`](../src/social/application/mod.rs) | UTF-8 byte length of a private message passed to Social; message text is omitted. |

## Account, inventory and progression inputs

| Attribute | Meaning and emission |
| --- | --- |
| [`input.username.bytes`](../src/access/application/mod.rs) | UTF-8 byte length of the account login for authentication, credentials lookup, seeding, or player creation; raw login is not recorded in this field. |
| [`input.display_name.bytes`](../src/accounts/application/players.rs) | UTF-8 byte length of a player-creation display name, recorded after decoding/normalization at the relevant operation. |
| [`input.algorithm`](../src/accounts/application/players.rs) | Password hashing algorithm on `accounts.players.hash_password`, currently `argon2id`. Password and hash are omitted. |
| [`input.previous_revision`](../src/access/application/mod.rs) | Expected persisted revision for an optimistic account write or its client diff. A stale database revision prevents the proposed write. |
| [`input.revision`](../src/accounts/application/mod.rs) | Revision of the input account snapshot used for catalog preparation or mutation planning. |
| [`input.changes.count`](../src/access/application/mod.rs) | Number of incoming account-setting changes in the Access settings use case. |
| [`input.settings.count`](../src/accounts/application/mod.rs) | Number of settings entries passed to account persistence. |
| [`input.inventory_id`](../src/accounts/application/mod.rs) | Owned vehicle inventory ID for ammunition planning, module mounting, equipment application, or consumables. Distinct from a vehicle type descriptor. |
| [`input.vehicle`](../src/accounts/domain/directives.rs) | Owned vehicle inventory ID targeted by directive installation. |
| [`input.vehicle_type`](../src/accounts/domain/crew.rs) | Vehicle type descriptor used to route battle experience to its crew. |
| [`input.catalog_key.bytes`](../src/accounts/domain/vehicles.rs) | Byte length of the vehicle catalog key checked for duplicate ownership and garage capacity. |
| [`input.collection`](../src/accounts/infrastructure/postgres.rs) | Collection being read: `vehicles` on the admin catalog span, `account_vehicles` on account catalog-key collection (which queries `owned_vehicles`). |
| [`input.count`](../src/accounts/domain/consumables.rs) | Requested purchase quantity, equipment charge quantity, or cost multiplier, depending on the operation. |
| [`input.item`](../src/accounts/domain/consumables.rs) | Compact item descriptor for a module, equipment, consumable, or directive involved in the operation. |
| [`input.index`](../src/accounts/domain/modules.rs) | Zero-based index into a vehicle module research plan. |
| [`input.gun`](../src/accounts/domain/modules.rs) | Gun descriptor supplied with a module equip request. |
| [`input.buy`](../src/accounts/domain/modules.rs) | Whether module equip may buy the requested module rather than requiring depot stock. |
| [`input.researched`](../src/accounts/domain/vehicles.rs) | Whether the vehicle purchase rule received a researched vehicle. |
| [`input.gold`](../src/accounts/domain/vehicles.rs) | Gold component of a proposed vehicle/item cost before charging; not the account balance. |
| [`input.credits`](../src/accounts/domain/vehicles.rs) | Credit component of a proposed vehicle/item cost before charging; not the account balance. |
| [`input.amount`](../src/accounts/domain/grants.rs) | Requested amount in a currency grant, before positivity and balance-limit checks. |
| [`input.slots`](../src/accounts/domain/consumables.rs) | Current garage slot balance on `accounts.vehicles.buy_vehicle_slot`; available consumable-slot count on consumable purchase/fill operations. |
| [`input.requested.count`](../src/accounts/domain/ammunition.rs) | Number of requested shell selections passed to ammunition validation. |
| [`input.depot.count`](../src/accounts/domain/ammunition.rs) | Number of distinct shell types in the depot passed to ammunition validation; not total shell units. |
| [`input.stock.shell_types`](../src/accounts/domain/depot.rs) | Number of distinct shell types currently held in depot stock at the start of a depot sale. |
| [`input.stock.fitting_types`](../src/accounts/domain/depot.rs) | Number of distinct fitting/item descriptors currently held in depot stock at the start of a depot sale. |
| [`input.player_count`](../src/accounts/infrastructure/seeding.rs) | Number of predefined demo players in the seeding operation. |
| [`input.vehicle_count`](../src/accounts/infrastructure/seeding.rs) | Number of owned vehicles being persisted in a starter garage. |

## Crew inputs

| Attribute | Meaning and emission |
| --- | --- |
| [`input.roster.count`](../src/accounts/domain/crew.rs) | Crew member count before inserting a member into the roster. |
| [`input.berths`](../src/accounts/domain/crew.rs) | Barracks berth capacity passed to crew capacity validation. |
| [`input.at`](../src/accounts/domain/crew.rs) | Unix timestamp in seconds used for crew dismissal or restoration eligibility. |
| [`input.slot`](../src/accounts/domain/crew.rs) | Optional zero-based crew seat index for assignment. An absent index represents vehicle-wide unassignment and is omitted from exported attributes. |
| [`input.member`](../src/accounts/domain/crew.rs) | Optional crew member ID to assign. Omitted when absent (unassignment). |
| [`input.accelerated_training`](../src/accounts/domain/crew.rs) | Whether accelerated crew training is enabled when distributing battle XP. |
| [`input.combat_xp`](../src/accounts/domain/crew.rs) | Combat XP awarded by the battle, used for accelerated-training routing. |
| [`input.crew_xp`](../src/accounts/domain/crew.rs) | Crew XP awarded by the battle before per-member routing. |
| [`input.packs`](../src/accounts/domain/crew.rs) | Number of barracks expansion packs requested. |
| [`input.requested`](../src/accounts/domain/crew.rs) | Amount of free XP requested for crew training. |
| [`input.course`](../src/accounts/domain/crew.rs) | Perk-reset course code: `0` free (80% reuse), `1` credits (90%), `2` gold (100%), `4` 100,000 credits (100%); other codes are rejected. |
| [`input.role`](../src/accounts/domain/crew.rs) | Crew role code used to validate learned perks. |
| [`input.bonus`](../src/accounts/domain/crew.rs) | Whether the learned perks are bonus perks rather than the main perk set. |
| [`input.reuse_percent`](../src/accounts/domain/crew.rs) | Requested percentage of allocated perk XP to refund on reset; calculation caps it at 100. |

## Results and delivery

| Attribute | Meaning and emission |
| --- | --- |
| [`output.bytes`](../src/access/infrastructure/gateway/commands.rs) | Encoded outgoing XMPP stanza or Mercury resource response byte length; excludes transport framing added later. |
| [`output.count`](../src/social/application/mod.rs) | Number of returned Social search results, roster entries, resulting roster entries after a contact change, pending inbound requests, presence observers, or restored private conversations, depending on the operation. |
| [`output.outcome`](../src/access/infrastructure/gateway/ammunition.rs) | Garage progression/ammunition completion result: `committed`, `unchanged`, `rejected`, `stale`, or `error`. Describes the mutation result, not guaranteed socket delivery. |
| [`output.response`](../src/access/infrastructure/gateway/social.rs) | Legacy chat `actionResponse` code sent by `access.social.send_chat`. |
| [`output.stanza_count`](../src/access/infrastructure/xmpp/mod.rs) | Number of immediate replies returned by the XMPP handler, recorded before writing them. Does not count separate queued pushes or prove all writes succeeded. |
| [`output.success`](../src/access/infrastructure/gateway/grants.rs) | Whether a Social job or admin-grant completion contains an `Ok` result; does not by itself prove recipient delivery. |

## Startup, database and OpenTelemetry controls

| Attribute | Meaning and emission |
| --- | --- |
| [`input.resource_dir`](../src/server/mod.rs) | Configured extracted server resource-bundle path loaded at startup. |
| [`input.pool.max_connections`](../src/server/mod.rs) | Configured maximum PostgreSQL pool connections on `server.connect_database`. |
| [`input.connections`](../src/accounts/infrastructure/postgres.rs) | Requested maximum PostgreSQL pool connections on `accounts.postgres.connect`. |
| [`db.system.name`](../src/observability.rs) | Database system, always `postgresql` in the shared database tracing adapter. |
| [`db.operation.name`](../src/observability.rs) | First SQL statement token, such as `SELECT`, `UPDATE`, `BEGIN`, or `COMMIT`; falls back to `QUERY` for an empty statement. |
| [`db.query.text`](../src/observability.rs) | Complete executed SQL statement with parameter placeholders. Bind values are not interpolated or serialized. |
| [`error.type`](../src/observability.rs) | Rust error type name when a database operation fails; omitted on success. |
| [`otel.kind`](../src/observability.rs) | Tracing bridge control field set to `client` for PostgreSQL operations. Exported as the intrinsic span kind, not an ordinary attribute. |
| [`otel.status_code`](../src/observability.rs) | Tracing bridge control field set to `ERROR` on database failure. Exported as intrinsic span status, not an ordinary attribute; diagnostic error events can also mark spans as failed. |

## Transport diagnostic spans

The Mercury [protocol tracker](../src/access/infrastructure/mercury/net/proto.rs)
also declares legacy TRACE-level spans with implicit argument fields. The
production OTLP filter exports the server target at INFO, so these spans and
their fields are not currently exported. The `pigb` TRACE span has no declared
fields. If the export filter is expanded, the following fields become relevant:

| Attribute | Meaning and emission |
| --- | --- |
| `addr` | Debug-form remote socket address on packet `accept`. Packet contents and the tracker itself are skipped. |
| `reliable` | Boolean specifying reliable bundle transmission on `prepare`. Bundle contents and the channel itself are skipped. |

## Metadata added by the tracing library

[Logging initialization](../src/server/logging.rs) uses the defaults of the
`tracing-opentelemetry` version pinned in [Cargo.lock](../Cargo.lock) (currently
0.34.0). Its span metadata is independent of the explicit field inventory above.
Source locations describe where a span is created, not a distributed stack trace.
Thread metadata describes the creating thread; async work can later move threads.

| Attribute | Meaning and emission |
| --- | --- |
| `code.file.path` | Source file containing the span declaration, when available. |
| `code.module.name` | Rust module path containing the span declaration, when available. |
| `code.line.number` | Source line containing the span declaration, when available. |
| `thread.id` | Tracing library's process-local numeric ID for the creating thread; not an OS thread identifier. |
| `thread.name` | Creating thread's name, when the thread has one. |
| `target` | Tracing target, normally the emitting Rust module path. |
| `busy_ns` | Accumulated time in nanoseconds while the tracing span was entered, attached when it closes. |
| `idle_ns` | Accumulated time in nanoseconds while the tracing span was not entered, attached when it closes. |
| `exception.message` | Error description when a typed error is recorded on the span and mapped by the bridge. Error events can carry it on the event instead. |
| `exception.stacktrace` | Typed error's source-chain descriptions when the bridge maps a span error; despite the name, this is not a captured call stack. |

Span `level` is disabled by the current bridge defaults. Diagnostic events can
have their own level, target, source metadata, error description and event-specific
fields; those event fields are not ordinary attributes of the enclosing span.
Trace ID, span ID, parent span ID, span name, kind, status, timestamps, events and
links are intrinsic OpenTelemetry span fields, not entries in its attribute map.
In particular, the `otel.*` fields above configure kind/status and are consumed
by the bridge.

## Resource attributes accompanying spans

These describe the exporting process/resource, not individual operations. The
same resource accompanies logs and metrics. Source:
[telemetry initialization](../src/server/telemetry.rs).

| Attribute | Meaning and emission |
| --- | --- |
| `service.name` | Nonempty `OTEL_SERVICE_NAME`, otherwise the Cargo package name (`wot-server`). |
| `service.version` | Nonempty build-time `WOT_SERVER_VERSION`, otherwise the Cargo package version; release builds supply the release tag. |
| `wot.game.build_version` | Nonempty build-time `WOT_GAME_VERSION` (launcher build used to extract resources); omitted when not supplied. |
| `wot.game.version` | Game version read from the loaded resource bundle manifest; omitted if the bundle cannot be opened or lacks that value. |
| `telemetry.sdk.name` | SDK name supplied by the OpenTelemetry resource detector. |
| `telemetry.sdk.language` | SDK language supplied by the resource detector (`rust`). |
| `telemetry.sdk.version` | OpenTelemetry SDK version supplied by the resource detector. |

`Resource::builder()` also runs the SDK's environment resource detector.
`OTEL_RESOURCE_ATTRIBUTES` can therefore add arbitrary resource keys; those
operator-defined keys are outside the fixed server inventory. Explicit resource
attributes above override detector values with the same keys. A collector or
backend may add further metadata after export.

## Maintaining this reference

When adding, renaming, removing, or changing the meaning of a span field, update
its entry and emitting-source link here in the same change. Record units,
operation-specific meanings, optionality, and enum outcomes. Update the metadata
section when changing tracing bridge configuration or dependency behavior, and
the resource section when changing SDK resource initialization.

To audit coverage, inspect `#[tracing::instrument(... fields(...))]`,
`tracing::*_span!`/`tracing::span!` declarations, and subsequent `Span::record`
calls under `src/`; event/log fields are a separate inventory. The regression
capture in [tests/support/telemetry_capture.rs](../tests/support/telemetry_capture.rs)
checks representative exported fields and parent links through real entry points.
