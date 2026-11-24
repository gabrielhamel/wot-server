# Telemetry contract

Telemetry is required for server behavior. Every changed workflow must be
understandable through its trace, including its context transitions,
input, database work, result and response. Production sends traces to the
configured OTLP collector. See the [server guide](server.md#logging-and-telemetry)
for endpoint configuration, resource attributes and console filtering. The
server includes each runtime library’s Rust tracing target in its console and
OTLP filters. The E2E trace capture uses the same target allowlist.

## Server metrics

The server exports these OpenTelemetry metrics through the configured OTLP
collector. Each series describes one server process.

| Metric | Type | Unit | Measurement |
| --- | --- | --- | --- |
| `wot.server.uptime` | Gauge | Seconds | Time since telemetry started. |
| `wot.players.online` | Gauge | Players | Authenticated game sessions across the garage and battles, using the same count as in-game server stats. Pending login tickets are excluded. |
| `wot.battles.running` | Gauge | Battles | Activated arenas with participants, from loading through the departure of the last participant, including countdown and results. |
| `wot.battle.tick.duration` | Histogram | Seconds | Time for the worker to advance all active arenas and enqueue their outputs, including waits for output capacity. |
| `wot.battle.tick.delay` | Histogram | Seconds | Time between the scheduled tick deadline and the worker starting arena processing. |
| `wot.battle.tick.skipped` | Counter | Ticks | Number of subsequent deadlines the worker discarded when a tick started at least one full interval late. The interval is 100 ms. |
| `wot.mercury.reliable_sequence.drops` | Counter | Packets | Unreliable on-channel Mercury packets discarded because their last-reliable-sequence footer differs from the receiver's last contiguous reliable sequence. |
| `wot.db.operation.duration` | Histogram | Seconds | Elapsed time of each completed PostgreSQL operation observed by the shared adapter, including transaction boundaries. Sample counts give operation and error totals. |
| `wot.db.pool.connections` | Gauge | Connections | SQLx pool occupancy by `state`, reported as `idle` or `non_idle`. The latter is the pool size minus idle connections and can include connections being opened or checked. |
| `wot.db.pool.capacity` | Gauge | Connections | Configured maximum size of the shared SQLx pool. |

The gateway refreshes both count gauges on its own loop. The battle worker
samples duration and delay when at least one arena is active at the start of
its tick. Both histograms use buckets around the 100 ms interval. The worker
preserves the originating span with each queued command and delivery. Arena
preparation and its terrain and obstacle loads run in the Training Room or admin launch trace.
Player Sessions retains the launch span until it acknowledges activation or
rejects preparation. The simulation scheduler measures its own tick timings
independently of the preparation task. Mercury
delivery spans and position snapshot socket outcomes are recorded in the
gateway when it drains the queue. See the [metric instruments](../src/player-sessions/src/infrastructure/gateway/metrics.rs)
and [battle scheduler](../src/battles/src/infrastructure/worker.rs).

The [Mercury receive check](../src/shared-infrastructure/src/mercury/net/proto.rs)
counts each rejected packet once. The `direction` label is `behind` or
`ahead` in Mercury's wrapping sequence space. Both series start at zero.
The counter measures this specific transport rejection. Establishing packet
reordering or congestion requires additional network evidence. In Mimir,
query drops per five minutes by direction:

```promql
sum by (direction) (increase(wot_mercury_reliable_sequence_drops{job="wot-server"}[5m]))
```

PostgreSQL operation timing starts when the adapter polls the SQLx future, so
it includes pool acquisition time for operations that acquire a connection.
The histogram uses `ddd.bounded_context.name`, `db.operation.name` (allowlisted
SQL verbs with an `OTHER` fallback), and `outcome` (`success` or `error`). SQL
text, bind parameters and error messages are absent from metric attributes.
Pool gauges sample the shared pool at collection time. The server registers
these metrics after connecting and applying migrations. See the
[PostgreSQL instruments](../src/shared-infrastructure/src/infrastructure/observability/database_metrics.rs).

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
shared assets emit `web.ui.request` with their matched route, HTTP status and
asset source. They preserve a valid sampled W3C trace parent when supplied.
Admin and Store API requests keep their own request spans.

Dashboard battle launches keep the browser parent through authentication,
`admin.start_battle`, the gateway request and the arena worker. Accounts records
the selected dummy vehicle and loadout counts. Arena preparation and the
subsequent native client entry share the launch trace. The launch response
records the allocated battle ID and the count of client handoffs queued.

Managed player creation traces the input lengths and validation outcome
through `admin.players.create` and `accounts.registration.validate`. An accepted
request continues through account provisioning and its SQL transaction.
The browser sends the generated password to the creation API and the server
stores its hash. Neither the password nor the hash is a trace attribute.

Admin grants retain the browser trace parent through authentication, the
grant queue, Accounts planning, the PostgreSQL write and garage delivery.
The new grant paths record the style, skin or recruit catalog ID. Successful
style planning records the resulting total copies, and special-recruit planning
records the new crew member inventory ID.

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

**Start Battle** records the host's two team confirmations and the selected
room and arena. The launch request retains its trace through roster
preparation and the participants' `EnableEntities` acknowledgments.

```text
game.command                                      [doCmdInt3, 411]
  player_sessions.commands.command
    player_sessions.start_training_battle
      mercury.response                            [onCmdResponse]
      player_sessions.prepare_training_battle
        accounts.battle_vehicle                   [for each combat vehicle, outfit byte count]
          accounts.battle_mobility                 [equipment and supply counts, collision bounds availability, effective profile]
        battles.prepare                           [allocates battle ID and counts arena outfits]
          battles.load_terrain                     [when terrain is loaded]
          battles.load_obstacles                  [when obstacle collision is loaded]
          battles.enter                            [one per participant handoff]
            mercury.arena_delivery                 [arena elements and delivery]
```

Avatar commands record the player's account and entity ID. The readiness
command that starts the countdown also parents the period broadcast.
Arena cell commands trace the wire element and target entity IDs, the method,
and the argument byte count. The decoded battle control stays in the same
trace and records the gateway-local participant ID. Arena entry and participant
removal use that same identifier. A movement command parents the first changed
position snapshot. Aim commands retain their spans through the turret traverse
to the requested aim, with a child span for each resulting angle or server
marker update. Enabling the marker parents its first delivery. The marker
replicates to the owner and the packed angle replicates to battle peers.
Position replication records the tick and counts the recipients and successful
socket writes of the shared timestamped snapshot. `mercury.arena_snapshot`
records the peer, wire tick, sequence number and delivery result for the
datagram that also carries any pending acknowledgements. The driving command also
parents `battles.drive_vehicle` for the first simulation step, the driver's
`hull_aim` update, and any changed `engine_mode`, `wheels_scroll` and
`steering_angles` deliveries to battle peers. The step records the sampled
surface counts, hull pitch, airborne state at entry, resulting longitudinal
speed and yaw rate, and whether track-lock steering was used.
When the world step detects contact, elevated support, scenery destruction,
a held pileup, slope sliding, tipping or recovery,
`battles.world_motion` records the surviving displacement, contact counts and
hull roll. It also records whether turning contacted scenery and the retained
speed multiplier after a break. Projectile impact additionally records whether a hit exhausted a
scenery fragment's health. A pushed vehicle without its own drive span uses the initiating
vehicle's available drive parent. The world span then parents pose and chassis
replication and any scenery updates from the pushed hull's contact.

`battles.scenery` follows the world-motion or shot span for each nonempty batch
of newly broken fragments. It counts fragments with native identities, changed
chunks, numbered tree falls, projectile references and queued peer deliveries.
Map collision loading counts numbered trunks available for contact. An arena element
delivery retains this span as its parent through the gateway. A batch without
native identities records `no_native_identity`. A queue error marks the span
as failed and disconnects the affected peer.

Shots retain their parent through target selection, ground and obstacle
intersection, the wait for the tracer's flight, and the impact and health
deliveries. The same span parents the immediate muzzle and tracer deliveries
and records the shell kind, caliber, launch velocity, gravity, tracer recipient
count, effective range and scheduled flight time. Explosion resolution and
vehicle fire and repair ticks retain the shot parent. Explosion spans include
tree, vehicle and stun counts, and vehicle damage tick spans record repaired
devices and HP lost to fire. The child
`battles.resolve_armor` records the selected material and part index, nominal
and effective thickness, remaining penetration, damage before capping to
remaining HP, and the penetration outcome. The shot records cumulative flight
time through the next contact after a ricochet or a destructible fragment. It
also records the reflected tracer's range when a ricochet continues. Both
armor decisions and their deliveries share the shot span. An impact that
adds a persistent penetration or resisted mark records the retained sticker
count and parents a `battles.damage_stickers` span per recipient. Scene impacts
record distance along the ray and parent the `explodeProjectile`
deliveries. Obstacle impacts include the raw collision material kind. The same shot parents
the shooter-only result, the victim-only direction and the private damage
feedback for each. When the direction update is queued for the victim, the
shot records its yaw, blocked flag and displayed damage amount. The reload
initiator parents the simulation tick deliveries as the gun cooldown and
magazine refill complete. Unsupported methods emit an error span.
Target ownership, invalid input, inactive rounds, and reloading are distinct
outcomes.

Battle preparation traces the map geometry and the number of height tiles,
driving surface tiles and obstacle instances loaded from the extracted resource bundle. A native `setClientReady`
command parents the nested roster update and generation commit for both
peers. Departure sends the corresponding disconnected state. Arena transport
acknowledgements record socket delivery and the transport sequence number,
with no game payload.
Each arena entry records the viewer’s customization display preference and the
Avatar control mode, and parents delivery of the
control handoff to the client.

Training Room invitations record recipient counts and the optional message's
byte length. Immediate delivery stays in the invite command trace. Delivery
on garage sync links that trace to the original command for each pending
invitation. A returning participant's first garage sync also traces the room
roster, including how many members are still in battle, chat membership and
the fresh Account entity through
`player_sessions.restore_training_room`. Accept, decline and room cleanup trace
the resulting invitation state. Invitation text and recipient lists stay out
of spans.

`battles.period` records the winner, finish reason and delivery count at the
transition to the finished phase. The reliable outcome messages, coast-down
pose replication and scheduled return to the garage retain that trace.

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
completion. Grants and Store gold purchases of eligible `private` and
`testTank` vehicles use the same traces as other vehicle acquisitions. Catalog
initialization records accepted and rejected variant counts and a diagnostic
with the vehicle ID and key when the stock definitions fail validation.

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
item and quantity. The root records crew book, Improved Equipment, Bounty Equipment and Experimental Equipment
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

The [Platform database tracing adapter](../src/shared-infrastructure/src/infrastructure/observability.rs)
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
or unchanged ammunition layouts perform no SQL. The PostgreSQL test checks
exported operation outcomes and pool gauges across checkout, return, and pool
closure. The server telemetry test checks OTLP resource export through the
server executable.

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
enabling tracing. The relay accepts protobuf POSTs from the configured
origin. The dashboard and Store use `Referrer-Policy: no-referrer`, which can
serialize the origin of these same-origin POSTs as `null`. For requests with
a null or missing origin, the relay requires browser Fetch Metadata
`Sec-Fetch-Site: same-origin` and the configured public host and port in the
request authority. Reverse
proxies must preserve the public host and port. The relay applies a 32 KiB
request limit, 24 spans per batch, four concurrent forwards and 30 batches
per ten-second window per server replica. Collector calls have a
two-second deadline. The relay assigns browser service identity and forwards
to the same trace pipeline as the server. Keep collector ingress private and
preserve `traceparent` when proxying the admin and Store APIs. Browser spans
are untrusted client reports. The browser normally starts exporting buffered
spans after 500 ms, splits batches that exceed the relay's byte limit, and
attempts a best-effort flush on page exit. It reports final export failures
in the browser console with only the span count.

Use `browser.telemetry.ingest` to see whether an upload reached the server and
whether it was rejected locally or failed while forwarding to the collector.
The relay records its HTTP status, the collector status when available, and a
bounded outcome. Successful forwarding acknowledges delivery to the collector.
Browser and server spans can arrive at Tempo separately.

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
