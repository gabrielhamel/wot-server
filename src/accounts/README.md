# Accounts

Accounts owns durable player identity, password hashes, balances, vehicle ownership,
integer preferences, tank/module research, vehicle purchases and sales, garage capacity, module installation, mounted equipment, consumables, directives, cosmetic ownership and outfits, loaded ammunition, shared depot stock and sales, crew and barracks, and account revisions. It publishes immutable snapshots to
Player Sessions. It does not own transport sessions, entities, battles or matchmaking.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Account snapshots, balances, owned vehicles and ammunition, progression, equipment, consumables, directives, depot sales, crew rosters, training and vehicle purchase rules; no SQL or protocol types. |
| `application/` | Repository ports, credential verification, paginated player listing and provisioning, snapshot queries, preferences and ammunition/progression proposals/commit use cases. |
| `infrastructure/postgres.rs` | SQLx pool, embedded migrations, coherent snapshot reads, atomic preference updates and revision-checked inventory transactions. |
| `infrastructure/players.rs` | PostgreSQL player directory pages and atomic creation with tier I garage provisioning. |
| `infrastructure/client/` | Anti-corruption layer adapting the versioned [resource bundle](../../docs/resources.md), garage catalog and account/shop cache formats. These wire models are not account domain models. |

Player Sessions consumes `Accounts<R>`, `AccountSnapshot`, and the `GarageCatalog` encoder
through this context's entry point (`mod.rs`). Catalog loading and garage cache
encoding remain infrastructure of Accounts for this milestone and use the shared
[client formats](../platform/infrastructure/client_formats/README.md) codecs. The
catalog adapter discovers tier I tech tree vehicles and prepares starter
loadouts for [player creation](../../docs/admin.md). A separate Catalog
context is a possible future boundary, not an implemented one.

| Term | Meaning |
| --- | --- |
| Account | A durable identity selected by a unique, case-sensitive username. |
| Starter garage | Tier I regular tech tree tanks with stock modules, ammunition, and trained crew. |
| Owned vehicle | A stable inventory ID and catalog key attached to one account. |
| Catalog key | A resource identifier such as `usa:A01_T1_Cunningham`. |
| Snapshot | A coherent read of an account and its vehicle ownership. |
| Revision | Monotonically increasing version changed with persisted preferences, research or inventory. |
| Cosmetic ownership | Total copies of an exterior item, including applied copies. |
| Outfit | Vehicle appearance for a season, including camouflage, inscriptions and an effect. |
| Shell layout | Ordered compatible shell/count pairs loaded on one vehicle. |
| Depot stock | Shared unmounted shells, modules, equipment, directives and consumables. Sales debit these counts and credit the account in the same revision. |
| Equipment components | Balance used for paid demounting of level 2 and 3 Experimental equipment. The client calls it `equipCoin`. |
| Ammunition proposal | Validated next snapshot and purchase receipt, committed against its source revision. |
| Garage projection | Compressed Python cache derived from a snapshot and client definitions. |
| Research | An account-wide tank or module unlock bought with vehicle XP and free XP. |
| Vehicle purchase | An atomic debit and permanent ownership grant, with optional crew, ammunition and consumables. |
| Player grant | An operator-authorized resource addition or permanent tank with stock modules, ammunition and basic crew, committed as an account revision. |
| Garage slot | One unit of total vehicle capacity. A slot purchase adds one unit and charges the shop price in the same revision. |
| Vehicle sale | An atomic ownership removal that settles the vehicle and fitting value, returns retained fittings to the Depot, and transfers or dismisses its crew. |
| Module loadout | The six module descriptors installed on an owned tank. |
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

Accounts publishes `account_directory(id, display_name)` as a read-only SQL
identity projection for Social, and exposes the shared SQLx pool for composition.
Social owns its contact table; foreign keys use Accounts IDs. Credentials, balances,
inventory and revisions are not part of this directory contract.

[Player Administration](../player_administration/README.md) consumes `PlayerAccounts<R>` through the public
context entry point. It receives account IDs, logins and display names. Account
creation hashes the password with Argon2id and commits the identity and starter
garage in one PostgreSQL transaction.
Player Administration also reads snapshots and vehicle catalog entries. Player Sessions uses the same
revision-checked progression transaction for player grants and garage purchases.

Accounts emits application, domain decision, catalog and repository spans.
Repository queries and transaction boundaries use the shared database tracing
adapter with complete parameterized SQL and error status. Catalog work retains
the caller's parent across blocking tasks. See the
[telemetry contract](../../docs/telemetry.md).
