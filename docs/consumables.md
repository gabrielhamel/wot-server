# Garage consumables

The garage supports buying, mounting, reordering and removing regular
consumables, including repair kits, first aid kits, fire extinguishers, food
and fuel. Apply keeps copies already on the tank, uses Depot stock for new
items, and buys only the shortfall. Removing an unused consumable returns it to
the Depot. Repeating the same layout does not charge or advance the account
revision. Auto-resupply is a separate preference for each vehicle and defaults
to on, as it did before the setting could be changed.

Accounts validates vehicle ownership, slot count, catalog identity, vehicle
filters, duplicate items, conflicts, shop availability and funds. The installed
client catalog, rather than the submitted request, determines price and
compatibility. Regular slots are counted from each vehicle's `supplySlots`;
directives and other equipment types cannot occupy those slots. A regular item
marked `notInShop` can only be mounted from stock. Factory built-in and mode
specific equipment provisioning are outside this milestone.

The ordered layout and auto-resupply preference are stored in
`accounts.progression` by owned vehicle ID. Old rows default to empty slots and
the original auto-resupply preference. Layout, stock, balances, and any shell
changes commit under one PostgreSQL revision check. A rejected part of a
combined layout leaves the whole account unchanged.

## Client contract

The following layout arguments are passed to `CMD_SET_AND_FILL_LAYOUTS` (108)
through `doCmdIntArr`. Lengths count integers. Each consumable or directive pair
is `[id, 1]` or `[0, 0]` for an empty slot.

| Caller | Arguments |
| --- | --- |
| Consumables Apply | `[shopRev, vehicleInvID, 0, 0, length, ...regularSlots, ...installedDirectiveSlots]` |
| Combined Auto Fill | `[shopRev, vehicleInvID, shellLength, ...shellPairs, 0, length, ...regularSlots]` |
| Directive Apply | `[shopRev, vehicleInvID, 0, 1, length, ...installedRegularSlots, ...directiveSlots]` |

Only the layout named by the request can change. In an Apply request, the
other slots must agree with the currently installed items. Negative regular
item IDs select the alternate price for gold-priced items using the shop
exchange rate; validation and cache projection use the positive descriptor.

Buy More uses `CMD_BUY_ITEM` (302) with the shop revision, descriptor, quantity,
and the existing zero exchange flag. `CMD_VEH_SETTINGS` (107) with flag 8
and value 0 or 1 changes the auto-resupply setting independently of directive
auto-resupply (128).

The server sends `Account.update` before acknowledging a changed layout.
`inventory[1].eqs` carries installed descriptors and
`inventory[1].eqsLayout[(vehicleID, '_r')]` replaces the active setup list.
The lists preserve interior gaps as zeros. `inventory[11]` carries Depot
changes, using `None` for a depleted stack. The extended response's `eqs` list
contains `(itemID, (credits, gold), 1)` for each actual purchase. Combined Auto
Fill may also return the usual [shell receipt](ammunition.md).

## Evidence and verification

The packed `common/equipments.xml` and `common/supply_slot_types.xml` in the
supplied client's `res/packages/scripts.pkg` provide item prices, filters and
slot metadata. Its `version.xml` reports `v.2.4.1.1 #993`. The catalog includes
both packed booleans and the string `True` for `notInShop`.

The matching decompiled references under `docs/references/wot-src/sources/res/scripts/` are
`client/account_helpers/Inventory.py` for framing,
`client/gui/shared/gui_items/processors/vehicle.py` for all three callers,
`client/gui/shared/gui_items/vehicle_equipment.py` for the cache,
`common/AccountCommands.py` for command and setting numbers, and
`common/items/artefacts.py` and `artefacts_helpers.py` for item compatibility.

The [PostgreSQL garage test](testing.md#postgresql-garage-integration) verifies
encrypted requests and responses with the installed catalog. It applies the
Small First Aid Kit, Automatic Fire Extinguisher and Large Repair Kit to an
ISU-130 for 43,000 credits and verifies the cache after reconnect. The running
game UI has not been checked. In-battle activation, effects, cooldowns,
consumption, and automatic restocking after battle require a battle lifecycle.
