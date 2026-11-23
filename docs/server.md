# Running the server

The `wot-server` binary runs a LoginApp and BaseApp in one process. PostgreSQL is
mandatory. The binary can also serve the [HTTP API and dashboard](admin.md).
Startup reads an [extracted resource bundle](resources.md), validates Account
binding and applies embedded migrations without creating accounts or inventory.
Offline binary mapping remains available as a separate development tool.

## Running

Generate a bundle with [wot-extract](resources.md) and place it under
`target/release/resources` or set `resource_dir` to its location. From the
repository root, start the server. A source build requires Node.js and npm to
build the dashboard and Store bundle. See the
[dashboard guide](admin.md#enable-the-dashboard) for configuration:

```sh
cargo run --release
```

The built binary also starts directly:

```sh
./target/release/wot-server
```

On first start, the server creates **`server.json` beside the executable**, then
starts using that file. Existing configuration files are never overwritten.
With Cargo the file is `target/release/server.json`; moving the binary to its own
folder makes that folder the server's home. Relative resource and key paths resolve
against this folder, independently of the shell's working directory. Restart after
editing the file. See [logging and telemetry](#logging-and-telemetry) for the
console option and the OpenTelemetry environment variables.

The generated JSON contains the three required fields. All other fields are optional.

| Field | Default / meaning |
| --- | --- |
| `resource_dir` | Required. First-start file uses `resources`, the directory containing the generated bundle manifest and `res/` |
| `database_url` | Required. First-start file uses `postgresql://wot_server:local-development@127.0.0.1:54329/wot_server` |
| `login_bind` | `0.0.0.0:20016` UDP |
| `base_bind` | `0.0.0.0:20017` UDP for BaseApp and TCP for native XMPP. Port zero selects a port available on both transports. |
| `advertised_base` | `null`: automatically choose the route to each client; set e.g. `"192.168.1.10:20017"` for an explicit non-local handoff destination |
| `private_key` | Required. First-start file uses `login.pem`. Its public key is exported as `login.pubkey`. |
| `db_connections` | `16`, allowed range 1–256 |
| `max_clients` | `256`, admitted/pending handoff capacity, allowed range 1–256 |
| `max_pending` | `32`, authentication worker capacity, allowed range 1–256 |
| `training_room_visible` | `true`. Advertises the Training Room card at login and allows room creation when the resource bundle contains arena definitions. Set to `false` and restart the server to disable creation and hide the card for new logins. |
| `periphery_id` | `250`. Local server ID used for Training Room invitations. Match the `periphery_id` on the clients' server-list entry so the game accepts the invitation on the current server. |
| `log_level` | `"debug"`. Also accepts `"error"`, `"warn"`, `"info"`, and `"trace"`. |
| `web` | Serves the dashboard and Store embedded by Cargo on `0.0.0.0:8080`. Set `web.bind` to change the listener or `web.ui_dir` to serve assets from disk. Relative override paths resolve beside `server.json`. |
| `admin` | [Dashboard and admin API](admin.md) enabled. Session cookies support HTTP by default. Set `admin.secure_cookies` to `true` for HTTPS, or set `admin` to `null` to disable admin. |
| `store` | In-game [Store](store.md#configuration) enabled with origin `http://127.0.0.1:8080`. Set `store.origin` for remote clients or `store` to `null` to disable the Store. |

Unknown fields, invalid types and invalid limits stop startup with a diagnostic.
Missing required fields stop startup. In an existing configuration, replace `game_dir`
with `resource_dir` and point it to the generated bundle. The configuration contains the database password;
new files have owner-only permissions on Unix. Do not commit or share this file.

When BaseApp listens on all interfaces or loopback, a localhost client always
receives `127.0.0.1` and the actual bound BaseApp port, even when `advertised_base`
is configured. This avoids sending local sessions through the router's public
address. Other clients receive the explicit destination when configured, or the
route-selected interface address otherwise. A concrete non-loopback BaseApp bind
does not support this loopback shortcut. Client server-list entries on other
machines must point to this computer's reachable address on port 20016.

PostgreSQL is a separate service; start the existing Compose instance with
`docker compose up -d --wait` if stopped. Embedded migrations run automatically,
but startup never creates accounts or vehicles. Existing accounts and inventory
remain untouched. Create player accounts and their starter garages through
the [dashboard or admin API](admin.md). Integration tests create their own
temporary accounts.
Passwords use salted Argon2id hashes. The Compose DB port is bound to loopback.
The PostgreSQL volume survives `docker compose down`. The `--volumes` flag
deletes it. Ctrl-C gracefully stops the gateway.

The server uses one PostgreSQL database and one connection pool. Accounts and
Social run their own embedded SQLx migrations and track them in
`accounts._sqlx_migrations` and `social._sqlx_migrations`. For a new
installation, start with an empty PostgreSQL database. The database role must
be able to create schemas during the first startup.

The schema and migration owners are [Accounts](../src/accounts/README.md) and
[Social](../src/social/README.md). Player Administration and Player Sessions
consume their application contracts. Social stores numeric player IDs and
resolves them through the Accounts directory API. Social omits missing accounts
from roster and conversation responses.

### Logging and telemetry

The console defaults to `debug` on stderr, including protocol method names and
transfer sizes. Use `info` for startup and session events with less detail,
or `trace` for packet diagnostics. The CLI overrides
`log_level` in `server.json`; `RUST_LOG` is not read.

```sh
cargo run --release -- --log-level debug
```

To export logs, traces and metrics, set the collector's OTLP/HTTP base URL:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./target/release/wot-server
```

Use the ingestion endpoint, not the Grafana URL. The server uses
[OTLP over HTTP with protobuf](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
and defaults to the service name `wot-server`; set `OTEL_SERVICE_NAME` to
override it. The exporter appends the signal paths and supports standard OTLP
headers and timeouts. With the endpoint unset, the server does not export.

The collector receives server events and spans at `info` and above,
independently of the console level. Traces cover resource loading, database
startup and the [layered request workflows](telemetry.md). Metrics cover uptime,
online players, running battles, battle tick health, PostgreSQL operations and
connection pool occupancy. See the [metrics reference](telemetry.md#server-metrics).
Exports run in the background with bounded
queues. Collector outages do not stop the game server, but telemetry can be
lost. On SIGINT or SIGTERM, the server shuts down and flushes pending exports.

Telemetry resources include `service.version`, set to the exact Git tag by
the release image workflow. Local Cargo builds use the package version unless
`WOT_SERVER_VERSION` is supplied at build time. Release images also include
`wot.game.build_version` from the bundled version as formatted for the image tag.
`wot.game.version` comes from the loaded bundle's `game_version`, copied from
the source client's `version.xml`. Bundles without that field omit this attribute.
These resource attributes accompany logs, traces and metrics.

Authentication spans include `outcome`, `network.protocol.name`,
`wot.client.protocol_version`, the optional `wot.client.resource_digest`,
and `wot.client.language` from the login context when present.
Successful authentication adds the local account ID as `user.id` and the
available player display name as `user.name`. An unavailable or empty display
name is omitted. Email-shaped logins in the current request populate `user.email`;
this identifies the supplied login and does not establish email ownership.
Other requests omit email when it is absent from their payload. Telemetry never
queries the database solely to obtain a name or email.
Account IDs and email addresses are span attributes, so operators can search
individual player traces. They are not added to metric labels.

Game command spans retain their parent through worker execution and queued
completion handling. Child spans identify context, selected inputs,
domain decisions, database commands and response outcomes. XMPP traces retain
the same parent through application services, repositories and outgoing
delivery. The [telemetry contract](telemetry.md) defines the required hierarchy,
fields and verification for contributions.

The login packet does not provide a confirmed client release version or a
persistent installation ID. The protocol number and resource digest describe
what the peer sent, and the account ID identifies the authenticated player.
The decompiled reference at
`docs/references/wot-src/sources/res/scripts/client/connection_mgr.py:192`
constructs a context with `lang_id` and `publication`. Its applicability to
the latest client remains a research question. The
[client validation record](client-validation.md) identifies the executable
used for earlier observations.

## Server source layout

[Server composition](../src/server-runtime/src/lib.rs) handles startup, dependency wiring,
and graceful shutdown. [Server infrastructure](../src/server-runtime/src/infrastructure)
contains configuration I/O, PostgreSQL connection and migration orchestration,
login key storage, console and OTLP export, browser telemetry ingestion, and
static UI serving. Browser routing lives in
[server presentation](../src/server-runtime/src/presentation/web/main.tsx), and the
[web workspace](../src/server-runtime/src/infrastructure/web/package.json) bundles the browser application.

## Protocol implementation

The server owns its [Mercury adapter](../src/shared-infrastructure/src/mercury) for
UDP packet and bundle framing, LoginApp RSA, BaseApp Blowfish and dynamic
entity dispatch. [Player Sessions](../src/player-sessions/README.md) binds
Account RPCs for the garage. [Battles](../src/battles/README.md) binds arena
entities and controls through its synchronous arena delivery port. The
Mercury adapter maps participant IDs to live BaseApp connections, translates
incoming methods into typed battle commands and publishes native arena
updates. The gateway runs the battle tick in the same loop as the session
transport. Both adapters validate their method tables and creation layout
against the configured bundle. The [shared codecs](../src/shared-infrastructure/src/infrastructure/client_formats/README.md)
read packed resources and serialize Python values for Player Sessions and Accounts.

A standalone checkout builds with `cargo build --locked`. See the
[resilience report](resilience-audit.md) for packet limits and regression
coverage, and [third-party notices](../THIRD_PARTY_NOTICES.md) for provenance.

## Connecting the game

The real client's server list must contain this server. The existing toolkit's
`wg-toolkit-wot/src/main.rs` documents its proven Packed XML server registration
filter. Use a separate/local entry with `url` and `url_token` set to
`127.0.0.1:20016` (or this machine's reachable address), a unique name/short name,
and an unused `periphery_id` (250 in the validated installation; 205 is EU5). Preserve a backup of `res/scripts_config.xml` before editing.
The binary does not automatically change client files. The installed client was
configured and tested separately; see [live validation](client-validation.md).

RSA-encrypted login is enabled by default. Startup reuses the configured
`private_key` and exports its matching public key alongside it with a `.pubkey`
extension. If absent, it generates a 2048-bit PKCS#8 key with owner-only permissions
on Unix; existing keys are never replaced. Preserve the key when moving the server.
For this already-configured installation, the saved `.local/login.pem` identity
is selected directly by `private_key` in `target/release/server.json`, preserving
the game client's matching public key. Keep the identity outside `target/`: cleaning
build outputs must not remove the key. The default `login.pem` is appropriate when
the binary lives in a persistent server folder. A missing key generates a new
identity, which requires updating the public key in all configured clients.

The [validated client entry](client-validation.md) references a file in the
client's `res/` directory that must match the exported public key. The existing
configured installation already has the matching key. If you generate a new
key, copy its public key into each client that uses the server. BaseApp traffic uses the client's negotiated Blowfish key. Use
username/password login for an existing account; official Wargaming launcher/SSO
tokens are not local database credentials.

### Reconnecting and replacing a session

Sign in again after a client crash or to move the same account to another
machine. After successful authentication, the server retires the old garage
session or unused handoff ticket. Outstanding account writes finish before the
replacement reads its garage state. The server continues to retire clients
that stop sending traffic after two minutes, even if they never sign in again.

If the old client has an Account entity, the server attempts to send the
account replacement notice before removing its UDP peer. This uses
`Account.onKickedFromServer(STRING, UINT8, UINT32)` from the versioned
`scripts/entity_defs/Account.def` in the resource bundle. In the pinned
decompiled client, `Account.py` forwards the notice to
`connection_mgr.py`, which disconnects the client and reports the reason.
`constants.ACCOUNT_KICK_REASONS.LOGIN_TO_OTHER_GAME` is 1 in that reference.

## Runtime contract

1. Login extracts `login` from the client's JSON username envelope when
   `auth_method` is `basic`, then validates username/password asynchronously against
   PostgreSQL. Plain usernames remain supported for protocol tools.
2. A random one-use handoff ticket binds the account and negotiated cipher to the
   source IP, expires after 30 seconds and is installed before login success.
3. BaseApp consumes the ticket, negotiates the session and announces 10 Hz updates.
4. After `enableEntities`, it creates Account and sends `showGUI` with the actual
   database ID. No networked garage space or Vehicle entity is required.
5. `CMD_SYNC_DATA` (100), `CMD_SYNC_SHOP` (300) and `CMD_SYNC_DOSSIERS` (600)
   return zlib-compressed Python pickle resources, correlated to request IDs.
   Resource descriptions contain `(compressed length, signed CRC32)`.
6. Integer preference writes (1600) update JSONB and revision atomically, then
   rebuild the session projection and send the committed diff/response. A stale/initial
   revision receives a complete snapshot; the follow-up request at the current
   revision is acknowledged with `onCmdResponseExt` to finish synchronization.

Shop prices are loaded once from resources and shared; account projections are
prepared outside the UDP loop and shared by reference during sync. Argon2 work and compression use Tokio's blocking
pool. SQLx uses a shared asynchronous connection pool. Password jobs, result
queues, handoffs and admitted clients have explicit bounds. Simultaneous logins
for the same account are refused; different accounts have separate snapshots and
entities. Disconnect messages and silent client expiry reclaim capacity.

Atomic JSONB merge and revision increments prevent concurrent preference writes
from losing unrelated keys. Reads use a repeatable-read transaction to keep
balances, ownership, settings, progression and ammunition coherent. Player
creation persists the account and its starter garage in one transaction.
Migrations are embedded and tracked by SQLx; no separate migration CLI is required.

## Evidence and limits

The [live validation record](client-validation.md) documents earlier client
walkthroughs, including login and garage rendering. The
[client breaking changes](client-breaking-changes.md) record ABI changes
observed across client updates. See the [testing guide](testing.md) for the
current automated checks.

Supporting sources:

- Toolkit `wg-toolkit-wot/src/base/{mod,account,shop}.rs`: working prototype bootstrap,
  payload shape, command responses and resource streams.
- Toolkit `wg-toolkit-wot/src/wot/{schema,items,item_defs,item_prices}.rs`: schemas,
  descriptors and installed catalog readers. Adaptations here remove battle-only
  resource/physics loading and replace global account constants with snapshots.
- `docs/references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/login/login_modes/credentials_mode.py`
  and `common/external_strings_utils.py`: production basic login validates an email-shaped identifier.
- `docs/references/wot-src/sources/res/scripts/client/connection_mgr.py` (15, 170–190): basic/token
  method names and JSON login envelope; `client_common/pwd_token.py`: English
  passwords pass through, while the CN/VN paths transform them (not supported).
- `docs/references/wot-src/sources/res/scripts/client/Account.py`: `onBecomePlayer` (248) and
  `onStreamComplete` (424); `account_helpers/AccountSyncData.py` (150) requests full sync;
  `account_helpers/SyncController.py` decompresses/unpickles the resource.
- `docs/references/wot-src/sources/res/scripts/client/gui/ClientHangarSpace.py`: local space and
  `HangarVehicle` creation. Its decompiled tree reports `v.2.4.1.1 #993`, EU;
  treat it as corroboration, not proof of executable build identity.
- `docs/references/BigWorld-Engine-14.4.1/programming/bigworld/lib/connection/baseapp_ext_interface.hpp`
  and `common_client_interface.hpp`: historical connection framing references.
  Modern framing and method order come from the toolkit and installed resource
  model, not an assumption that the old engine's ABI still matches.

Vehicle ownership, balances, preferences and ammunition are durable. The shell
panel can load/unload, reorder and buy missing shells using shared depot stock;
see [garage ammunition](ammunition.md). Tank modules can be researched, bought,
installed and swapped from the shared depot. See [module progression](modules.md).
Equipment can be bought from the shop, mounted from the depot, or bought and
mounted together from the equipment panel. See [garage equipment](equipment.md).
[Garage consumables](consumables.md) covers purchases, slot changes, combined
Auto Fill and the consumable auto-resupply preference.
[Garage directives](directives.md) covers buying, mounting and removing crew
and equipment directives, and storing the auto-resupply setting.
The [Garage Depot](depot.md) displays shared stock and supports single and bulk
sales of unmounted equipment, modules, shells and consumables.
Tank research, permanent vehicle purchases and sales, and garage slot purchases are covered in
[tank progression](vehicles.md). The [crew guide](crew.md) covers hiring,
assignments, barracks, retraining, perks and the related shop operations.
Stock modules come from the extracted vehicle definitions. Battle entry,
matchmaking and results remain outside the garage server.
Contacts search, durable directed friends/ignored users, presence, offline private
messages and retained history use the native XMPP TCP endpoint authenticated through
the game connection; see [social protocol](social.md).
Dossiers are empty; external web-service token payloads are placeholders; type-1 XMPP tokens are game-session credentials; two optional UI
initialization commands are acknowledged without gameplay effects. Unsupported
commands in the bound subset receive an explicit failure. Unbound telemetry and
unknown methods may be ignored by the server.

The runtime supports multiple active clients by multiplexing UDP events and using
bounded asynchronous database work. It has not been load-tested for production
capacity. Large resource writes still use synchronous UDP sends.
Transport budgets and login rate limits are implemented; fair resource scheduling,
SSO, further operational hardening and
cross-platform real-client validation remain future work. External DB mutations
are observed on next login; new catalog keys require restarting the server.

Native chat uses TCP on the BaseApp port as well as UDP. Forward/allow both
transports for remote clients; see [native chat endpoint](social.md#native-endpoint-and-authentication).
