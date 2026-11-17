# Context map

## wot-server

![wot-server context map](docs/context-maps/generated/wot-server.svg)

[Accounts](src/accounts/README.md) publishes its application API and immutable
snapshots upstream of [Client Gateway](src/client_gateway/README.md), which adapts them into login
and garage sessions. Both translate external client data through anti-corruption
layers. Accounts' PostgreSQL adapter persists user state, including revision-checked
ammunition, tank/module research, vehicle purchases and module, equipment, consumable and directive loadouts, shared depot stock and balance updates; its resource adapter builds
the garage projection. Client Gateway adapts shell, equipment, consumable, directive, module and tank progression requests and publishes committed
inventory/balance changes and purchase receipts. Client Gateway owns the Mercury UDP adapter and binds
garage RPCs to the extracted script definitions. Both contexts use the shared
[client formats](src/client_formats/README.md) infrastructure.

[Resource Extraction](crates/wot-extract/README.md) runs offline and publishes
the [bundle contract](crates/wot-resources/README.md) to Accounts and Client Gateway. The
extractor reads installed client packages and loose files. Accounts loads the
garage catalog from the bundle, and Client Gateway loads its ABI definitions and
component class metadata.

[Social](src/social/README.md) owns directed contacts and private conversation
membership, durable history and legacy per-player subscriptions. Accounts publishes a read-only SQL directory; Social owns its contact
table and consumes that directory through an anti-corruption adapter. Client Gateway
consumes Social application operations and adapts native XMPP, both legacy
messenger RPC versions, presence and committed participant delivery. See [social protocol](docs/social.md).

[Admin](src/admin/README.md) authenticates operators and adapts Accounts' player
listing and creation contract to HTTP and the React Admin dashboard.

[Binary Mapping](src/binary_mapping/README.md) independently consumes the external
executable and produces research artifacts. Runtime contexts do not consume those
artifacts yet. Catalog, Battles and Battle Allocation are future candidate
boundaries and do not appear as implemented contexts.

Editable models and the deterministic, checksummed renderer live in
[docs/context-maps](docs/context-maps). Each context README documents its actual
layers and ubiquitous language; [server guide](docs/server.md) maps runtime sources
and integration contracts. The existing workflow validates and regenerates SVGs.
