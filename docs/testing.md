# Testing

The CI workflow runs Rust formatting, Clippy, workspace tests, the shared web UI
build, and E2E Tests on pushes to `main` and on pull requests. Both Rust jobs
cache Cargo downloads and compiled dependencies between runs. CI does not need
the research submodules or a game installation.
Container builds and publishing run only when a Git tag is created; see the
[container guide](container.md).

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --locked --doc -p wot-server
cargo test --workspace --all-targets --features integration-test-support
```

The server doctests check that Rust rejects access to private context APIs.
The integration-test feature exposes fixture helpers for storage and protocol
tests. Build the production binary with the default features.

The integration suite builds a synthetic, rebased AMD64 PE in memory, invokes
the real CLI and compiles its generated Rust. It verifies initializer ordering
that differs from function address order, range expansion, callback branch
execution and refusal of unsupported calls, RTTI/vtable extraction, malformed
inputs and output overwrite rejection. Fixtures contain no game binary bytes.

To validate the supplied client explicitly:

```sh
WG_GAME_FOLDER=/path/to/game cargo test -p wot-binary-map --test supplied_client -- --ignored
```

This opt-in test checks the documented executable hash, interface sizes and
ranges and the recorded toolkit movement callback lengths. It also compares two
runs for determinism and compiles generated Rust. It is not run in CI because
the game executable is an external input. See [findings](binary-mapping.md) for
version boundaries and expected evidence.

Context maps use `cargo context-maps` with Java 17+ and
Graphviz on PATH. The renderer validates CML, checks the pinned Context Mapper
archive's SHA-256, normalizes randomized graph attributes and publishes SVGs
only after all models render. The workflow regenerates them on `main`.

## Browser pages

From the repository root, use Node 22.22.2 or a newer supported LTS and run:

```sh
npm ci
npm run check --workspace tools/web
npm run test --workspace tools/web
npm run build --workspace tools/web
```

Vite+ runs formatting and lint checks and executes the Testing Library suites.
TypeScript runs in strict mode. Update formatting with
`npm run format --workspace tools/web`. The tests render the real administration
and Store pages, use the accessible controls, and intercept HTTP at the
network boundary with MSW. They cover admin sign-in, player creation, garage
grants, Store catalog and checkout, stale offer recovery and browser exception
privacy. Trace assertions match the propagated `traceparent` to the exported
client span and its operation parent. The Rust HTTP tests check that the
exported server span continues that remote parent into downstream work.

## E2E Tests

The [E2E Tests job](../.github/workflows/ci.yml) starts a fresh PostgreSQL 18 service
and copies the EU SD English `2.4.1.5514` resource bundle from the public
`2.4.1.5514-0.9.0` server image. It pins the image by digest so later releases
do not change the fixture. The job builds and tests the checked-out Rust code
and uses the image only to read the resource files. It needs no registry
credentials, so it also works for fork pull requests. Update the image digest
and fixture build together when refreshing the CI resource bundle. The pinned
bundle includes camouflage and style definitions for garage customization and
Store purchases, as well as arena definitions for Training Room coverage.

The database-dependent tests are opt-in when running Cargo locally.

The default suite does not require PostgreSQL or proprietary resources. To exercise
embedded migrations, dashboard provisioning, Argon2 credentials, concurrent preference
writes, restart persistence and two isolated encrypted UDP clients:

```sh
docker compose up -d --wait
export WG_TEST_DATABASE_URL='postgresql://wot_server:local-development@127.0.0.1:54329/wot_server'
cargo run -p wot-extract -- /path/to/game --output /path/to/resources
export WG_RESOURCE_DIR='/path/to/resources'
cargo test --locked --lib initializes_context_storage -- --ignored
cargo test --locked --lib exports_admin_http -- --ignored
cargo test --locked -p wot-server --features integration-test-support --test garage -- --ignored
```

The admin HTTP tests send requests through the authenticated router and inspects
exported inputs, status and parent links for successful and rejected requests.
They authenticate a player created through the API, check their starter garage,
and verify that raw credentials and invalid input text stay out of spans. They
also exercise concurrent first-admin setup, case-sensitive credentials,
60-day session persistence across router and database-pool restarts, role
changes, concurrent demotion, revocation, expiration and sign-out. The access
lifecycle test uses a temporary PostgreSQL database.

For an existing database, set `WG_TEST_DATABASE_URL` to the same connection
URL used by the server. The test uses uniquely named temporary accounts and removes them after its network
worker ends. Use a development database. It uses the client's basic-login JSON envelope and verifies Account/showGUI, owned tank
cache isolation, all three resource streams, committed settings/revisions, explicit
unsupported-command failure, duplicate-login refusal and disconnect/reconnect with
capacity reclamation. A fresh connection pool also reads committed preferences.
With a freshly extracted bundle containing `scripts/arena_defs/`, the suite also
verifies Training Room creation for Studzianki, the host roster, the owned-vehicle
ready response, the bonus type and roster limits used by the native client,
switching to Observer and back, moving a player into an occupied team and
emptying the original team, room settings, and voice-chat changes. Two peers check the shared list
and player counts, the invitation-only lock, public joining, guest leave,
host deletion and cleanup on host session replacement. The bootstrap asserts
the Description field's enable flag, and a guest sees the description on
creation, in live edits and on the next list refresh. Both peers open the
Training text channel and exchange messages. The test verifies that leaving
or deleting the room closes the channel and that a former member cannot
send or receive room messages. The test also checks trace parent links for successful and rejected requests. Older bundles
exercise the unavailable response.
The same peers exercise [garage ammunition](ammunition.md): rejected requests
followed by valid unload/reload/purchase, depot conservation/deletions, balances,
receipts, idempotency and identical cache after reconnect. A fresh pool confirms
stored loadouts, competing commits share a revision guard, and lost ownership
rolls back the account write. Domain tests cover alternate prices, insufficient
funds and overflow without needing game assets.

The same peers exercise [module progression](modules.md): rejected indices and
counts followed by valid research, shop desync, buy/install, depot swaps, restart
persistence and competing revision-checked commits. Domain tests cover research
prerequisites, XP accounting and atomic turret/gun replacement.

The same peers exercise [tank research and buying](vehicles.md): reject malformed
and unresearched purchases, research the Pz. II, buy with crew and shells, verify
immediate inventory additions and prevent duplicate purchases. Reconnect,
fresh-pool reads and a fresh catalog restore the purchased tank.

The same test target exercises [admin grants](admin.md#grants) with an offline
grant, a grant during BaseApp handoff, all four live balance deltas, and a tank
grant racing a slot purchase. It checks inventory, ammunition, crew, duplicate
rejection and the full snapshot after reconnect.

The [crew flow](crew.md) exercises hiring, unloading and returning crew, ordered
role changes and retraining, major and bonus perks, free XP, crew books, passport
edits, skin stock and outfits, barracks expansion, dismissal, restoration and
undertrained conversion. It checks the cache after reconnect and covers invalid
group sizes, incompatible roles, stale shop revisions and unavailable stock.
The ISU-130 flow verifies Accelerate Crew Training on and off, repeat requests,
rejected toggles, incremental settings and the saved setting after reconnect.
Accounts domain tests exercise XP routing with and without acceleration.

The [equipment flow](equipment.md) covers Standard and Improved purchases,
Bounty and Experimental depot mounting, paid demounting, insufficient funds,
and balances and mounted equipment after reconnect.

The [Depot flow](depot.md) checks the enable flag, stock from unloading shells,
demounting equipment and buying consumables, and single and mixed bulk sales.
It verifies stale-shop and invalid-request handling, atomic rejection when
duplicate lines exceed stock, matching sale prices, cache deletions and
persistence across reconnect.

The [consumables flow](consumables.md) checks purchase receipts, Depot-first
Apply, compatibility, slot gaps, free reordering and remounting, directive
coexistence, atomic combined Auto Fill and the saved auto-resupply preference.
It checks the full cache again after reconnect.

The [directive flow](directives.md) covers native Apply and Buy More requests,
credit and bond charges, stock reuse, removal, auto-resupply settings, and
cache persistence across reconnect. It rejects invalid layouts, non-directives,
unavailable purchases, unsupported vehicles and stale shop revisions.

The same peers also exercise Contacts search, saved friends, private messages,
ignore enforcement and rejected chat requests followed by valid messages, plus
native XMPP authentication, offline persistence, history/reconnect, retention and malformed XML recovery;
see [Social verification](social.md#validation). It is a real PostgreSQL + UDP integration test, not a claim of rendered game UI or
a throughput benchmark. An RSA-encrypted login path is covered by the same harness.

Start `./target/release/wot-server` to validate the actual configuration, database,
resources, schema binding and sockets; stop with Ctrl-C after the ready log.
Settings are read exclusively from the generated file described in the
[server guide](server.md). Environment variables above belong to the integration
test harness, not the server. Live garage rendering passed with client `2.4.1.5514`;
see [evidence and limits](client-validation.md). The protocol test also checks
revision acknowledgement after a full cache and required startup settings.

Unit tests cover default configuration creation, preservation of edits, relative
path resolution, rejection of invalid settings, key reuse, and cumulative filtering
at all three logging levels with appended log contents. Endpoint
checks verify wildcard listeners advertise a concrete route-selected IPv4 address
and actual port. Local clients bypass explicit public overrides when BaseApp
listens on wildcard/loopback; non-loopback clients retain the override, including
its separately configured port. Concrete non-loopback listeners remain explicit. The garage integration tests bind BaseApp to `0.0.0.0:0` without reserving a port
ahead of time. Plaintext login checks automatic routing; encrypted login configures
an unreachable public override and verifies the local handoff still reaches the garage.

`tests/rsa_login.rs` requires no database or game assets. It sends a login encrypted
with a different RSA key, checks `InvalidData`, then verifies that the same LoginApp
accepts a valid encrypted request. It guards against packet-triggered server panics.

`tests/transport_resilience.rs` exercises malformed packets and subsequent valid
traffic, all encrypted padding values, fragment consistency/expiry, global and
per-channel capacity, reliable-send backpressure, login rate limiting, hostile
pickle shapes and 5000 generated packets. See the [resilience report](resilience-audit.md)
for the server-owned transport bounds and the scope of verification.
