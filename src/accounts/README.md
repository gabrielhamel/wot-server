# Accounts

The `wot-accounts` library publishes its operations through `wot_accounts::contract`.

Accounts owns durable player identity, password hashes, player roles, dashboard sessions, balances, vehicle ownership,
integer preferences, tank/module research, vehicle purchases and sales, garage capacity, module installation, mounted equipment, consumables, directives, cosmetic ownership and outfits, loaded ammunition, shared depot stock and sales, crew and barracks, and account revisions. It publishes account views and garage projections to Player Sessions.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Account snapshots, balances, owned vehicles and ammunition, progression, equipment, consumables, directives, depot sales, crew rosters, training and vehicle purchase rules; registration validation. Contains no SQL or protocol types. |
| `application/` | Repository ports, credential verification, paginated player listing, managed provisioning and self-service registration, snapshot queries, preferences and ammunition/progression proposals/commit use cases. |
| `infrastructure/postgres.rs` | PostgreSQL adapter, coherent snapshot reads, atomic preference updates and revision-checked inventory transactions. |
| `infrastructure/directory.rs` | PostgreSQL adapter for the published player directory. |
| `infrastructure/players.rs` | PostgreSQL player directory pages and atomic creation with initial balances and starter garage provisioning. |
| `infrastructure/player_access.rs` | Role-aware account and session access, serialized role changes and session revocation. |
| `infrastructure/client/` | Anti-corruption layer adapting the versioned [resource bundle](../../docs/resources.md), garage catalog and account/shop cache formats. These wire models are not account domain models. |

Runtime consumers import the context's [published contract](src/contract.rs).
Game authentication returns an opaque `AccountView` with identity and revision
accessors. `GarageQueries` publishes session projections, Store offers and
dashboard inspections. `GarageCommands` validates garage changes against the
current revision and commits them. Player Sessions delivers the committed
projection and receipt. Server composition creates these services through
the [PostgreSQL entry point](src/postgres.rs).

Accounts owns the `accounts` PostgreSQL schema and its
[embedded migrations](src/infrastructure/migrations/). It owns identity and Garage
in this deployment and keeps initial garage provisioning atomic with account
creation. Catalog adapters translate the resource bundle and versioned client
formats.

| Term | Meaning |
| --- | --- |
| Account | A durable identity selected by a unique, case-sensitive sign-in name (an email for self-registered players). |
| Player role | The account's Player or Administrator access level. |
| Dashboard session | A 60-day panel credential bound to the account's role at issuance and backed by a revocable PostgreSQL record. |
| Starter garage | Tier I regular tech tree tanks with stock modules, ammunition, and trained crew. |
| Owned vehicle | A stable inventory ID and catalog key attached to one account. |
| Catalog key | A resource identifier such as `usa:A01_T1_Cunningham`. |
| Snapshot | A coherent read of an account and its vehicle ownership. |
| Revision | Monotonically increasing version changed with persisted preferences, research or inventory. |
| Cosmetic ownership | Total copies of an exterior item, including applied copies. Stat trackers are bound to a vehicle type. |
| Outfit | Vehicle appearance for a season, including styles, camouflage, projected decals, inscriptions and effects. |
| Special recruit | Crew member granted to the barracks from a national premium passport group. |
| Locked style | A catalog-defined outfit supplied with its vehicle in garage and battle projections. |
| Shell layout | Ordered compatible shell/count pairs loaded on one vehicle. |
| Depot stock | Shared unmounted shells, modules, equipment, directives and consumables. Sales debit these counts and credit the account in the same revision. |
| Equipment components | Balance used for paid demounting of level 2 and 3 Experimental equipment. The client calls it `equipCoin`. |
| Ammunition proposal | Validated next snapshot and purchase receipt, committed against its source revision. |
| Garage projection | Compressed Python cache derived from a snapshot and client definitions. |
| Battle vehicle | An owned loadout projected for battle with the encoded descriptor, locked outfit, equipped turret and hull health, crew size, loaded ammunition, and performance from the equipped chassis, turret and gun. |
| Training vehicle descriptor | Compact descriptor for the Training Room roster. A player vehicle uses its owned loadout. The observer uses the stock descriptor from the resource bundle. |
| Research | An account-wide tank or module unlock bought with vehicle XP and free XP. |
| Vehicle purchase | An atomic debit and permanent ownership grant, with optional crew, ammunition and consumables. |
| Player grant | An operator-authorized addition of currency, cosmetics, crew books, Bounty and Experimental Equipment, or a permanent tank with stock modules, ammunition and basic crew, committed as an account revision. |
| Garage slot | One unit of total vehicle capacity. A slot purchase adds one unit and charges the shop price in the same revision. |
| Vehicle sale | An atomic ownership removal that settles the vehicle and fitting value, returns retained fittings to the Depot, and transfers or dismisses its crew. |
| Module loadout | The six module descriptors installed on an owned tank. |
| Store offer | A selected item, vehicle or Premium Account package with its server-owned price in credits, gold or bonds. |
| Premium Account | WoT entitlement active until an account’s persisted UTC expiration. A purchase extends any remaining time. |
| Equipment layout | The ordered optional devices mounted on an owned tank. |
| Consumable layout | Ordered regular consumables mounted on a vehicle, with an independent auto-resupply preference. |
| Directive slot | A mounted crew or equipment directive and its vehicle-specific auto-resupply preference. |
| Research step | A resource-ordered XP cost, target and prerequisite set. |
| Elite vehicle | A vehicle whose research requirements are met. Some vehicles earn this status through a vehicle skill tree. |
| Crew member | A persistent inventory ID with a passport, qualification, specialization and training. |
| Seat | A vehicle inventory ID and crew slot. Each member occupies at most one seat. |
| Berth | Capacity for one active crew member without a seat. |
| Specialization | The vehicle type a member is trained for. |
| Perk | A selected crew competency with progress based on crew XP. |

Stock modules are derived from the extracted vehicle definitions. Legacy initial crew are projected
from vehicle ownership until the first crew write or vehicle purchase. The
[crew guide](../../docs/crew.md) describes roster persistence and native operations.
Module research, depot counts and changed loadouts are durable; see
[module progression](../../docs/modules.md) and [tank purchases and sales](../../docs/vehicles.md).
Untouched ammunition uses the existing default; changed ammunition and depot stock
are durable. See [garage ammunition](../../docs/ammunition.md) for the use case,
wire evidence and limits. [Garage equipment](../../docs/equipment.md) records
equipment purchasing, mounting and removal. [Garage consumables](../../docs/consumables.md) covers regular consumable stock, slot changes and combined Auto Fill. [Garage directives](../../docs/directives.md)
describes purchases, mounting and auto-resupply settings. The [Garage camouflage](../../docs/camouflage.md) describes stock, seasonal outfits and
pricing. The [Garage Depot](../../docs/depot.md)
records sale pricing, stock validation and cache updates. User balances and
ownership come from PostgreSQL; the compatibility adapter supplies empty/default
values for unsupported account features. See [server behavior](../../docs/server.md).

The published `PlayerDirectory` resolves numeric account IDs and display names
for Social. Server composition shares one PostgreSQL pool. Each repository uses
its own schema and Social holds account IDs as scalar values.

[Player Administration](../player-administration/README.md) consumes `PlayerAccess` through the public
context entry point. It receives account IDs, logins, display names and roles. Account
creation hashes the password with Argon2id and commits the identity and starter
garage in one PostgreSQL transaction. Bootstrap authorizes first-admin creation
only in an empty database. The role transaction rechecks the actor, prevents
self-role changes, and revokes sessions on demotion.
Player Administration reads garage projections and grant catalogs, including styles, crew skins and
special recruits. Player Sessions uses the same revision-checked progression transaction for
player grants, garage purchases and
[Store checkout](../../docs/store.md).

Accounts emits application, domain decision, catalog and repository spans.
Repository queries and transaction boundaries use the shared database tracing
adapter with complete parameterized SQL and error status. Catalog work retains
the caller's parent across blocking tasks. See the
[telemetry contract](../../docs/telemetry.md).
