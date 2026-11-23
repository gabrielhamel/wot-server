# Span attribute reference

This is the inventory of fields explicitly declared by server and browser spans,
including the OpenTelemetry control fields described below. It documents
current instrumentation, rather than promising that every field appears on
every span. See the [telemetry contract](telemetry.md) for tracing requirements
and [server configuration](server.md#logging-and-telemetry) for export setup.

## Source addresses

| Attribute | Meaning and emission |
| --- | --- |
| `client.address` | Caller IP without port on `admin.http.request`, `game.authenticate`, `game.command`, `xmpp.request`, and `web.ui.request`. Store API requests record their immediate network peer. Admin HTTP and shared UI requests use a valid single `X-Forwarded-For` IP only from a configured trusted peer; otherwise the socket IP. Omitted only when an HTTP request has no socket metadata (for example, an in-process router call). |
| `client.port` | Caller source port, in TCP/UDP port units, on the same roots when the socket identifies the caller. Omitted for forwarded HTTP callers because the header supplies no source port. |
| `network.peer.address` | Immediate socket peer IP without port on those roots and `store.http.request` or `web.ui.request`; remains the proxy address for forwarded HTTP requests. |
| `network.peer.port` | Immediate socket peer TCP/UDP source port on those roots and `store.http.request` or `web.ui.request`. |
| `input.web.asset_source` | Static UI asset source on `web.ui.request`. `bundle` means embedded in the server executable and `directory` means a configured filesystem override. |
| `network.transport` | `tcp` for HTTP/XMPP and `udp` for Mercury on those roots and `store.http.request` or `web.ui.request` as applicable. |

Supporting sources: [Admin HTTP middleware](../src/player-administration/src/presentation/mod.rs),
[Store API middleware](../src/player-sessions/src/presentation/store/mod.rs),
[Web UI middleware](../src/server-runtime/src/infrastructure/web.rs),
[HTTP address validation](../src/shared-infrastructure/src/infrastructure/http.rs),
[Mercury gateway](../src/player-sessions/src/infrastructure/gateway/mod.rs), and
[XMPP listener](../src/player-sessions/src/infrastructure/xmpp/mod.rs).
Child spans retain the causal parent rather than copying its source attributes.

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
| [`ddd.bounded_context.name`](../src/player-sessions/src/application/grants.rs) | Custom DDD attribute naming the owning context. Values include `player_sessions`, `accounts`, `battles`, `social`, `player_administration`, and `server`. |
| [`user.id`](../src/player-sessions/src/application/mod.rs) | Local account ID when known: authenticated login, session, account operation, authenticated XMPP handler, or Store request authorized by a live garage or dashboard session. It identifies the affected account, not necessarily an admin caller. Login records it only after authentication succeeds. |
| [`user.name`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Nonempty player display name from the account snapshot already returned by successful game authentication. Never copied from the login email; omitted if unavailable. No extra database query. |
| [`user.email`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Email-shaped login supplied by the current game authentication payload, recorded after authentication succeeds. Requires nonempty local part, a dot in the domain, and no whitespace; this is a shape check, not proof of ownership. Omitted for non-email logins and requests without an email payload. |
| [`network.protocol.name`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | `mercury` for game authentication/commands, `xmpp` for incoming XMPP requests. |
| [`output.store.host`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Exact Store hostname or IP included in the client browser allowlist when the Account entity is created. Omitted when the Store is disabled or no origin is available for that client. |
| [`output.training_room.visible`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Boolean Training Room visibility flag on `player_sessions.enable` after the Account creation is queued. |
| [`output.account.version_property`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Account version field name from the bundle on `player_sessions.enable` after Account creation is queued. |
| [`output.training_room.description_enabled`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Boolean Description field enable flag for Create Room and Modify Room on `player_sessions.enable` after the Account creation is queued. |
| [`rpc.method`](../src/shared-infrastructure/src/mercury/app/base/mod.rs) | Known incoming entity method on `game.command` or outbound entity method on `mercury.response`. |
| [`wot.client.protocol_version`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Protocol number supplied by the game login request; not a confirmed game release version. |
| [`wot.client.resource_digest`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Optional login digest encoded as hexadecimal. Omitted when the request does not supply a digest. |
| [`wot.client.language`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Login context `lang_id`, when valid JSON provides a string of at most 32 bytes; otherwise omitted. |
| [`outcome`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Game authentication result: `authenticated`, `rejected` or `error` after credential work. Early exits record `invalid_username`, `malformed_request`, `rate_limited`, `admission_unavailable`, `admission_expired` or `completion_unavailable`. |

## HTTP and XMPP

| Attribute | Meaning and emission |
| --- | --- |
| [`http.request.method`](../src/player-administration/src/presentation/mod.rs) | HTTP method on the root admin API, Store API or web UI request span. |
| [`url.path`](../src/player-administration/src/presentation/mod.rs) | Admin request URI path, excluding its query string. |
| [`http.route`](../src/player-administration/src/presentation/mod.rs) | Matched route template on the root HTTP span when available, with path parameters left as placeholders. Store request handlers also record the matched route. |
| [`input.query.bytes`](../src/player-administration/src/presentation/mod.rs) | Length in bytes of the encoded URI query, when present. Query text is excluded. |
| [`input.content_length`](../src/player-administration/src/presentation/mod.rs) | Declared body length in bytes when a valid nonnegative `Content-Length` fits in a signed 64-bit integer. This is request metadata and does not count bytes read. |
| [`http.response.status_code`](../src/player-administration/src/presentation/mod.rs) | HTTP response status after the admin API, Store API or web UI handler returns. |
| [`input.channel`](../src/player-sessions/src/presentation/store/mod.rs) | Store request credential channel, `garage` or `dashboard`, on the catalog and purchase handler spans. |
| [`input.session.present`](../src/player-administration/src/presentation/access.rs) | Whether a well-formed session cookie was supplied on an admin session check or sign-out. |
| [`input.session.lifetime_seconds`](../src/accounts/src/infrastructure/player_access.rs) | Configured admin session lifetime in seconds when a session is opened. |
| [`input.role`](../src/player-administration/src/presentation/access.rs) | Requested account role (`player` or `admin`) on a role change. Bootstrap records `admin`. |
| [`input.id.bytes`](../src/player-administration/src/presentation/garage.rs) | Byte length of the admin account-ID path parameter before parsing. |
| [`input.page`](../src/player-administration/src/presentation/players.rs) | Requested player-list page after query decoding. The root span records supplied values only. The list operation records default `1` when omitted. |
| [`input.per_page`](../src/player-administration/src/presentation/players.rs) | Requested player-list page size after query decoding. The root span records supplied values only. The list operation records default `25` when omitted. |
| [`input.offset`](../src/accounts/src/application/players.rs) | Validated zero-based pagination offset passed to the player repository. |
| [`input.limit`](../src/accounts/src/application/players.rs) | Validated maximum player rows requested by pagination. |
| [`input.grant`](../src/player-sessions/src/application/grants.rs) | Debug representation of the parsed `PlayerGrant` containing the selected currency and amount, vehicle type descriptor, depot or cosmetic item ID and count, and target inventory ID for stat trackers. Recorded after HTTP decoding and on the Player Sessions and Accounts grant spans. |
| [`input.grant.kind`](../src/player-administration/src/presentation/garage.rs) | Decoded grant kind on the root HTTP span, one of `currency`, `vehicle`, `crew_book`, `improved_equipment`, `bounty_equipment`, `experimental_equipment`, `attachment`, `stat_tracker`, `style2d`, `style3d`, `crew_skin`, or `special_recruit`. |
| [`input.grant.currency`](../src/player-administration/src/presentation/garage.rs) | Decoded currency on the root HTTP span for currency grants only. One of `credits`, `gold`, `bonds`, `free_xp`, or `equipment_components`. |
| [`xmpp.stanza.name`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Parsed XML element local name on `player_sessions.xmpp.handle`; unavailable if XML parsing fails. |
| [`xmpp.stanza.id`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Optional XML stanza `id` when at most 128 bytes; omitted when absent or longer. |
| [`input.xmpp.query`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Category of an unimplemented IQ query on `player_sessions.xmpp.iq`: `filter_roster`, `blocking`, `storage` or `unknown`. |
| [`messaging.destination.id`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Resolved recipient account ID on an XMPP private-message handler; recorded after recipient resolution. |
| [`messaging.message.body.size`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | UTF-8 byte length of the private-message body. The body text is not recorded. |
| [`input.bytes`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | Incoming XMPP stanza byte length on `xmpp.request`. On `game.command`, undecoded argument bytes for an unknown Mercury method. |
| [`input.stanza.bytes`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | XMPP stanza byte length on handling or push operations. |
| [`input.available`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | Presence availability boolean requested by `player_sessions.presence`. |
| [`output.disconnect.reason`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | Reason for a normal XMPP connection close on `player_sessions.serve`. One of `session_revoked` when the game credential is no longer valid, `peer_closed` when the peer ends the stream or reaches EOF, `server_shutdown`, or `outbound_closed`. Omitted on failure. |

## Dispatch, social operations and identifiers

| Attribute | Meaning and emission |
| --- | --- |
| [`input.client.address`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Client socket address (IP and port) for connection, gateway enable, or social dispatch/work/completion operations. |
| [`input.port`](../src/player-sessions/src/infrastructure/gateway/sockets.rs) | Configured BaseApp and native XMPP port number. Zero requests automatic assignment. |
| [`output.port`](../src/player-sessions/src/infrastructure/gateway/sockets.rs) | Shared port number once both BaseApp UDP and native XMPP TCP sockets are bound. |
| [`output.bind_retries.count`](../src/player-sessions/src/infrastructure/gateway/sockets.rs) | Number of automatic port selections retried because the TCP port was in use. |
| [`input.request_id`](../src/player-sessions/src/infrastructure/gateway/ammunition.rs) | Decoded Mercury command or legacy chat request correlation ID; also recorded by operation, resource, LoginApp reply and garage completion spans. Mercury client method spans record it for `onCmdResponse`, `onCmdResponseExt` and `onTokenReceived`. Omitted when the operation has no decoded correlation ID. |
| [`input.command_id`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Decoded account command number on `game.command` and `player_sessions.commands.command`. |
| [`input.method_id`](../src/shared-infrastructure/src/mercury/app/base/mod.rs) | Numeric exposed Mercury method ID after wire decoding. |
| [`input.entity_id`](../src/shared-infrastructure/src/mercury/app/base/mod.rs) | Protocol entity ID of the player's target entity. |
| [`input.string.bytes`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Byte length of the `doCmdStr` argument, Training Room creation description, or Training Room invitation message. The text stays out of telemetry. |
| [`input.arena_type_id`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Client-supplied arena type ID on Training Room creation, or the server-selected arena on battle preparation, obstacle loading and entry. |
| [`input.round_length.seconds`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Requested duration in seconds on Training Room creation and battle preparation. |
| [`input.room.open_flag`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Wire privacy flag on Training Room creation. `0` denotes invite-only and `1` denotes open. |
| [`input.room.id`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Training Room ID supplied by the client on join or decline, or set by the server for invitation delivery, channel membership and Training launch preparation. |
| [`input.battle.id`](../src/battles/src/infrastructure/mod.rs) | Server-assigned battle ID on activation, arena entry, period and readiness broadcasts, vehicle controls and shots. |
| [`input.participant.id`](../src/battles/src/infrastructure/mod.rs) | Numeric admission generation scoped to the gateway. Recorded on arena entry and participant removal, and on [decoded vehicle controls](../src/battles/src/infrastructure/controls.rs), [shots](../src/battles/src/infrastructure/projectiles.rs), and [damage sticker deliveries](../src/battles/src/infrastructure/wire/controls.rs). |
| [`output.battle.id`](../src/battles/src/infrastructure/mod.rs) | Battle ID allocated during preparation or resolved on participant removal. |
| [`input.cluster.id`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Cluster supplied on Training Room join or decline, or configured periphery on invitation delivery. Public room-list joins use `0`. |
| [`input.invitation`](../src/player-sessions/src/infrastructure/gateway/training.rs) | True when the join request accepts an invitation. |
| [`input.recipients.count`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Number of requested recipient IDs before deduplication. The IDs stay out of telemetry. |
| [`input.invite.state`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Native invitation state sent in the update. `1` active, `2` accepted, `3` declined, `4` expired. |
| [`input.player_entity_id`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Target Account entity ID on a Training Room roster assignment or exclusion. |
| [`input.roster.slot`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Requested roster slot on assignment. Team 1 and Team 2 use `1` and `2`. Their unassigned slots use `17` and `18`. |
| [`input.roster.group`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Requested player group on roster assignment. Training Rooms accept group `0`. |
| [`input.team`](../src/player-sessions/src/infrastructure/gateway/training/start.rs) | Team number from a Start Battle confirmation or the server-assigned team on arena entry. |
| [`output.avatar_control`](../src/battles/src/infrastructure/mod.rs) | `server` once the Avatar control handoff is queued during arena entry. The child Mercury delivery span records the send outcome. |
| [`input.force`](../src/player-sessions/src/infrastructure/gateway/training/start.rs) | Native team-ready flag supplied with command `411`. Values `0` and `1` are accepted. |
| [`input.ready`](../src/battles/src/infrastructure/mod.rs) | True for a team confirmation and false for a team unlock. On arena roster updates, true once the Avatar is ready and false on departure. |
| [`input.element_id`](../src/shared-infrastructure/src/mercury/app/base/mod.rs) | One-byte Mercury element ID on arena delivery and incoming cell commands. |
| [`input.drive.flags`](../src/battles/src/infrastructure/controls.rs) | Native movement bitmask supplied by the player. The high bit is reserved and rejected. The accepted mask is also recorded on the drive step. |
| [`input.ground.hard_contacts`](../src/battles/src/domain/vehicle.rs), `input.ground.medium_contacts`, `input.ground.soft_contacts` | Number of chassis surface samples of each class in the drive step, out of four. Unpainted or unavailable surface samples count as medium. |
| [`input.ground.pitch_radians`](../src/battles/src/domain/vehicle.rs) | Starting hull pitch in radians for the drive step. Positive values descend along the forward axis. |
| [`input.ammunition.setting`](../src/battles/src/infrastructure/controls.rs) | Native ammunition setting code `0`, `1`, `17` or `18` on a decoded shell selection or reload command. |
| [`input.ammunition.shell`](../src/battles/src/infrastructure/controls.rs) | Requested signed shell compact descriptor on shell selection, or current shell compact descriptor on a [shot attempt](../src/battles/src/infrastructure/projectiles.rs). |
| [`input.shell.kind`](../src/battles/src/infrastructure/projectiles.rs) | Client shell kind index of a fired shell. |
| [`input.shell.caliber_mm`](../src/battles/src/infrastructure/projectiles.rs) | Effect caliber in millimetres for a fired shell. |
| [`input.shell.damage`](../src/battles/src/infrastructure/projectiles.rs) | Nominal hit points for the fired shell before capping by the target's remaining health. Omitted on rejected shots. |
| [`input.aim.kind`](../src/battles/src/infrastructure/controls.rs) | Aim representation on an aim command, `world_point` or `hull_angles`. |
| [`input.aim.server_marker`](../src/battles/src/infrastructure/controls.rs) | Boolean supplied by the Avatar's server reticle setting command. |
| [`input.replication.kind`](../src/battles/src/infrastructure/controls.rs) | Arena delivery kind, including `position`, `hull_aim` (driver orientation and track motion), `engine_mode`, `wheels_scroll`, `steering_angles`, `gun_angles`, `gun_marker` and `ammunition`. |
| [`input.arena.tick`](../src/battles/src/infrastructure/mod.rs) | Full arena tick count for the first position snapshot carrying a drive command's change. One unit is 100 ms. |
| [`input.geometry`](../src/battles/src/infrastructure/terrain.rs) | Bundle geometry path when loading terrain or obstacles for the selected arena. |
| [`input.roster.row`](../src/battles/src/infrastructure/mod.rs) | Zero-based row index when updating a player's ready state in the arena roster. |
| [`input.players.count`](../src/battles/src/infrastructure/mod.rs) | Roster size during preparation and activation, or current participant count on a period transition. |
| [`input.cache.bytes`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Cached shop length in bytes reported by the client. |
| [`input.cache.crc`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Signed CRC32 of the shop cache reported by the client. |
| [`input.token_type`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Numeric token kind from the Mercury request. Credential material stays out of telemetry. |
| [`input.arg0`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | First decoded scalar argument on supported scalar command forms; absent for array-only forms. |
| [`input.arg1`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Second decoded scalar argument, when the command form supplies it. |
| [`input.arg2`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Third decoded scalar argument, when the command form supplies it. |
| [`input.arg3`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Fourth decoded scalar argument, when the command form supplies it. |
| [`input.chat.joining`](../src/player-sessions/src/infrastructure/gateway/training/chat.rs) | True when opening the Training channel for a member, false when closing it. |
| [`input.args.count`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Number of wire method arguments on `game.command`; number of integers on decoded array commands, Training Room listing, settings and ammunition spans. Array contents stay out of telemetry. |
| [`input.customization.display_type`](../src/battles/src/infrastructure/mod.rs) | Viewer preference sent to their Avatar at arena entry. Values are 0 for historical, 1 to include non-historical styles and 2 for all styles. |
| [`input.outfits.count`](../src/player-sessions/src/infrastructure/gateway/cosmetics.rs) | Number of outfit descriptors in a decoded array command, including empty descriptors, or participants with a nonempty outfit on arena preparation. |
| [`input.outfit.bytes`](../src/player-sessions/src/infrastructure/gateway/cosmetics.rs) | Total length in bytes of all outfit descriptors passed to the Exterior adapter. Descriptor contents are omitted. |
| [`input.shop_revision`](../src/player-sessions/src/infrastructure/gateway/cosmetics.rs) | Requested shop revision on a cosmetic purchase or apply command, or on progression and ammunition dispatch when supplied. |
| [`input.has_command`](../src/player-sessions/src/infrastructure/gateway/progression.rs) | Whether progression dispatch received a parsed progression command. |
| [`input.id`](../src/player-sessions/src/application/mod.rs) | Operation-specific ID: account ID for account snapshot/settings and Social player lookup; target account for XMPP pushes/presence; crew member ID for crew dismissal/restoration; conversation ID for `social.conversations.load`. Interpret with the span name. |
| [`input.account_id`](../src/player-sessions/src/application/grants.rs) | Target account ID on grants, garage reads or role changes. The HTTP root records the validated ID. |
| [`input.owner`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Account owning a social operation, roster, friendship, or private conversation; authenticated account for XMPP roster/IQ operations. |
| [`input.peer`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Other account in a friendship, roster-state, or private-conversation operation. |
| [`input.player`](../src/social/src/application/mod.rs) | Account whose presence observers are being queried. |
| [`input.first`](../src/social/src/infrastructure/conversations.rs) | First account ID used to acquire a private-conversation pair lock. |
| [`input.second`](../src/social/src/infrastructure/conversations.rs) | Second account ID used to acquire a private-conversation pair lock. |
| [`input.conversation`](../src/social/src/application/mod.rs) | Private-conversation ID for send, close, history loading, or retention operations. |
| [`input.action`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Chat action number on `player_sessions.social.social_command` and `player_sessions.social.send_chat`; debug-form friendship action (`Save`, `Request`, `Accept`, `Decline`, `Cancel`, `Remove`) on Social friendship operations. Interpret with the span name. |
| [`input.change`](../src/social/src/application/mod.rs) | Debug-form contact change (`Friend`, `Ignore`, `RemoveFriend`, `RemoveIgnored`) requested through the Social use case/repository. |
| [`input.request`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Legacy chat correlation ID passed to the protocol error-response builder. |
| [`input.event`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Legacy chat event/action byte passed to the protocol error-response builder. |
| [`input.channel`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Legacy chat channel ID on Mercury dispatch and protocol error responses. |
| [`input.response`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Legacy chat response code passed to the protocol error-response builder. |
| [`input.prefix.bytes`](../src/social/src/application/mod.rs) | UTF-8 byte length of a player-search prefix; prefix text is omitted. |
| [`input.text.bytes`](../src/social/src/application/mod.rs) | Byte length of a private message passed to Social or of the primary Mercury chat string argument, including BW_CHAT2 Training messages. Message text is omitted. |

## Account, inventory and progression inputs

| Attribute | Meaning and emission |
| --- | --- |
| [`input.email.bytes`](../src/player-administration/src/presentation/registration.rs) | UTF-8 byte length of the submitted email on the HTTP request, handler and Accounts validation for registration, managed player creation and [first-admin setup](../src/player-administration/src/presentation/players.rs). Managed player creation and setup receive the email in the request's `username` field. |
| [`output.registration_open`](../src/accounts/src/infrastructure/players.rs) | Boolean indicating whether an administrator exists. Emitted on the Accounts registration status span and signed-out session status. |
| [`output.username.available`](../src/player-administration/src/presentation/registration.rs) | Boolean availability of the submitted in-game username after a case-insensitive check. |
| [`output.vehicle_count`](../src/player-administration/src/presentation/garage.rs) | Number of owned tanks returned by the personal garage handler. |
| [`input.username.bytes`](../src/player-sessions/src/application/mod.rs) | UTF-8 byte length of the account login for authentication, credentials lookup, or player creation. The root HTTP span records the length after decoding sign-in, player creation or first-admin setup JSON. On registration HTTP handlers and validation it is the UTF-8 byte length of the proposed in-game username. |
| [`input.display_name.bytes`](../src/accounts/src/application/players.rs) | UTF-8 byte length of a player-creation display name, recorded after decoding/normalization at the relevant operation. The root HTTP span records the decoded length before validation. [First-admin validation](../src/accounts/src/domain/players.rs) also records the submitted length. |
| [`input.algorithm`](../src/accounts/src/application/players.rs) | Password algorithm on `accounts.players.hash_password` and `accounts.verify_password`, currently `argon2id`. Password and hash are omitted. |
| [`input.previous_revision`](../src/accounts/src/application/garage.rs) | Expected persisted revision for a Store quote, an optimistic account write or its client diff. A stale database revision prevents the proposed write. |
| [`input.revision`](../src/accounts/src/application/mod.rs) | Revision of the input snapshot used for catalog preparation or mutation planning. On the Store purchase span, the submitted quote revision. On garage sync, the revision requested by the client. |
| [`input.changes.count`](../src/player-sessions/src/application/mod.rs) | Number of incoming account-setting changes in the Player Sessions settings use case or completion, after deduplication by key. |
| [`input.settings.count`](../src/accounts/src/application/mod.rs) | Number of settings entries passed to account persistence. |
| [`input.inventory_id`](../src/accounts/src/application/mod.rs) | Vehicle inventory ID for ammunition planning, module mounting, equipment application, consumables, outfit changes, a bound stat tracker grant or Training Room readiness. Training Room readiness uses `-5000` for the observer. For a standalone cosmetic purchase the value can be zero for unbound stock. Distinct from a vehicle type descriptor. |
| [`input.operation`](../src/accounts/src/domain/cosmetics.rs) | Exterior action on the ownership decision span, `buy` or `apply`. On progression planning, a server-defined operation label such as `research`, `buy_vehicle`, `crew`, `disassemble_equipment` or `upgrade_equipment`. Admin access uses `status` for session checks and `post` for the required write header check. |
| [`input.items.count`](../src/accounts/src/domain/cosmetics.rs) | Number of purchase lines or requested placements across the supplied seasons. Each painted region counts as one placement. A style counts once in the common outfit. |
| [`input.vehicle`](../src/accounts/src/domain/directives.rs) | Owned vehicle inventory ID targeted by directive installation. |
| [`input.vehicle_type`](../src/accounts/src/domain/crew.rs) | Vehicle type descriptor used to route battle experience to its crew. Also the numeric catalog vehicle descriptor for a bound stat tracker grant or vehicle-specific outfit change, and on the root HTTP span for a decoded vehicle grant. |
| [`input.catalog_key.bytes`](../src/accounts/src/domain/vehicles.rs) | Byte length of the vehicle catalog key checked for duplicate ownership and garage capacity. |
| [`input.collection`](../src/accounts/src/infrastructure/postgres.rs) | Collection being read: `vehicles`, `attachments`, `stat_trackers`, `styles_2d`, `styles_3d`, `crew_skins`, `special_recruits`, `crew_books`, `improved_equipment`, `bounty_equipment` or `experimental_equipment` on the admin catalog span, `account_vehicles` on account catalog-key collection (which queries `owned_vehicles`), `accounts` when checking first-admin eligibility, or `web_sessions` when finding or closing a web session. |
| [`input.lines`](../src/accounts/src/domain/depot.rs) | Number of priced rows in the Depot sale or disassembly batch, including repeated descriptors. |
| [`input.equipment_change`](../src/accounts/src/domain/equipment.rs) | Bounded representation of a mounted equipment change, with up to four descriptors for an Apply, slot and payment flag for a slot change, or the descriptor to disassemble. |
| [`input.price.credits`, `input.price.components`](../src/accounts/src/domain/equipment.rs) | Catalog price for a Bounty or Experimental Equipment upgrade, recorded in credits or Components after resolving the source descriptor. |
| [`input.components.per_item`](../src/accounts/src/domain/equipment.rs) | Catalog payout per Experimental Equipment copy, in Components. |
| [`input.count`](../src/accounts/src/domain/consumables.rs) | Requested purchase quantity, equipment charge quantity, cost multiplier, or copies in a 2D or 3D style, crew skin, crew book, or equipment grant. Depot grants record it in the HTTP root and Accounts depot operation. Disassembly records the number of copies in each priced line. |
| [`input.item`](../src/accounts/src/domain/consumables.rs) | Compact item descriptor for a module, equipment, consumable, or directive involved in the operation. Depot grants record the compact descriptor in the Accounts depot operation. The admin HTTP span records the catalog ID for styles, crew skins, and special recruits, or the compact descriptor for equipment and crew books. Experimental Equipment disassembly records the descriptor being converted. Equipment upgrades record the source descriptor, read from the slot for mounted upgrades. Store orders record the submitted item descriptor or negative Premium Account package duration in days. |
| [`input.price.currency`, `input.price.amount`](../src/accounts/src/infrastructure/client/store.rs) | Submitted Store quote currency (`credits`, `gold` or `bonds`) and integer amount. Accounts compares both with the offer before charging. |
| [`input.duration.days`, `input.price.gold`](../src/accounts/src/domain/premium.rs) | Requested Premium Account days and the server price in gold. Only recognized durations record a gold price. |
| [`output.premium.expires_at`](../src/accounts/src/domain/premium.rs) | Expiration after a successful extension, in UTC Unix seconds. |
| [`input.premium.expires_at`](../src/player-sessions/src/infrastructure/gateway/premium.rs) | Expired entitlement timestamp in UTC Unix seconds when the gateway sends an inactivity update. |
| [`input.style_id`](../src/accounts/src/domain/cosmetics.rs) | Client style definition ID when a Store purchase or admin grant adds a style to the account or an outfit change requests a style in the common outfit. Omitted for outfit changes without a style ID. |
| [`input.index`](../src/accounts/src/domain/modules.rs) | Zero-based index into a vehicle module research plan. |
| [`input.gun`](../src/accounts/src/domain/modules.rs) | Gun descriptor supplied with a module equip request. |
| [`input.buy`](../src/accounts/src/domain/modules.rs) | Whether module equip may buy the requested module rather than requiring depot stock. |
| [`input.researched`](../src/accounts/src/domain/vehicles.rs) | Whether the vehicle purchase rule received a researched vehicle. |
| [`input.gold`](../src/accounts/src/domain/vehicles.rs) | Gold component of a proposed vehicle/item cost before charging; not the account balance. |
| [`input.credits`](../src/accounts/src/domain/vehicles.rs) | Credit component of a proposed vehicle/item cost before charging; not the account balance. |
| [`input.attachment_id`](../src/accounts/src/domain/cosmetics.rs) | Numeric catalog ID of the granted 3D attachment. Recorded on the root HTTP span after decoding and on the account operation. |
| [`input.stat_tracker_id`](../src/accounts/src/domain/cosmetics.rs) | Numeric catalog ID of the granted tracker. Recorded on the root HTTP span and on the account grant operation. |
| [`input.amount`](../src/accounts/src/domain/grants.rs) | Requested amount of currency or number of cosmetic copies in a grant, before count and ownership checks. Also recorded on the root HTTP span after decoding the grant. |
| [`input.currency`](../src/accounts/src/domain/grants.rs) | Parsed currency variant on the Accounts balance grant operation. |
| [`input.slots`](../src/accounts/src/domain/consumables.rs) | Current garage slot balance on `accounts.vehicles.buy_vehicle_slot`; available consumable-slot count on consumable purchase/fill operations. |
| [`input.requested.count`](../src/accounts/src/domain/ammunition.rs) | Number of requested shell selections passed to ammunition planning or validation. |
| [`input.depot.count`](../src/accounts/src/domain/ammunition.rs) | Number of distinct shell types in the depot passed to ammunition validation; not total shell units. |
| [`input.stock.shell_types`](../src/accounts/src/domain/depot.rs) | Number of distinct shell types currently held in depot stock at the start of a depot sale. |
| [`input.stock.fitting_types`](../src/accounts/src/domain/depot.rs) | Number of distinct fitting/item descriptors currently held in depot stock at the start of a depot sale. |
| [`input.vehicle_count`](../src/accounts/src/infrastructure/players.rs) | Number of vehicles persisted during starter garage creation or projected for the account cache. On [variant validation](../src/accounts/src/infrastructure/client/vehicle_defs.rs), the number of vehicle definitions screened for catalog eligibility. |
| [`output.accepted_count`, `output.rejected_count`](../src/accounts/src/infrastructure/client/vehicle_defs.rs) | Counts of otherwise eligible private or test vehicle variants that pass or fail validation of their stock garage and battle definitions. |
| [`vehicle.id`, `vehicle.key`](../src/accounts/src/infrastructure/client/vehicle_defs.rs) | Vehicle compact descriptor ID and nation-qualified catalog key on a variant validation rejection diagnostic. |
| [`input.starter.credits`, `input.starter.gold`](../src/accounts/src/infrastructure/players.rs) | Initial balances in credits and gold offered by `accounts.players.provision`, including attempts that end in conflict. |

## Crew inputs

| Attribute | Meaning and emission |
| --- | --- |
| [`input.roster.count`](../src/accounts/src/domain/crew.rs) | Crew member count before inserting a member into the roster. |
| [`input.berths`](../src/accounts/src/domain/crew.rs) | Barracks berth capacity passed to crew capacity validation. |
| [`input.at`](../src/accounts/src/domain/crew.rs) | Unix timestamp in seconds used for crew dismissal or restoration eligibility. |
| [`input.slot`](../src/accounts/src/domain/crew.rs) | Optional zero-based crew seat index for assignment. An absent index represents vehicle-wide unassignment and is omitted from exported attributes. Mounted equipment upgrades use the zero-based equipment slot. |
| [`input.member`](../src/accounts/src/domain/crew.rs) | Optional crew member ID to assign. Omitted when absent (unassignment). |
| [`input.accelerated_training`](../src/accounts/src/domain/crew.rs) | Whether accelerated crew training is enabled when distributing battle XP. |
| [`input.combat_xp`](../src/accounts/src/domain/crew.rs) | Combat XP awarded by the battle, used for accelerated-training routing. |
| [`input.crew_xp`](../src/accounts/src/domain/crew.rs) | Crew XP awarded by the battle before per-member routing. |
| [`input.packs`](../src/accounts/src/domain/crew.rs) | Number of barracks expansion packs requested. |
| [`input.requested`](../src/accounts/src/domain/crew.rs) | Amount of free XP requested for crew training. |
| [`input.course`](../src/accounts/src/domain/crew.rs) | Perk-reset course code: `0` free (80% reuse), `1` credits (90%), `2` gold (100%), `4` 100,000 credits (100%); other codes are rejected. |
| [`input.role`](../src/accounts/src/domain/crew.rs) | Crew role code used to validate learned perks. |
| [`input.bonus`](../src/accounts/src/domain/crew.rs) | Whether the learned perks are bonus perks rather than the main perk set. |
| [`input.reuse_percent`](../src/accounts/src/domain/crew.rs) | Requested percentage of allocated perk XP to refund on reset; calculation caps it at 100. |

## Results and delivery

| Attribute | Meaning and emission |
| --- | --- |
| [`output.outfit.bytes`](../src/accounts/src/infrastructure/client/mod.rs) | Length of the locked-style outfit descriptor in bytes on successful battle loadout projection. Zero for an unstyled vehicle. |
| [`output.outfits.locked_count`](../src/accounts/src/infrastructure/client/mod.rs) | Number of owned vehicles with a catalog-defined locked style in a successful garage projection. |
| [`output.bytes`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Byte length of an encoded outgoing XMPP stanza, compressed garage cache, Mercury resource response or available Training Room observer or combat vehicle descriptor. Excludes transport framing. |
| [`output.recipients.count`](../src/player-sessions/src/infrastructure/gateway/training/chat.rs) | Number of queued Training channel messages, including the sender, or invitations enqueued immediately for recipients with active garages. For arena readiness updates, counts the recipients queued for delivery. For position snapshots, counts the clients targeted by the socket writes. |
| [`output.invites.queued_count`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Number of new invitations awaiting a garage sync after recipient deduplication. |
| [`output.invites.delivered_count`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Number of active invitations enqueued for the recipient on garage sync. |
| [`output.delivered.count`](../src/battles/src/infrastructure/delivery.rs) | Number of successful socket writes for an arena position snapshot, or zero when the output queue is full. Hull orientation, chassis animation, gun angles, server marker, ammunition and arena period replication count successful enqueues to the gateway, where Mercury traces actual delivery. |
| [`output.rooms.count`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Number of Training Rooms returned by a successful list request. Omitted on rejection or a failed send. |
| [`output.rooms.total`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Total discoverable rooms in the list response before paging. Omitted on rejection or a failed send. |
| [`output.players.count`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Count of players in returned rooms on listing, or membership count on successful join or removal. A deleted room has zero players. |
| [`output.room.id`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Server-assigned Training Room ID when a creation, settings change, assignment, team swap, readiness, leave, exclusion, membership cleanup or chat broadcast operation has a room. |
| [`output.roster.previous_slot`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Target player's prior slot on assignment or the host's prior slot on team swap. |
| [`output.roster.slot`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Host's resulting roster slot on a valid team swap. |
| [`output.count`](../src/social/src/application/mod.rs) | Number of returned Social search results, roster entries, resulting roster entries after a contact change, pending inbound requests, presence observers, or restored private conversations, depending on the operation. |
| [`output.unlocked_vehicle_count`](../src/accounts/src/infrastructure/client/store.rs) | Number of gold vehicle descriptors purchasable through the native shop and projected in the Tech Tree economics unlock set during garage sync. |
| [`output.offers.count`](../src/player-sessions/src/infrastructure/gateway/store.rs) | Number of offers included in a Store catalog response. |
| [`output.arena_type_id`](../src/player-sessions/src/infrastructure/gateway/training/start.rs) | Arena type selected in the room when the host confirms or unlocks a team. |
| [`input.shell.max_distance_metres`](../src/battles/src/infrastructure/projectiles.rs) | Equipped gun's nominal maximum travel distance in metres for the fired shell. |
| [`output.shot.id`](../src/battles/src/infrastructure/projectiles.rs) | Arena-local sequence number assigned after firing, omitted on rejection. |
| [`output.projectile.flight_seconds`](../src/battles/src/infrastructure/projectiles.rs) | Seconds from launch to scheduled contact. Omitted for shots with no contact. |
| [`output.damage_stickers.count`](../src/battles/src/infrastructure/projectiles.rs) | Number of retained damage stickers on the target when the hit adds a new sticker. |
| [`input.damage_stickers.count`](../src/battles/src/infrastructure/wire/controls.rs) | Number of stickers in the full Vehicle damage sticker property sent to one participant. |
| [`output.hit_direction.radians`](../src/battles/src/infrastructure/projectiles.rs) | Incoming projectile's world travel yaw in radians, sent to the victim and omitted on miss. |
| [`output.damage_log.shell_type`](../src/battles/src/infrastructure/projectiles.rs) | Client damage-log shell type ID for a vehicle hit when known. |
| [`output.scene_impact.distance_metres`](../src/battles/src/infrastructure/projectiles.rs) | Distance in metres from the muzzle to a ground or obstacle impact. |
| [`output.obstacle.material_kind`](../src/battles/src/infrastructure/projectiles.rs) | Raw client material kind of the hit obstacle collision triangle, including 0 when the triangle carries no material. |
| [`output.target.entity_id`](../src/battles/src/infrastructure/projectiles.rs) | Vehicle entity struck by a shot, recorded for vehicle hits. |
| [`output.damage`](../src/battles/src/infrastructure/projectiles.rs) | Hit points actually removed by a shot, omitted on miss. |
| [`output.outcome`](../src/battles/src/infrastructure/controls.rs) | Cell controls and [shot resolution](../src/battles/src/infrastructure/projectiles.rs) record `accepted`, `shell_selected`, `shell_queued`, `shell_change_started`, `reload_started`, `magazine_full`, `manual_reload_unavailable`, `launched`, `projectile_queue_full`, `target_unavailable`, `hit`, `ground_impact`, `obstacle_impact`, `miss`, `reloading`, `empty`, `invalid_flags`, `invalid_aim`, `shell_unavailable`, `shell_damage_unavailable`, `shell_range_unavailable`, `round_inactive`, `vehicle_unavailable`, `unimplemented` or `delivery_failed`. Missing or invalid loaded-shell damage or range sets error status. Mercury records `entity_unavailable` for a target outside the session's control and `wire_decode_failed` for invalid framing or argument data. |
| [`output.obstacles.instances`](../src/battles/src/infrastructure/obstacles.rs) | Number of collision mesh placements loaded for the selected gameplay mode. Zero when the bundle has no obstacle collision for the map. |
| [`output.terrain.tiles`](../src/battles/src/infrastructure/terrain.rs), `output.terrain.surface_tiles` | Number of decoded outdoor height tiles and driving surface tiles for the selected arena. |
| [`output.drive.speed_mps`](../src/battles/src/domain/vehicle.rs) | Signed speed in metres per second after the simulation step. Negative means reverse. |
| [`output.drive.yaw_rate_rad_per_second`](../src/battles/src/domain/vehicle.rs) | Signed yaw rate in radians per second after the simulation step. Positive turns right. |
| [`output.battle.winner_team`](../src/battles/src/infrastructure/mod.rs) | Team 1 or 2, or 0 for a draw, when publishing the finished phase. |
| [`output.battle.finish_reason`](../src/battles/src/infrastructure/mod.rs) | Client finish reason at the finished phase. Training emits 1 for elimination, 3 for time limit or 5 for technical end. |
| [`output.phase`](../src/battles/src/infrastructure/mod.rs) | New round phase when publishing a transition. Values are `countdown`, `battle` and `finished`. |
| [`output.crew.count`](../src/accounts/src/infrastructure/client/mod.rs) | Number of crew seats in the catalog vehicle prepared for the arena. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training/start.rs) | Team commands record `team_ready`, `team_unlocked`, `launched`, `invalid_arguments`, `room_unavailable`, `not_host`, `battle_in_progress`, `team_not_ready`, `roster_or_map_unavailable`, `preparation_in_progress` (a pending launch or the pending launch limit), `preparation_stale` (the roster, settings or readiness changed), `preparation_failed` or `battle_queue_unavailable`. |
| [`output.outcome`](../src/battles/src/infrastructure/mod.rs) | Loadout projection can reject with `vehicle_unavailable`, `health_unavailable` or `performance_unavailable`. Preparation records `arena_unavailable`, `invalid_roster` or `prepared`. The worker records `activated` after registering the prepared arena. Entry records `entered` or `delivery_failed`. Avatar readiness records `ready` or `already_ready`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/battles.rs) | Garage return records `garage_reset`, `handoff_aborted` or `return_failed`. Battle command submission records `battle_queue_unavailable` and error status when the queue is full or closed. Vehicle controls received outside the arena record `arena_unavailable`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/admission.rs) | Login admission records `admitted`, `replaced`, `superseded`, `waiting_for_write`, `refreshing_snapshot`, `expired`, `full`, `destination_unavailable`, or `reply_unavailable`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | XMPP connection spans record `disconnected` on normal closure or `failed` on error. XMPP stanza spans record `handled`, `rate_limited` or `failed`. |
| [`output.outcome`](../src/player-administration/src/presentation/players.rs) | First-admin setup records `created`, `setup_closed` when setup is unavailable, or the provisioning error outcome. Field validation records `invalid_email`, `invalid_display_name` or `invalid_password`. |
| [`output.outcome`](../src/accounts/src/infrastructure/players.rs) | Account provisioning records `created` after commit or `conflict` after rolling back an existing username or a first-admin setup that is closed. |
| [`output.delivery`](../src/player-sessions/src/infrastructure/gateway/admission.rs) | Session replacement notice was `sent` to the old UDP peer, `unavailable` because it had no Account entity, or `failed` during enqueue or send. |
| [`output.outcome`](../src/player-sessions/src/presentation/store/mod.rs) | Store request result such as `success`, `stale`, `busy`, `unauthorized`, `insufficient_funds`, `research_required` or `failed`. The [dashboard catalog load](../src/player-sessions/src/infrastructure/gateway/store.rs) records `success`, `busy` or `failed`. |
| [`output.delivery`](../src/player-sessions/src/infrastructure/gateway/account_updates.rs) | Account update `queued` on the live garage connection or `next_sync` when recovered from persisted state on a later sync. |
| [`output.delivery`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | `mercury.response.resource` records `sent` after writing the resource header and every fragment to the socket. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Mercury dispatch records `dispatched`, `wire_decode_failed`, `decode_failed`, `unknown_method`, `entity_unavailable`, `dispatch_failed` or `unimplemented` on the root. The root uses `unimplemented` for exposed base methods without a server handler. Unimplemented account commands record `unimplemented` and error status on the decoded command span, including fire-and-forget requests. Compatibility paths record `ignored` or `acknowledged_without_change`. Garage and shop sync record `up_to_date`, `cache_hit` or `stream` as applicable. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Both Mercury chat methods record `unimplemented` and error status on `player_sessions.social.social_command` for unsupported actions. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/xmpp/protocol.rs) | Unimplemented XMPP IQ operations and unrecognized stanza types record `unimplemented` and error status on `player_sessions.xmpp.iq` and `player_sessions.xmpp.handle`, respectively. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Training Room listing records `listed` after sending a populated page, `empty` after sending a zero-room page, or `invalid_request` for malformed or unsupported queries. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Room restoration records `restored`, `no_membership` or `already_in_room` on initial garage sync. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Invitation batches record `sent`, `queued`, `no_recipients`, `invalid_request`, `host_required`, `rate_limited` or `delivery_failed`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Training Room creation records `created`, `already_hosting`, `already_in_room`, `invalid_request` or `unavailable`. Join records `joined`, `already_joined`, `already_in_room`, `invitation_only`, `invitation_unavailable`, `room_full`, `room_unavailable` or `invalid_request`. Settings changes, assignment and team swaps record `changed`, `assigned` or `swapped` on mutation, and `unchanged`, `invalid_request`, `room_unavailable` or `host_required` otherwise. Assignment also records `roster_full` when the destination has reached its advertised limit. Readiness records `ready`, `observer`, `not_ready`, `room_unavailable` or `vehicle_unavailable`. Leave and membership cleanup record `deleted` for a host, `left` for a guest, or `already_left`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training.rs) | Exclusion records `kicked`, `room_unavailable`, `host_required`, `invalid_target` or `invalid_request`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training/invites.rs) | Invite send records `sent`, `no_recipients`, `delivery_failed`, `invalid_request`, `host_required` or `rate_limited`. Decline records `declined`, `invitation_unavailable` or `invalid_request`. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/training/chat.rs) | Training text chat records `broadcast` or `delivery_failed` after enqueueing, and `not_in_room`, `invalid_message` or `rate_limited` on rejection. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/ammunition.rs) | Garage completions record `committed`, `unchanged`, `rejected`, `stale` or `error`. The Exterior decision records `changed`, `unchanged` or `rejected`. Mutation dispatch records `invalid_request`, `stale_shop`, `busy`, `queued` or `started` as applicable. |
| [`output.outcome`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Mercury Social dispatch records early rejection reasons. Its worker records `busy`, `started` or `completion_unavailable`. Admin session checks, write-header checks and credential verification record `authorized` or `rejected`. Session status records `setup_required`, `signed_out`, or `authenticated`. Registration and provisioning record `created`, `duplicate_login`, `duplicate_username`, `registration_closed`, `conflict` or `failed`. The [registration handlers](../src/player-administration/src/presentation/registration.rs) also record `invalid_email`, `invalid_username`, `invalid_password`, or `password_mismatch` for HTTP 422. Role changes record `updated`, `unchanged`, `actor_not_admin`, `own_role`, `player_not_found`, `email_required`, or `last_admin`. |
| [`output.session_count`](../src/accounts/src/infrastructure/player_access.rs) | Number of sessions removed by a sign-out request with a well-formed cookie. |
| [`output.delivery`](../src/player-sessions/src/infrastructure/gateway/progression.rs) | Garage completion reports response enqueue (`queued`), absent session or entity (`unavailable`), or failure to enqueue (`failed`). XMPP enqueue reports `queued`, `offline` or `enqueue_failed`. |
| [`output.delivery`](../src/shared-infrastructure/src/mercury/app/outbox.rs) | Mercury transport records `sent` on a successful socket write, `send_failed` on write failure, `enqueue_failed` on method encoding or enqueue failure, or `discarded` on removal of an unsent bundle. Login responses record `sent` or `send_failed`. |
| [`output.packet.sequence`](../src/shared-infrastructure/src/mercury/app/base/mod.rs) | 28-bit unreliable Mercury sequence number after an arena acknowledgement datagram is sent. |
| [`output.response_code`](../src/shared-infrastructure/src/mercury/app/entity.rs) | On-wire result from `onCmdResponse`, `onCmdResponseExt`, or the LoginApp reply. |
| [`output.players.online`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Number of current gateway sessions returned as cluster and region CCU by command 501. |
| [`output.revision`](../src/player-sessions/src/infrastructure/gateway/commands.rs) | Current in-memory garage revision returned by the sync check. Grant planning records the planned revision and the HTTP grant handler records the committed revision. |
| [`output.delivery`](../src/player-administration/src/presentation/garage.rs) | Successful HTTP grant reports `queued_for_garage` or `on_next_sync`. |
| [`output.item`](../src/accounts/src/domain/equipment.rs) | Compact descriptor of the upgraded device after successful planning. |
| [`output.equipment_upgrades.enabled`](../src/player-sessions/src/infrastructure/gateway/mod.rs) | Whether the Account bootstrap advertised the shared equipment upgrade flag `isTrophyDevicesEnabled`. |
| [`output.components.received`, `output.components.balance`](../src/accounts/src/domain/equipment.rs) | Components awarded for the priced line and resulting planned balance, in Components. Recorded after the line passes balance checks. A later line or an unsuccessful transaction can still reject the batch. |
| [`output.balance`](../src/accounts/src/domain/grants.rs) | Resulting balance in units of the selected currency on successful Accounts grant planning. |
| [`output.items.count`](../src/player-administration/src/presentation/garage.rs) | Number of grantable styles, crew skins, recruit groups, attachments, stat trackers, crew books or equipment entries returned by the admin catalog endpoint. The recruits endpoint counts groups across countries. |
| [`output.member`](../src/accounts/src/infrastructure/client/special_recruits.rs) | Account-local crew member inventory ID assigned by successful special-recruit grant planning. |
| [`output.owned.count`](../src/accounts/src/domain/cosmetics.rs) | Total copies of the granted cosmetic including fitted copies, or depot stock of the granted crew skin, crew book or equipment item, after successful planning. |
| [`output.purchases.count`](../src/accounts/src/domain/cosmetics.rs) | Number of cosmetic units to purchase after using owned stock. For paint, a region uses the number of units specified by the catalog. Zero for a successful change that spends only owned stock or leaves the layout as it was. |
| [`output.response`](../src/player-sessions/src/infrastructure/gateway/social.rs) | Legacy chat `actionResponse` code sent by `player_sessions.social.send_chat`. |
| [`output.stanza_count`](../src/player-sessions/src/infrastructure/xmpp/mod.rs) | Number of immediate replies returned by the XMPP handler, recorded before writing them. Does not count separate queued pushes or prove all writes succeeded. |
| [`output.success`](../src/player-sessions/src/infrastructure/gateway/grants.rs) | Whether a Social job or admin-grant completion contains an `Ok` result; does not by itself prove recipient delivery. |

## Startup, database and OpenTelemetry controls

| Attribute | Meaning and emission |
| --- | --- |
| [`input.resource_dir`](../src/server-runtime/src/lib.rs) | Configured extracted server resource-bundle path loaded at startup. |
| [`input.pool.max_connections`](../src/server-runtime/src/infrastructure/database.rs) | Configured maximum PostgreSQL pool connections on `server.connect_database`. |
| [`input.schema`](../src/server-runtime/src/infrastructure/database.rs) | Owning PostgreSQL schema selected for a context's embedded migrations. |
| [`input.ids.count`](../src/accounts/src/infrastructure/directory.rs) | Number of account IDs supplied to the published directory lookup. |
| [`db.system.name`](../src/shared-infrastructure/src/infrastructure/observability.rs) | Database system, always `postgresql` in the shared database tracing adapter. |
| [`db.operation.name`](../src/shared-infrastructure/src/infrastructure/observability.rs) | First SQL statement token, such as `SELECT`, `UPDATE`, `BEGIN`, or `COMMIT`; falls back to `QUERY` for an empty statement. |
| [`db.query.text`](../src/shared-infrastructure/src/infrastructure/observability.rs) | Complete executed SQL statement with parameter placeholders. Bind values are not interpolated or serialized. |
| [`error.type`](../src/shared-infrastructure/src/infrastructure/observability.rs) | Rust error type name when a database operation fails; omitted on success. |
| [`otel.kind`](../src/shared-infrastructure/src/infrastructure/observability.rs) | Tracing bridge control field set to `client` for PostgreSQL operations. Exported as the intrinsic span kind, not an ordinary attribute. |
| [`otel.status_code`](../src/shared-infrastructure/src/infrastructure/observability.rs) | Tracing bridge control field set to `ERROR` on failures such as database errors, rejected Exterior decisions, malformed progression input, Mercury decode and dispatch failures, XMPP request failures, and unimplemented game and chat operations. Exported as intrinsic span status. Diagnostic error events can also mark spans as failed. |

## Transport diagnostic spans

The Mercury [protocol tracker](../src/shared-infrastructure/src/mercury/net/proto.rs)
also declares legacy TRACE-level spans with implicit argument fields. The
production OTLP filter exports the server and runtime crate targets at INFO, so these spans and
their fields are not currently exported. The `pigb` TRACE span has no declared
fields. If the export filter is expanded, the following fields become relevant:

| Attribute | Meaning and emission |
| --- | --- |
| `addr` | Debug-form remote socket address on packet `accept`. Packet contents and the tracker itself are skipped. |
| `reliable` | Boolean specifying reliable bundle transmission on `prepare`. Bundle contents and the channel itself are skipped. |

## Metadata added by the tracing library

[Logging initialization](../src/server-runtime/src/infrastructure/logging.rs) uses the defaults of the
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
[telemetry initialization](../src/server-runtime/src/infrastructure/telemetry.rs).

| Attribute | Meaning and emission |
| --- | --- |
| `service.name` | Nonempty `OTEL_SERVICE_NAME`, otherwise the Cargo package name (`wot-server`). |
| `service.version` | Nonempty build-time `WOT_SERVER_VERSION`, otherwise the Cargo package version; release builds supply the release tag. |
| `wot.game.build_version` | Nonempty build-time `WOT_GAME_VERSION`. Release images use the bundled client version as formatted for the image tag. Omitted when not supplied. |
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


## Browser spans

Every browser span declares `ddd.bounded_context.name`. Browser service
resources carry `service.name` (`wot-server-web`) and `service.version` (the
serving release). Operation and transport outcomes use `output.outcome`
with `success`, `rejected`, `conflict`, `cancelled` or a bounded failure
code. On error, the exception event's `exception.type` is that failure code.
[Browser tracing](../src/shared-infrastructure/src/infrastructure/web/telemetry.ts)

| Attribute | Meaning and emission |
| --- | --- |
| `http.request.method`, `http.route` | Method and fixed route template on the browser HTTP client span. |
| `http.response.status_code` | HTTP status once a response arrives. |
| `http.request.body.size` | Serialized request payload size in UTF-8 bytes, on requests with bodies. |
| `input.page.route` | Fixed page route on browser entry-module load, `/dashboard` or `/shop`. |
| `input.page`, `input.per_page` | Requested admin list page and maximum rows per page. |
| `input.account_id` | Account identifier for an admin garage load, grant or role change. |
| `input.role` | Destination role on first-admin setup or a role change. |
| `input.catalog` | Catalog selected by the admin grant form. |
| `input.grant.kind`, `input.grant.currency`, `input.amount` | Grant type and, for a resource grant, currency and amount in units. |
| `input.vehicle_type`, `input.attachment_id`, `input.stat_tracker_id`, `input.inventory_id`, `input.item`, `input.count` | Applicable grant item IDs and cosmetic or depot grant quantity in copies. |
| `input.email.bytes`, `input.username.bytes` | UTF-8 byte lengths of the email and in-game username for self-service registration and managed player creation. First-admin setup records the email length. Username availability records the username length only. |
| `input.display_name.bytes` | UTF-8 byte length of the display name submitted during first-admin setup. |
| `output.username.available` | Boolean availability returned by the username check. |
| `output.player_count`, `output.vehicle_count`, `output.item_count` | Number of records returned by the player list, admin or personal garage, or grant catalog. |
| `output.session.status` | Admin session state returned by the status endpoint. |
| `output.delivery` | Delivery outcome returned by the admin grant API. |
| `input.item`, `input.revision`, `input.price.currency`, `input.price.amount` | Store order item ID, account revision string, price currency and integer amount. |
| `output.offer_count` | Offers returned by a Store catalog load or purchase. |
| `output.notification` | In-game Store checkout’s browser bridge outcome after a successful order. `submitted` indicates the bridge accepted the call. `bridge_unavailable` covers ordinary browsers and `submission_failed` records a synchronous bridge error. The span omits notification text. |
| `exception.source` | Fixed browser error category on a standalone exception span. |
| `exception.error_class`, `exception.asset_location` | Allowlisted built-in error class and bundled asset filename with line and column, when available. |

Supporting sources include the [browser HTTP adapter](../src/shared-infrastructure/src/infrastructure/web/http.ts),
[admin API adapter](../src/player-administration/src/infrastructure/admin-api.ts),
[Store API adapter](../src/player-sessions/src/infrastructure/store-api.ts),
[browser exception observer](../src/shared-infrastructure/src/infrastructure/web/browser-telemetry.ts),
[entry point](../src/server-runtime/src/presentation/web/main.tsx) and
[OTLP relay](../src/server-runtime/src/infrastructure/browser_telemetry.rs).

### Browser telemetry relay

`browser.telemetry.ingest` is a Rust server span for a browser OTLP POST. It
has `ddd.bounded_context.name=server` and covers forwarding through the
HTTP response. It omits browser span contents, origins and collector URLs.
[Relay implementation](../src/server-runtime/src/infrastructure/browser_telemetry.rs)

| Attribute | Meaning and emission |
| --- | --- |
| `http.request.method`, `http.route` | `POST` and the fixed `/api/web/telemetry` route. |
| `input.body.bytes` | Request body size in bytes, when the bounded body extractor succeeds. |
| `input.span_count` | Number of spans in a validated batch. |
| `http.response.status_code` | HTTP status returned to the browser. |
| `upstream.http.response.status_code` | HTTP status returned by the collector, when available. |
| `output.outcome` | `forwarded`, `origin_rejected`, `content_type_rejected`, `body_rejected`, `concurrency_limited`, `rate_limited`, `batch_rejected`, `collector_rejected`, `collector_timeout`, or `collector_unreachable`. Collector failures set span status to Error and emit a warning. |
