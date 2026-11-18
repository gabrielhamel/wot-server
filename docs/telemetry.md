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

When the client enables entities, `player_sessions.enable` records the
Training Room visibility sent in the queued Account creation. Its
`mercury.response` child records delivery of `showGUI`.

The shared web listener composes the admin and Store APIs. Page shells and
shared assets emit `web.ui.request` with their matched route and HTTP status.
Admin and Store API requests keep their own request spans.

A Store checkout joins the HTTP request to the account transaction and garage
delivery even though they run on separate workers.

    store.http.request                            [POST /api/shop/orders, 200]
      player_sessions.store.purchase_request
        player_sessions.store.purchase
          accounts.store.plan_purchase
            accounts.progression.plan_progression
              accounts.equipment.buy
          player_sessions.commit_progression
            accounts.save_progression
              accounts.postgres.save_progression
                postgresql.query                  [BEGIN / UPDATE / COMMIT]
          player_sessions.store.complete_purchase
            player_sessions.publish_account_change
              mercury.response                    [update, sent]

A stale quote produces HTTP 409 with `output.outcome=stale` and no account
write. Store tokens and authorization headers are excluded from traces.

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

Training Room discovery records `empty` for a successful zero-room list and
`invalid_request` for a malformed or unsupported request. A valid request
uses the garage resource stream. A rejected request sends a command error.

    game.command                                  [command 520]
      player_sessions.commands.command
        player_sessions.list_training_rooms       [empty, 0 rooms]
          mercury.response.resource
            mercury.response                      [onCmdResponse, sent]

Training Room creation records the requested arena, duration, open flag and
comment length. The join and room updates retain the original game-command
parent. A rejection sends `onPrebattleJoinFailure` under the same operation.
The subsequent ready command traces the owned-vehicle descriptor projection
and sends the roster update and command response under its own trace.

    game.command                                  [accountPrebattle_createTraining]
      player_sessions.commands.command
        player_sessions.create_training_room      [created, room ID]
          mercury.response                        [onPrebattleJoined, sent]
          mercury.response                        [updatePrebattle, sent]

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
          player_sessions.commit_progression
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
setup and role changes propagate the same W3C parent through HTTP and SQL.
Email lengths, account IDs, role choices and outcomes are allowlisted. Passwords,
email values and cookie secrets stay out of spans.
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
attachment ID and count.
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
delivery. A grant follows the equivalent path through `web.admin.grant` and
`admin.http.request`. Expected browser cancellations, rejections and conflicts
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
