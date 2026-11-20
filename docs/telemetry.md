# Telemetry contract

Telemetry is required for server behavior. Every changed workflow must be
understandable through its trace, including its context transitions,
input, database work, result and response. Production sends traces to the
configured OTLP collector. See the [server guide](server.md#logging-and-telemetry)
for endpoint configuration, resource attributes and console filtering.

## Trace structure

Each game command, XMPP stanza, admin request and Store request starts an operation trace.
Spans below it represent meaningful boundaries and decisions in the owning
bounded context. The custom attribute `ddd.bounded_context.name` identifies
`player_sessions`, `accounts`, `social`, `player_administration`, or `server`
(server composition).
Omit architecture
layer segments (presentation, application, domain, infrastructure) from span names.
Name spans after the operation and meaningful subsystem instead.

Player Sessions traces use `player_sessions` as their context value and span
prefix. Historical traces may use `access` or `client_gateway`.
Player Administration traces use the context value `player_administration`
in place of `admin`. The `admin.*`, `game.command`, and `xmpp.request`
operation names remain stable.

A successful login that replaces an active session keeps the old client's kick
notice and the replacement client's login reply under the same authentication
trace. A login that waits for a pending account write records
`waiting_for_write` on its initial admission span and resumes within the
same authentication trace.

    game.authenticate
      player_sessions.admit                     [replaced]
        player_sessions.replace_session
          mercury.response                      [onKickedFromServer, sent]
        mercury.login.response                  [success, sent]

At startup, `player_sessions.bind_base_sockets` records the requested BaseApp
port, the port bound for both BaseApp UDP and native XMPP TCP and the number
of automatic port retries after TCP collisions. A fixed port collision fails
startup.

When the client enables entities, `player_sessions.enable` records the
Training Room visibility and Description field availability sent in the queued Account creation. Its
`mercury.response` child records delivery of `showGUI`.

The shared web listener composes the admin and Store APIs. Page shells and
shared assets emit `web.ui.request` with their matched route and HTTP status.
Admin and Store API requests keep their own request spans.

Managed player creation traces the input lengths and validation outcome
through `admin.players.create` and `accounts.registration.validate`. An accepted
request continues through account provisioning and its SQL transaction.
The browser sends the generated password to the creation API and the server
stores its hash. Neither the password nor the hash is a trace attribute.

A Store checkout joins the HTTP request to the account transaction and garage
delivery even though they run on separate workers.

    store.http.request                            [POST /api/shop/orders, 200]
      player_sessions.store.purchase_request
        player_sessions.store.purchase
          accounts.store.plan_purchase
            accounts.progression.plan_progression
              accounts.equipment.buy
          accounts.garage.commit_progression
            accounts.save_progression
              accounts.postgres.save_progression
                postgresql.query                  [BEGIN / UPDATE / COMMIT]
          player_sessions.store.complete_purchase
            player_sessions.publish_account_change
              mercury.response                    [update, sent]

A stale quote produces HTTP 409 with `output.outcome=stale` and no account
write. Store tokens and authorization headers are excluded from traces.

Experimental Equipment disassembly traces carry either Depot sale line counts or
the mounted equipment change through `accounts.progression.plan_progression`.
Each priced disassembly records the descriptor, quantity, per-item payout and
proposed Components balance. The same game command trace includes the
revision-checked database write and the Mercury cache update and reply.
Rejected requests report the failure and leave the account revision unchanged.

Equipment upgrades record whether the native feature flag was advertised on
`player_sessions.enable`. Upgrade decisions record the source descriptor,
catalog price in credits or Components, and upgraded descriptor. Mounted decisions also record the
owned vehicle ID and slot. Successful decisions and rejections retain the
game command parent through the SQL transaction when applicable and the
Mercury response.

An online XMPP private message follows this hierarchy. Repository operations
can contain several query and transaction spans.

    xmpp.request                              [player_sessions]
      player_sessions.xmpp.handle              [player_sessions]
        social.open                           [social]
          social.open                         [social]
            social.conversations.open_conversation
              postgresql.query                [BEGIN / SELECT / INSERT / COMMIT]
        social.send                           [social]
          social.send                         [social]
            social.conversations.send_message
              postgresql.query                [BEGIN / SELECT / INSERT / COMMIT]
        player_sessions.enqueue                [player_sessions]
          xmpp.response                       [player_sessions]

The response may run on the recipient's connection task. It carries the
originating request's parent through the bounded output queue. The enqueue span
records whether the recipient is offline, the stanza was queued, or the queue
was unavailable. Offline
delivery is stored by the original request. A later history replay belongs
to the recipient's history or presence request.

Mercury starts `game.command` before decoding a base method for the player's
entity. The root records the method and request IDs available at each decoding
stage. Use its dispatch outcome to identify unknown methods, invalid wire
data or missing entities. The `player_sessions.commands.command` span records
decoded scalar arguments and sizes. Its children show the operation that
actually ran. For example, command 501 returns the current in-memory count.

    game.command                                  [command 501]
      player_sessions.commands.command
        player_sessions.server_stats              [online count]
          mercury.response                        [receiveServerStats, sent]

Across client protocols, unimplemented operations set their operation span
status to `Error`, record `output.outcome=unimplemented`, and emit an
error-level diagnostic event. This applies to account commands, exposed
Mercury base methods, Mercury chat actions, and XMPP IQ operations or stanza
types. Failure replies remain in the originating trace. Fire-and-forget
requests emit the same error telemetry.

Use `{ resource.service.name = "wot-server" && status = error && span.output.outcome = "unimplemented" }`
in Tempo to find these operations. Mercury decode and dispatch failures set
the `game.command` root to `Error` with their own outcome.

Training Room discovery records the total room count, rooms returned and
players in those rooms. It records `listed` when the page contains rooms,
`empty` when it contains zero, and `invalid_request` for a malformed or
unsupported request. A valid request uses the garage resource stream. A
rejected request sends a command error.

    game.command                                  [command 520]
      player_sessions.commands.command
        player_sessions.list_training_rooms       [listed, 1 room, 2 players]
          mercury.response.resource
            mercury.response                      [onCmdResponse, sent]

Training Room creation records the requested arena, duration, open flag and
comment length. The join and room updates retain the original game-command
parent. A rejection sends `onPrebattleJoinFailure` under the same operation.
The subsequent ready command traces the owned-vehicle descriptor projection
and sends the roster update and command response under its own trace. An Observer
request carries reserved inventory value `-5000`, records `observer`, and uses the
cached catalog descriptor. Returning to Player traces the owned-vehicle lookup again.

    game.command                                  [accountPrebattle_createTraining]
      player_sessions.commands.command
        player_sessions.create_training_room      [created, room ID]
          mercury.response                        [onPrebattleJoined, sent]
          mercury.response                        [updatePrebattle, sent]

A public join records the requested room and cluster and the resulting
membership count. A rejected join sends `onPrebattleJoinFailure`. A guest
leaving records `left` and sends the updated roster to the remaining players.
Host leave records `deleted` and links the room removal, guest notifications,
host notification and command acknowledgement under the same command.

    game.command                                  [command 401]
      player_sessions.commands.command
        player_sessions.leave_training_room       [deleted, room ID]
          player_sessions.remove_training_member  [deleted, 0 players]
            mercury.response                      [guest onPrebattleLeft, sent]
          mercury.response                        [host onPrebattleLeft, sent]
          mercury.response                        [onCmdResponse, sent]

Training Room roster assignment records the requested player entity, slot and
group. A team swap records the previous and resulting slots. A valid request,
including an unchanged assignment or swap, sends a full roster and a command
acknowledgement under the originating trace. Invalid requests and full
destination rosters send a failed acknowledgement. Assignment records
`roster_full` when the destination is at capacity.

Each Modify Room command and voice-chat mode change retains its own `game.command`
trace. The decoded command span carries the numeric input or the comment's byte length. The
`player_sessions.change_training_setting` span links the room update and
acknowledgement and records the result. Room comment text stays out of traces.

Excluding a guest records the target Account entity ID and room. The
exclusion trace includes roster removal for the remaining players, the
guest's kick notice and chat close, and the host's command acknowledgement.

Training Room invitations record recipient counts and the optional message's
byte length. Each queued invite and its delivery retain the original command
trace. Accept, decline and room cleanup trace the resulting invitation
state. The invitation message and recipient list stay out of spans.

    game.command                                  [accountPrebattle_sendPrebattleInvites]
      player_sessions.commands.command
        player_sessions.send_training_invites      [sent, 2 recipients]
          player_sessions.publish_training_invite  [active]
            mercury.response                      [update, sent]
          player_sessions.publish_training_invite  [active]
            mercury.response                      [update, sent]

Training text chat opens and closes under the room membership trace.
Each broadcast records the message size, the room and the number of queued
recipients. The client request acknowledgement and each outgoing message
retain the original Mercury command trace. Chat text stays out of spans.

    game.command                                  [messenger_onActionByClient_chat2]
      player_sessions.commands.command
        player_sessions.social.social_command
          player_sessions.broadcast_training_chat [broadcast, 2 recipients]
            mercury.response                      [chat message to sender, sent]
            mercury.response                      [chat message to guest, sent]
            mercury.response                      [request acknowledgement, sent]

A committed research command follows this path. Intermediate Accounts and
Player Sessions spans retain their own bounded context.

    game.command                                  [command 201]
      player_sessions.commands.command
        player_sessions.gateway.change_progression
          accounts.progression.plan_progression   [research]
            accounts.modules.research
          accounts.progression.progression_diff
          accounts.prepare
            accounts.project_garage
            accounts.encode_garage
          accounts.garage.commit_progression
            accounts.save_progression
              accounts.postgres.save_progression
                postgresql.query                  [BEGIN / UPDATE / COMMIT]
          player_sessions.gateway.progression_response
            mercury.response                      [update, sent]
            mercury.response                      [onCmdResponseExt, code 0, sent]

Settings and ammunition use the same trace across their workers and
completions. Invalid, stale-shop, busy and unchanged requests can return
before any SQL runs. Queued crew commands retain their request parent while
waiting for earlier writes.

Each Mercury client method response in a traced operation retains its span in
the outbox until the bundle is sent, fails to send, or is discarded. A bundle
can contain replies from independent requests. Each reply keeps its own trace
and receives the bundle's send outcome. `sent` means the local UDP write
succeeded. Reliable ACKs and retransmissions run independently. Resource
streams write their fragments inside `mercury.response.resource`.

Login admission carries the authentication trace through SQL, garage cache
preparation and the login response queue. The `mercury.login.response` span
records the LoginApp protocol code and the UDP send outcome. The later BaseApp
handshake uses a separate protocol exchange.

Admin grants carry the HTTP request parent into the gateway's bounded
queue. The admin session check and handler are siblings under the HTTP root.
The session check traces database authorization, and the grant handler traces
inventory processing. Browser session status, sign-in, sign-out, first-admin
setup, player registration, username availability, the personal garage and
role changes propagate the same W3C parent through HTTP and SQL. Input lengths,
account IDs, role choices, availability and outcomes are allowlisted. Passwords,
email values, player-entered registration usernames and cookie secrets stay out
of these spans.
A connected player's garage update has a Mercury response span under the grant
completion.

Exterior traces record the shop revision, target inventory ID, outfit
count and encoded byte length. The Accounts decision records the action and
new units purchased. Personal number digits stay out of telemetry. Repeating a layout records an unchanged result with
no PostgreSQL write. A successful PostgreSQL commit is followed by a
progression completion that records the cache and response enqueue result.

Spans describe actual calls and work. An operation using only cached state
can have fewer children. Pure accessors and mechanical helpers do not need
spans. Cross-context calls retain their own context attributes.

## Attributes and outcomes

Use tracing::instrument with skip_all and an explicit field allowlist.
The [span attribute reference](telemetry-attributes.md) records every declared
span field, its meaning and emission conditions, along with library metadata
and resource attributes. Update it in the same change whenever fields or their
semantics change.
Include account IDs, command/request IDs, revisions, scalar command arguments,
enum choices, pagination, item counts, payload byte lengths, message
recipients and stanza IDs where relevant. Record output counts, response
codes, persistence outcomes and delivery information. Bound client-supplied
strings before attaching them. Keep player identity out of metric labels.

XMPP spans include stanza name, bounded stanza ID, input bytes, body size
and destination account ID. Mercury dispatch records command IDs, correlation
IDs, scalar arguments and array lengths. Responses include byte lengths or
business outcomes where available. HTTP traces include method, path and
response status. Business layer spans select the IDs, counts and values
relevant to their operation.

The root `admin.http.request` span carries route-specific inputs so a trace
can be filtered without opening child spans. The player list exports supplied
numeric pagination parameters. Garage routes export the parsed account ID,
and grants export their kind with the currency and amount, vehicle type, or
item and quantity. The root records crew book, Bounty Equipment and Experimental Equipment
descriptors in `input.item` and copies in `input.count`. Accounts records the
resulting depot stock or balance after validation. The grant handler records
the committed revision and delivery result.
Player creation exports the UTF-8 lengths of the login and display name.
Handlers record these fields after authentication and typed decoding, before
business validation where applicable. A rejected query or JSON body can still
be debugged using the status and available size metadata. The raw query, raw
JSON and credential headers are excluded.

Passwords, hashes, auth tokens, private keys, database connection credentials
and complete authentication envelopes are excluded. Message content and
binary data are represented by sizes and identifiers. User identity
attributes follow the [server guide](server.md#logging-and-telemetry). Emit
user.name from an available, nonempty player display name, never by copying an
email login. Emit user.email only when supplied by the current payload. Omit
unavailable attributes and never query the database solely for telemetry enrichment.

Failed operations emit diagnostic error events inside their span. The
OpenTelemetry layer uses these events to mark error status. Expected business
rejections, stale revisions and unchanged results remain distinguishable
through completion outcome attributes. A handled failure remains visible
even when writing its protocol error response succeeds.

## PostgreSQL

The [Platform database tracing adapter](../src/platform/infrastructure/observability.rs)
awaits application-issued queries and explicit transaction boundaries inside
`postgresql.query` spans. It records the context of the caller.

| Attribute | Meaning |
| --- | --- |
| ddd.bounded_context.name | Owning context |
| db.system.name | postgresql |
| db.operation.name | SQL operation such as SELECT, UPDATE, BEGIN or COMMIT |
| db.query.text | Complete SQL command with parameter placeholders |
| error.type | Error type when execution fails |
| OpenTelemetry status | Error when execution fails |

Driver-managed migration internals and implicit rollback on a dropped
transaction are represented by the enclosing repository operation and its
error. Explicit transaction boundaries receive query spans.

Duration covers the awaited operation, including pool acquisition when
execution uses the pool. BEGIN and COMMIT receive separate spans. SQL text
is the same string passed to SQLx. Bind values stay separate from the
statement, preserving the full command without serializing credentials or
large data fields.

## Propagation and verification

Use instrument(span) or in_current_span() on async futures. Wrap blocking
closures with observability::in_current_span. Carry a cloned parent in queue
envelopes and enter it around synchronous completion handling. Entered span
guards must not cross an await. Long-lived socket and gateway loops must
not accumulate independent requests into a single trace.

When the game session ends and revokes the chat credential, the XMPP
connection closes with `output.outcome=disconnected` and
`output.disconnect.reason=session_revoked`. The connection span stays open
through offline presence cleanup. XMPP connection failures set error status.

The telemetry capture helper under tests/support inspects exported spans
from normal garage and XMPP flows. Regression coverage verifies same-trace
ancestry through application, domain and repository work, complete SQL text,
UDP socket sends, XMPP delivery and database failure status. It checks that
the server stats trace contains its memory read and UDP send, and that invalid
or unchanged ammunition layouts perform no SQL. The server telemetry test
checks OTLP resource export through the server executable.

Contributions must extend these checks for changed workflows. The PR must
record the request-to-response trace shape and verification results,
including relevant errors and rejections. See the
[testing guide](testing.md) for the opt-in PostgreSQL setup.

## Browser traces

The admin dashboard and Store export service `wot-server-web`. Each tracked
workflow starts an operation span and passes its context explicitly to the
HTTP client span across asynchronous calls. The client sends W3C
`traceparent` on same-origin API requests. Admin and Store HTTP middleware
extracts a valid, sampled parent before entering the request span. Missing,
malformed, duplicate and unsampled headers start an independent server
trace. Baggage and tracestate are ignored. Queue, database and delivery spans
then use the same trace ID through the existing Rust propagation.

The expected sequence for a purchase is `web.store.purchase` →
`web.http.request` → `store.http.request` → Store use case → SQL and garage
delivery. Dashboard checkout follows the same sequence through
`admin.http.request`, with session lookup before the Store request. The
dashboard transport creates the child `web.http.request` span in
`player_administration` and preserves the parent Store operation in
`player_sessions`. Dashboard
catalog reads continue through `player_sessions.store.load_catalog` to the
Accounts snapshot and catalog projection. A grant follows the equivalent path through `web.admin.grant` and
`admin.http.request`. Registration follows `web.panel.register` →
`web.http.request` → `admin.http.request` → `web_panel.registration.create` →
Accounts validation and provisioning spans, including the SQL transaction.
Registration validation failures record the rejected field as a bounded
outcome on the handler span and return HTTP 422.
First-admin setup follows `web.admin.bootstrap` → `web.http.request` →
`admin.http.request` → `admin.players.bootstrap` → `accounts.bootstrap.validate`.
It traces the submitted email and display-name byte lengths. Accepted requests
provision the administrator in Accounts. Invalid fields return HTTP 422 with
a bounded handler outcome before credential hashing or account creation.
Expected browser cancellations, rejections and conflicts
record an outcome. Unexpected HTTP, network and decoding failures record
error status and a diagnostic event. Window errors, failed resources,
unhandled rejections and React error callbacks record `web.exception`
spans. The browser omits exception messages, raw stacks, credentials,
headers and free-form response text.

When serving the browser assets, opt in with both environment variables:

| Variable | Value |
| --- | --- |
| `WG_WEB_TELEMETRY_ORIGIN` | Exact browser origin, such as `https://wot.example.com`. |
| `WG_WEB_TELEMETRY_ENDPOINT` | Complete, fixed OTLP/HTTP protobuf trace endpoint inside the deployment, such as `http://alloy.telemetry.svc.cluster.local:4318/v1/traces`. |

The browser discovers the same-origin `/api/web/telemetry` endpoint before
enabling tracing. The relay accepts origin-matched protobuf POSTs, applies
a 32 KiB request limit, 24 spans per batch, four concurrent forwards and
30 batches per ten-second window per server replica. Collector calls have a
two-second deadline. The relay assigns browser service identity and forwards
to the same trace pipeline as the server. Keep collector ingress private and
preserve `traceparent` when proxying the admin and Store APIs. Browser spans
are untrusted client reports, and the browser attempts a best-effort flush on
page exit.

In Tempo, search for `{ resource.service.name = "wot-server-web" }` and open
a grant or purchase trace. Check that the browser HTTP span is the direct
parent of the Rust HTTP span, and that the trace contains the relevant SQL
and delivery spans. See the [browser attribute inventory](telemetry-attributes.md#browser-spans).

## Request source IP

Request roots (`admin.http.request`, `web.ui.request`, `game.authenticate`, `game.command`, and
`xmpp.request`) expose `client.address` for searching by caller IP and
`network.peer.address` for the immediate socket peer. Source ports are separate
attributes; a forwarded HTTP caller has no `client.port` because the proxy
header supplies only an IP. Child operations keep their existing trace ancestry.
See the [attribute reference](telemetry-attributes.md#source-addresses).

For HTTP behind a reverse proxy, set `WOT_ADMIN_TRUSTED_PROXY_IPS` to a
comma-separated list of exact IPv4/IPv6 proxy socket addresses. The default
(empty/unset) trusts no forwarding headers. Invalid configuration fails startup.
Only a single valid IP in a single `X-Forwarded-For` header is accepted from a
trusted peer. Missing, malformed, repeated, oversized, or comma-separated values
fall back to that peer's socket address. Raw header text is never exported.
`Forwarded` and `X-Real-IP` are ignored. The trusted edge proxy must overwrite
`X-Forwarded-For` from its accepted socket, rather than forwarding visitor input.
Do not trust a broad subnet or a shared gateway used by untrusted clients.

Caddy can use `header_up X-Forwarded-For {remote_host}` inside `reverse_proxy`.
Keep Caddy's incoming proxy trust disabled when it is the public edge. The
[official Caddy documentation](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#defaults)
describes its default header protection. Configure the server with the address
it actually sees from Caddy after Kubernetes routing, which can be the node's
CNI bridge IP when Caddy uses host networking.

Mercury and XMPP always use the socket source and do not parse proxy headers.
Preserve source IPs through router forwarding and Kubernetes: host-port DNAT
must avoid external SNAT; NodePort/LoadBalancer Services need
[`externalTrafficPolicy: Local`](https://kubernetes.io/docs/tutorials/services/source-ip/).
An upstream NAT that replaces the source limits visibility to that NAT address.
After deployment, compare request roots with the known caller address, including
a dashboard request with a forged forwarding header and an external game login,
command, and XMPP stanza. Local NAT-loopback tests alone cannot establish public
source preservation.
