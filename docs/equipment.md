# Garage equipment

Equipment such as Improved Aiming and Gun Rammer can be bought in the shop or
bought and mounted from a tank's equipment panel. The client calls these items
*optional devices*. The server reads prices, shop availability, equipment slots,
and compatibility rules from the installed resource catalog. The T-34, for
example, can mount Class 2 Improved Aiming and Class 2 Gun Rammer.

The shop sells eligible Improved equipment for bonds. The installed catalog
marks Bounty and Experimental equipment as out of shop, so those items mount
from depot stock. An administrator can add Basic or Upgraded Bounty Equipment,
Experimental Equipment at any tier, or Components from [Resources & Garage](admin.md#grants). The garage's
Bounty and Experimental tabs show items already in the depot or mounted on
the vehicle.

Accounts validates vehicle ownership, slot capacity, descriptor identity, vehicle
tier and tags, and conflicts between devices. Applying a layout retains devices
already on the tank, uses depot stock for additions, and buys any shortfall.
Reordering installed devices is free. Equipment that requires paid demounting
must be removed through the single-slot command before it can be replaced.
Paid demounting returns the device to the depot and uses the same price when
the vehicle is sold and its equipment is retained.

| Equipment | Paid demount |
| --- | --- |
| Standard, Bounty, and Experimental level 1 | 10 gold |
| Improved | 200 bonds |
| Experimental level 2 | 50 components |
| Experimental level 3 | 100 components |

Freely removable devices return to the depot without a charge. The explicit
destroy action discards the device.

Purchases, depot counts, mounted layouts, balances, and the revision commit
together using the existing PostgreSQL progression transaction. The equipment
layout map lives in `accounts.progression` and is keyed by owned vehicle
inventory ID. Older rows default to empty slots and zero components. An
identical layout request does not debit the account or advance the revision.

## Client contract

| RPC | Account command | Arguments |
| --- | --- | --- |
| `doCmdInt4` | `CMD_BUY_ITEM` (302) | Shop revision, equipment descriptor, quantity, gold-for-credits flag (0 for supported equipment). |
| `doCmdIntArr` | `CMD_EQUIP_OPT_DEVS_SEQUENCE` (10060) | Shop revision, vehicle inventory ID, item count, ordered descriptors. Zero marks an empty slot. |
| `doCmdIntArr` | `CMD_EQUIP_OPTDEV` (102) | Shop revision, vehicle inventory ID, descriptor or zero for removal, slot index, all-setups flag, paid-removal flag, demount-kit flag (0). |

A successful mutation sends `Account.update` before the command response.
`inventory[9]` carries depot counts and uses `None` to delete a depleted
stack. `inventory[1].devicesLayout[(vehicleID, '_r')]` replaces the vehicle's
equipment setup list. The active setup is also encoded in
`inventory[1].compDescr[vehicleID]`. The descriptor mask counts slots from the
last available slot and writes each occupied slot's 16-bit device ID in that
order. The layout cache uses full compact descriptors in garage slot order.
`stats.crystal` and `stats.equipCoin` carry bond and component balances in
both full syncs and equipment updates.

## Compatibility evidence

The installed client reports `v.2.4.1.1 #993` in `version.xml`. Packed XML
under `scripts/item_defs/vehicles` supplies `common/optional_devices.xml`,
`common/supply_slot_types.xml`, nation `list.xml` tier and tags, and each tank's
`supplySlots`. The installed optional-device rules use tier, nation and
vehicle tags. They contain no component-dependent filter clauses.

The installed `res/packages/scripts.pkg` Python bytecode and the corresponding
decompilation under `docs/references/wot-src/sources/res/scripts/` give these entry points.

| Source | Evidence |
| --- | --- |
| `common/AccountCommands.py` | Command numbers. |
| `client/account_helpers/Inventory.py` and `Shop.py` | Request framing and shop revision. |
| `client/gui/shared/gui_items/processors/vehicle.py` and `module.py` | Garage Apply, individual mounting and shop purchase callers. |
| `client/gui/impl/lobby/tank_setup/interactors/opt_device.py` | The garage demounts an occupied slot before fitting a replacement. |
| `client/gui/impl/lobby/tank_setup/array_providers/opt_device.py` | The Bounty and Experimental tabs show owned items. |
| `client/gui/shared/gui_items/artefacts.py` | Selects the shop demount price by equipment category and level. |
| `client/gui/shared/gui_items/vehicle_equipment.py` | Equipment layout cache and setup format. |
| `common/items/vehicles.py` | Supply slot checks and vehicle descriptor slot-mask order. |
| `common/items/artefacts_helpers.py` | Include and exclude filters and installed-device tag conflicts. |

The encrypted Mercury garage integration exercises Standard equipment purchase,
mounting, reordering, demounting and depot reuse on the T-34. On the ISU-130 it
exercises Improved shop purchase and garage Apply, depot mounting for Bounty and
Experimental equipment, and demounting for gold, bonds and components. It checks
that failed purchases and demounts leave the balances and inventory intact, and
verifies the account deltas and full cache after reconnecting to PostgreSQL.

Garage support covers one setup. The admin Components grant adds the
`equipCoin` balance used by Experimental Equipment demounting. Reward
distribution, in-game equipment upgrades,
Experimental deconstruction, demount kits, alternate setups, and server-side
battle effects still need implementation.
