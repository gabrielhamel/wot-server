# Tank module research and installation

Accounts owns researched modules, vehicle experience, shared module depot counts,
and installed module loadouts. Player Sessions translates the native garage commands and
publishes cache changes only after a revision-checked PostgreSQL transaction.
No client resource files are modified.

## Commands and rules

| Command | Wire arguments | Effect |
| --- | --- | --- |
| `CMD_UNLOCK` 201, `doCmdInt3` | vehicle type intCD, research index, 0 | Research one module using vehicle XP first, then free XP. |
| `CMD_BUY_ITEM` 302, `doCmdInt4` | shop revision, module intCD, count, 0 | Buy 1–100 researched modules with credits into the shared depot. |
| `CMD_BUY_AND_EQUIP_ITEM` 308, `doCmdIntArr` | shop revision, module intCD, vehicle inventory ID, slot 0, paid removal 0, gun intCD or 0 | Install from depot, buying missing modules with credits. |
| `CMD_EQUIP` 101, `doCmdInt3` | vehicle inventory ID, module intCD, gun intCD or 0 | Install an owned depot module; a turret may change its gun atomically. |

Research indices follow the client's resource traversal: vehicle unlocks, chassis,
engines, fuel tanks, radios, then each turret's guns followed by that turret's
unlocks. Vehicle research entries retain their indices and can now be executed; see
[tank research and purchasing](vehicles.md). Prerequisites for every occurrence of a target are combined and
expanded transitively, following `VehicleType.__convertAndValidateUnlocksDescrs`.
Research requires a researched or owned source tank, all module prerequisites
and sufficient XP. Stock modules of researched or owned tanks are always researched. Unlocks are account-wide,
so shared modules stay researched across vehicles. Duplicate research fails
without charging XP. Vehicle XP has persistent storage but no battle accrual yet;
existing accounts can research with their available free XP.

Installation validates the tank's module list, turret/gun compatibility,
research status, depot quantities, prices and balances. Removed modules return to
the depot. Combined turret/gun changes either commit together or fail together.
The installed resources do not define `maxLoad` on the stock Leichttraktor chassis;
a load limit is enforced when present, rather than inventing a required field.
Only credit prices are supported for modules. Optional devices, consumables,
tank research and purchasing are described in [their guide](vehicles.md).

Changing a turret or gun unloads shells into the shared shell depot and creates
an empty layout for the new turret/gun pair. Ammunition requests then use that
pair's actual shell types and capacity, including vehicle-local overrides.
Switching back does not restore an old saved ammunition setup automatically.
This avoids creating ammunition or silently discarding it when gun types change.

## Persistence, cache and resilience

The migration `0001_initial_schema.sql` defines `accounts.progression`:
unlocks, vehicle XP, module stock and per-inventory-ID loadouts. An absent loadout
projects the stock configuration. A full sync
combines saved unlocks with stock unlocks, projects installed modules into the
vehicle compact descriptor and exposes module stock under its item type.

A successful command updates credits/free XP, progression, module stock, affected
ammunition, shell stock and account revision in one transaction. Stale revisions
and lost vehicle ownership roll back the write. Worker permits and the session
save flag bound asynchronous work. Reconnects are refused while either ammunition
or module persistence is pending. Invalid indices, descriptors, argument lengths,
counts and costs fail only the request. Checked arithmetic protects balances,
stock counts and revisions. Cache deltas precede success responses and include
removed depot entries as `None`; `shellsLayout` uses replacement keys for the
changed turret/gun pair.

## Evidence and verification

Decompiled source references under `docs/references/wot-src/sources/res/scripts/`:

- `common/AccountCommands.py`: native command IDs.
- `client/account_helpers/Stats.py:unlock`: research arguments.
- `client/account_helpers/Shop.py:buy`, `buyAndEquipItem`: purchase arguments.
- `client/account_helpers/Inventory.py:equip`, `equipTurret`: depot installation.
- `common/items/vehicles.py:_readUnlocks`, `_readInstallableComponents`,
  `VehicleType.__convertAndValidateUnlocksDescrs`, `__collectDefaultUnlocks`:
  research ordering, combined prerequisites and default research.
- `common/items/vehicles.py:VehicleDescr.mayInstallComponent`, `mayInstallTurret`:
  vehicle and turret/gun compatibility.
- `client/account_helpers/Stats.py:synchronize`, `Inventory.py:synchronize`:
  cache update contracts.

Runtime definitions are read from the installed EU client, discoverable with
`cargo game-client path` (see [game client setup](game-client.md)), including
`scripts/item_defs/vehicles/germany/G12_Ltraktor.xml` and nation component files.
The resource-backed test confirms research index 0 is the 35-XP gun `515604`
(2 cm Breda (i) (LTr)), corresponding to the supplied screenshot. The older
kickoff client `2.4.1.5498` and decompiled source alone are not proof for another
installed build; catalog data always comes from the configured installation.

`tests/modules.rs` covers XP spending, prerequisites, depot conservation,
combined turret/gun purchases, incompatible and unaffordable changes.
`tests/support/modules_flow.rs`, invoked by the PostgreSQL garage test, covers
malformed requests followed by valid research, shop desync, buying/equipping,
depot swaps, gun-specific ammunition resupply, full sync and reconnect. The database test also checks competing
module commits, rollback after lost ownership and fresh-pool persistence. These tests use the installed script
schema and encrypted Mercury traffic; rendered game UI validation is distinct.

Implementation ownership:

| Source | Responsibility |
| --- | --- |
| `src/accounts/src/domain/modules.rs` | Progression, research, installation and stock rules. |
| `src/accounts/src/application/mod.rs` | Module command/proposal and repository contract. |
| `src/accounts/src/infrastructure/client/module_defs.rs` | Versioned resource translation, research indices and gun-specific ammunition. |
| `src/accounts/src/infrastructure/client/progression.rs` | Catalog-backed planning and client cache deltas. |
| `src/accounts/src/infrastructure/postgres.rs` | Coherent reads and atomic persistence. |
| `src/player-sessions/src/infrastructure/gateway/progression.rs` | Bounded command execution and completion responses. |
