# Context map

See the [bounded context guide](docs/bounded-contexts.md) for naming criteria,
published contracts, and storage ownership rules.

## wot-server workspace

Accounts, Social, Player Sessions, Battles, and Player Administration compile
as separate Rust libraries under `src/`. The server binary starts the [server
runtime library](src/server-runtime/README.md), which wires the contexts’ published
contracts and adapters.
The [web workspace](src/server-runtime/src/infrastructure/web/package.json) bundles the context-owned browser pages.

![wot-server context map](docs/context-maps/generated/wot-server.svg)

[Shared Kernel](src/shared-kernel/README.md) supplies the account ID model to
Player Administration, Player Sessions, and Social.
[Platform](src/shared-infrastructure/README.md) supplies shared codecs to Accounts and
Player Sessions, shared Mercury transport to Player Sessions and Battles, bounded HTTP serving to the shared web listener, and
tracing adapters to runtime infrastructure and server composition. The admin and Store browser views use Platform's TypeScript HTTP transport, telemetry and theme. Both support boundaries are independent of their consumers.

[Accounts](src/accounts/README.md) publishes commands, read operations and an
opaque account view upstream of [Player Sessions](src/player-sessions/README.md), which adapts them into login
and garage sessions. Both translate external client data through anti-corruption
layers. Accounts' PostgreSQL adapter persists user state, including revision-checked
ammunition, tank/module research, vehicle purchases and module, equipment, consumable and directive loadouts, cosmetic ownership and outfits, shared depot stock, balances and Premium Account expiration. Its resource adapter builds the garage projection. Player Sessions adapts shell, equipment, consumable, directive, camouflage, module and tank progression requests and publishes committed
inventory/balance changes and purchase receipts. Player Sessions binds garage RPCs to the extracted script definitions. Its [Store HTTP adapter](docs/store.md)
uses the live garage session to publish Accounts offers and route checkout through
the revision-checked account transaction. Player Sessions publishes Premium Account
activation, renewal and expiry to the garage. Both contexts use the shared
[client formats](src/shared-infrastructure/src/infrastructure/client_formats/README.md) infrastructure.

[Resource Extraction](tools/wot-extract/README.md) runs offline and publishes
the [bundle contract](src/resources/README.md) to Accounts, Player Sessions and Battles. The
extractor reads installed client packages and loose files. Accounts loads the
garage catalog from the bundle, and Player Sessions loads its ABI definitions and
component class metadata. Battles loads the arena catalog, terrain and scenery
collision meshes with destructible fragments.

[Battles](src/battles/README.md) owns live arenas, round phases, participants,
vehicle simulation and native battle replication. Player Sessions submits a
confirmed Training Room roster using the Battles launch contract and handles
the garage handoff and return. Player Sessions maps participant IDs to live
connections. The local Mercury adapter translates client messages into typed
battle commands. Separate bounded in-process queues link Player Sessions to
the Battles preparation and simulation tasks. The simulation owns live arena
state and advances the tick.
Player Sessions checks preparation results against the current roster,
assigns scenery controller IDs and delivers arena updates via BaseApp. Battles reports the active
arena count and worker tick timings for telemetry.

[Social](src/social/README.md) owns directed contacts and private conversation
membership, durable history and legacy per-player subscriptions. Accounts publishes the PlayerDirectory application contract. Social owns its
PostgreSQL schema and stores numeric account references. Accounts and Social
run independent SQLx migrations on one shared pool. Player Sessions
consumes Social application operations and adapts native XMPP, both legacy
messenger RPC versions, presence and committed participant delivery. See [social protocol](docs/social.md).

[Player Administration](src/player-administration/README.md) authenticates operators and adapts Accounts' player
listing, bootstrap, provisioning, roles, garage inspection and dashboard sessions to HTTP and the React Admin dashboard. It submits
currency, vehicle and cosmetic grants through Player Sessions for serialized
garage updates and supplies the embedded Store's dashboard HTTP transport.

[Binary Mapping](tools/wot-binary-map/src/binary_mapping/README.md) runs in its
own workspace package. It reads the external executable and produces offline
research artifacts. Catalog and Battle Allocation are future candidate
boundaries and do not appear as implemented contexts.

Editable models live in [docs/context-maps](docs/context-maps). Run
`cargo context-maps` to render them with the
[checksummed Rust tool](tools/wot-context-maps/README.md). Each context README documents its actual
layers and ubiquitous language; [server guide](docs/server.md) maps runtime sources
and integration contracts. The existing workflow validates and regenerates SVGs.
