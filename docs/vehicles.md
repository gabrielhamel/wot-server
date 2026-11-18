# Tank research, vehicle purchases and sales, and garage slots

Accounts owns vehicle research, purchases and sales through the same revision-checked
progression use case as module research and installation. Player Sessions translates the
native garage requests; the client resources supply research trees, prices,
stock modules, ammunition and crew roles.

## Native requests

`CMD_UNLOCK` (201, `doCmdInt3`) takes the source vehicle's integer compact
descriptor, its research index, and zero. A research step can target a module or
another vehicle. Its module prerequisites must already be researched, and its XP
cost is paid from source vehicle XP followed by free XP. The source tank must be
researched or owned; it need not be purchased to continue researching its tree.
Repeated research fails without spending XP. Researching a tank also makes its
stock modules available through the projected default unlock set.

For the configured installed EU resources, the Leichttraktor (`3089`) has:

| Research index | Target | Cost |
| --- | --- | --- |
| 0 | 2 cm Breda (i) (LTr), module `515604` | 35 XP |
| 1 | Pz.Kpfw. II, vehicle `2065` | 275 XP, requiring the Breda research |

`CMD_BUY_VEHICLE` (301, `doCmdIntArr`) carries:

```text
[shopRevision, vehicleIntCD, flags, crewCostType, rentPeriod,
 consumablesLength, ...consumableIDAndCountPairs]
```

Permanent purchases use `rentPeriod = -1`. The consumable length counts
integers, so one small repair kit is encoded as `[2, repairKitIntCD, 1]`.
Pairs are ordered by slot, with a count of 1 per consumable or `(0, 0)` for
an empty slot. A negative item ID selects its alternate price. The server
checks slot capacity, availability and vehicle compatibility, then purchases
and mounts the chosen consumables in the same transaction as the vehicle.
Checkout uses shop prices and leaves Depot stock intact.
Flag 1 recruits crew; flag 16 buys stock ammunition. Crew cost types 0, 1 and 2
use the published shop's free, 20,000-credit and 200-gold per-member options.
Without the crew flag, the tank is delivered without crew. Without the ammunition
flag, it has a valid empty shell layout. The supplied client uses the modern
100,000-XP skill-efficiency crew format even though the legacy cost table retains
role-level labels. See [garage crew](crew.md) for persistent crew management.

Purchases validate research, shop availability, price, balances, duplicate
ownership and garage capacity. Garage `slots` is the total capacity, so buying a
tank does not decrement it. Available slots are capacity minus owned vehicles.
A server limit of 1,024 owned vehicles bounds projection work independently of
an externally edited capacity. New inventory IDs exceed the highest owned or
previously sold ID, within the positive signed 32-bit range used by garage commands.

Researchable credit tanks require an unlock. For gold vehicles offered by the
[Store](store.md), the garage sync sends the eligible descriptors in
`economics.unlocks`. The client merges those descriptors into its Tech Tree
unlock set. Pz. S35, for example, uses the native purchase command at its
extracted 1,000-gold price. These catalog unlocks leave persisted research and
Collectors’ tier eligibility unchanged. A matching-revision sync also sends
them so an existing client cache can receive the update.

A `notInShop` or unsupported-currency offer cannot be purchased through the
native command. Collectors’ vehicles use the same-nation tier requirement.
Rental, trade-in and vehicle restoration remain outside the current garage
model. Unsupported
purchase options reject the entire request before any debit or ownership change.

With free crew and stock ammunition, the installed Pz. II costs 3,000 credits
plus 1,560 credits for 520 stock shells. Shells requested at vehicle checkout are
purchased at their catalog prices; existing shell depot stock is left intact.
Normal ammunition resupply continues to use the depot first.

## Garage slot purchases

`CMD_BUY_SLOT` (205, `doCmdInt3`) takes `(shopRevision, 0, 0)` and adds one
permanent garage slot. The shop's `slotsPrices` makes slots free below a total
capacity of 12 and prices each subsequent slot at 250,000 credits. The server
uses total capacity to determine the charge and caps purchases at the existing
1,024-vehicle garage limit.

The progression transaction commits the credits debit, total slot count and
account revision together. On success the gateway sends `stats.slots` and the
updated balance before acknowledging the command. A stale shop revision returns
`RES_SHOP_DESYNC` with the current shop revision. Invalid arguments, insufficient
credits and the capacity limit reject the purchase before any state changes.
The client sends this separate slot purchase before the vehicle command
whenever the Garage slot option is selected. If the vehicle command fails,
the purchased slot remains available in the garage.

## Vehicle sales

`CMD_SELL_VEHICLE` (304, `doCmdIntArr`) carries one or more sale records after
the shop revision. The current garage sells one independently owned permanent
vehicle at a time. Its supported record is:

```text
[shopRevision, inventoryID, dismissCrew,
 mountedCount, ...mountedItemCDs, depotCount, ...depotItemCDs,
 customizationLength, demountKitLength, freeDemountLength]
```

The crew flag is 0 for barracks or 1 for dismissal. Sending crew to barracks
requires enough berths. Dismissal uses the crew recovery buffer and returns
equipped skins through the existing dismissal rules.

The server values the tank at its shop resale price and adjusts it by the
difference between each installed and stock module's resale price. Selected
mounted shells, consumables, equipment and directives are sold using server-owned
counts and the same price rules as Depot sales. The remaining fittings go to the
Depot. Keeping non-removable equipment costs the advertised gold or bonds where
that removal is supported. Selected compatible depot shells or modules sell
their existing depot quantities before retained fittings are added. Zero-credit
sales are valid.

The sale deletes the ownership row, frees its slot and clears per-vehicle
layouts and crew settings. Account-wide research and vehicle-type XP remain
available. The `last_vehicle_id` field in progression JSON records the highest
sold ID so a delayed command for a sold tank cannot target a newly bought tank.
The same transaction commits proceeds, demount charges, depot stock, crew and
the account revision. The garage update removes the sold vehicle's cache rows,
updates crew and depot entries and refreshes the Elite set before acknowledging
success.

The customization, demount-kit and subscription free-demount lists must be
empty. Group sales for multi-nation vehicles, rentals, vehicle buyback,
equipCoin operations and the official sale-quota reset policy remain outside
the current garage model. Vehicle tags marked `cannot_be_sold` are enforced.

## Catalog and persistence

Startup indexes the vehicle IDs and catalog keys in enabled nation lists, but
loads full vehicle templates only when needed. The cache is shared, uses checked
lock acquisition, and is limited to known resource keys. Owned starter vehicles
are still validated during startup. Requests cannot supply a filesystem path.
Research/purchase work and template loading run on bounded blocking workers,
not the UDP event loop. A newly bought vehicle can be projected immediately;
no restart or catalog reload is needed.

The migration `0001_initial_schema.sql` defines the
**`accounts.progression`** JSONB column. Its fields are `unlocks`, `vehicle_xp`, `depot` and `installed`;
`unlocks` includes researched tank IDs as well as module IDs. Ownership is
stored in **`owned_vehicles`**, with the stable inventory ID, catalog key, saved
ammunition and a `has_crew` flag, which defaults to true.
A uniqueness constraint prevents duplicate ownership of a catalog key per
account.

The transaction guards the account's previous revision, debits credits/gold/free
XP, persists progression including mounted consumables and shell stock, and
inserts the purchased vehicle.
Existing ownership rows must still exist; a lost row or failed insert rolls back
the whole transaction. Concurrent proposals cannot debit twice against the same
revision. The session sends inventory additions and researched/default unlocks
only after commit, before acknowledging success. New crew IDs are assigned by the
account's persisted roster. Reconnect reads ownership and that roster to rebuild
the garage projection.

To inspect the result:

```sql
SELECT id, credits, gold, free_xp, jsonb_pretty(progression)
FROM accounts
WHERE id = YOUR_ACCOUNT_ID;

SELECT inventory_id, catalog_key, has_crew, ammunition
FROM owned_vehicles
WHERE account_id = YOUR_ACCOUNT_ID
ORDER BY inventory_id;
```

## Evidence and verification

Sources under `docs/references/wot-src/sources/res/scripts/`:

- `common/AccountCommands.py`: commands 201, 205, 301 and 304, and `BUY_VEHICLE_FLAG` values.
- `client/account_helpers/Stats.py:unlock`: source vehicle and research index.
- `client/account_helpers/Stats.py:__slot_onShopSynced`: slot purchase arguments.
- `common/items/item_price.py:getNextSlotPrice`: free capacity and price lookup.
- `client/gui/shared/utils/requesters/StatsRequester.py:vehicleSlots`: cache key
  for total garage capacity.
- `client/account_helpers/Shop.py:buyVehicle`: flags and purchase argument layout.
- `client/gui/shared/gui_items/processors/vehicle.py:VehicleBuyer`, `getSumPrice`:
  permanent purchase, crew, shell and consumable options, and component costs.
- `client/gui/impl/lobby/hangar/buy_vehicle_view.py`: consumable selection and
  the separate slot purchase preceding the tank command.
- `client/gui/shared/gui_items/artefacts.py:Equipment.defaultLayoutValue`:
  consumable ID and count pairs.
- `client/account_helpers/Inventory.py:__sellVehicleOnShopSynced` and
  `client/gui/shared/gui_items/processors/vehicle.py:VehicleSeller`: sale records,
  mounted and depot item selection, and demount lists.
- `client/gui/shared/gui_items/Vehicle.py:__calcSellPrice` and
  `client/account_helpers/Shop.py:getSellPrice`: vehicle module adjustments,
  currency conversion and resale rounding.
- `common/items/vehicles.py:_readUnlocks`,
  `VehicleType.__convertAndValidateUnlocksDescrs`: research order and prerequisites.
- `common/items/tankmen.py:generateCompactDescr`: current crew efficiency format.
- `client_common/shared_utils/account_helpers/diff_utils.py:synchronizeDicts`:
  nested cache update contract used for new vehicles and crew.

Definitions and prices come from the configured installation, tested here with EU
`2.4.1.5514` as recorded in the server guide. Discover the current installation
with `cargo game-client path` (see [game client setup](game-client.md)).
Decompiled source is supporting
reference, not proof of compatibility with a different build.

`tests/vehicles.rs` covers optional crew/ammunition, wallet currencies, duplicate
ownership, missing research, capacity, insufficient funds and arithmetic limits.
`tests/support/vehicles_flow.rs` exercises malformed requests, shop mismatch,
research prerequisites, the actual Pz. II purchase, immediate inventory additions,
duplicate rejection and full sync over encrypted Mercury. The garage integration
test reconnects the account, verifies the identical full cache, reads ownership
through a fresh database pool and projects it using a newly loaded catalog.
The slot purchase flow verifies the published price, insufficient credits, malformed
requests, stale shop revision, the live capacity and credit update, and persistence
through reconnect. It also checks the free threshold and capacity limit in planning.
The M14/41 checkout follows the slot purchase, rejects malformed or oversized
consumable layouts and verifies the credit debit, crew, ammunition and mounted
repair kit.
Rendered game UI confirmation remains a separate manual check.
`tests/support/vehicle_sales_flow.rs` exercises the encrypted sale command,
including 4TP and Renault FT, both crew outcomes, mixed item disposition,
garage cache deletions, and duplicate rejection. It checks the persisted sale,
reconnect, and inventory ID allocation after a sale.

| Source | Responsibility |
| --- | --- |
| `src/accounts/domain/modules.rs` | Shared research/progression state and module installation rules. |
| `src/accounts/domain/vehicles.rs` | Vehicle purchase rules, costs, capacity and options. |
| `src/accounts/application/mod.rs` | Progression command/proposal and persistence port. |
| `src/accounts/infrastructure/client/vehicle_defs.rs` | Resource-backed offers and purchase definitions. |
| `src/accounts/infrastructure/client/mod.rs` | Lazy template cache and account projection. |
| `src/accounts/infrastructure/client/progression.rs` | Research/purchase coordination and cache deltas. |
| `src/accounts/infrastructure/client/vehicle_sales.rs` | Resource-backed vehicle and fitting valuation, crew disposition and sale planning. |
| `src/accounts/infrastructure/postgres.rs` | Atomic progression and ownership persistence. |
| `src/player_sessions/infrastructure/gateway/progression.rs` | Native request decoding and bounded persistence jobs. |
