# Garage ammunition

Accounts owns loaded shells, their order, shared depot stock, shell purchases,
balances and revisions. Access translates the garage RPC and publishes committed
updates. Stock defaults and initial crew descriptors come from the catalog; module loadouts
and vehicle purchase options are persisted (see [tank progression](vehicles.md)); this milestone
does not implement battle consumption or postbattle automatic resupply.

## Behavior and persistence

The shell panel can increase or reduce individual counts, empty a vehicle, change
shell order and replace one shell type with another. The server checks ownership,
gun compatibility, distinct shell IDs, nonnegative counts and total gun capacity.
Unloading returns shells to the account depot without a sale or refund. Loading
uses depot stock first and buys only the missing shells, at the catalog price.
Unavailable shells can be moved from existing stock but cannot be bought.
Insufficient funds reject the whole proposal.

A nullable owned_vehicles.ammunition stores the loaded (shell, count) pairs.
An untouched vehicle retains its existing deterministic default ammunition.
The first changed loadout becomes durable. accounts.depot_shells stores shared
shell counts; both fields use domain JSON, separate from the client pickle format.
A transaction updates credits, gold, depot stock, vehicle ammunition and revision.
The previous revision is checked before writing, so competing proposals cannot
spend the same balance or stock twice. Missing ownership rolls back the account
update. Repeating an identical target neither charges nor advances the revision.

## Client contract

AccountCommands.CMD_SET_AND_FILL_LAYOUTS (108) arrives through doCmdIntArr:

    [shopRevision, vehicleInventoryID, shellArrayLength,
     shellID, count, shellID, count, ..., equipmentType, equipmentArrayLength]

The implemented shell-only request has equipment type 0, equipment length 0,
and between one and sixteen shell/count pairs. A complete set of zero counts
empties the tank. Omitted compatible types are appended with zero counts. The client’s combined Auto Fill request can also apply
[regular consumables](consumables.md) in the same transaction. Other combined
layouts are rejected in full.

Positive IDs use the item's normal price. Negative IDs select its alternate price:
for gold-priced shells the shop advertises credits at 400 credits per gold.
The sign is normalized before ownership/compatibility checks, so id and -id
cannot bypass duplicate checks. Returned loaded/layout IDs are positive; alternate
purchase preference is not persisted. The old inventory-only CMD_EQUIP_SHELLS
(103) is not implemented; no supplied modern GUI caller was found.

A mismatching shop revision gets RES_SHOP_DESYNC (-4) with shopRev in an
extended response. Invalid framing gets RES_WRONG_ARGS (-2), busy sessions get
RES_COOLDOWN (-5), and rejected business operations get RES_FAILURE (-1).
Success sends Account.update before onCmdResponseExt:

- prevRev and rev identify the committed account revision.
- inventory[1].shells[vehicleID] is the flat positive-ID/count array.
- inventory[1].shellsLayout[(vehicleID, '_r')] replaces that vehicle's
  turret/gun layout, preventing recursive merge with an old layout.
- inventory[10] updates depot counts and uses None to delete emptied stacks.
- stats.credits and stats.gold contain the resulting balances.
- The response's shells receipt lists (shellID, (totalCredits, totalGold), count)
  for actual purchases only.

Each session shares its one-write gate with preferences. Ammunition jobs also hold
a global worker permit through completion delivery, even after disconnect.
Reconnects for the same account are refused while its ammunition job is pending.
Compression and projection finish before the database commit and run outside
the UDP loop. A failed proposal sends no inventory or balance update.

## Compatibility evidence

Research on 2026-11-12 used the installed client's res/packages/scripts.pkg
through the Development game-directory symlink. The previously recorded launcher
build is 2.4.1.5514; that historical observation does not independently pin
today's executable. These contracts are grounded in installed resources and
Python bytecode, rather than inferred from older BigWorld engine behavior.

Supporting decompilation under docs/references/wot-src/sources/res/scripts/:

| Source | Evidence |
| --- | --- |
| common/AccountCommands.py | Command and result constants. |
| client/account_helpers/Inventory.py: setAndFillLayouts, __setAndFillLayoutsOnShopSynced | Request framing and callback. |
| client/gui/shared/gui_items/processors/vehicle.py: BuyAndInstallShellsProcessor | Shell-only apply request and purchase receipt handling. |
| client/gui/shared/gui_items/gui_item_economics.py: getVehicleShellsLayoutPrice | Existing loaded shells and depot stock reduce purchase cost. |
| client/gui/shared/gui_items/vehicle_equipment.py: _ShellsCollector | Installed shells and turret/gun layout cache shapes. |
| client_common/shared_utils/account_helpers/diff_utils.py: synchronizeDicts | Replacement keys and None deletions. |
| client/gui/shared/money.py: Currency.ALL, makeFromMoneyTuple | Receipt currency order begins credits, gold. |

The decompiled Shell.defaultLayoutValue has a broken conditional. Disassembly of
installed scripts/client/gui/shared/gui_items/vehicle_modules.pyc confirms that
isBoughtForAltPrice negates intCD. Installed vehicle_equipment.pyc also
confirms __parseInstalled searches the raw layout IDs for each positive gun
shell descriptor; the projection therefore returns positive layout IDs. This
avoids treating the damaged decompiler expression as a protocol requirement.
Local bytecode investigation used xdis in a temporary directory; it adds no
runtime or repository dependency.

## Verification and limits

tests/ammunition.rs covers shell conservation, depot-first purchases, alternate
pricing, insufficient funds, unavailable types, ordering, duplicates, ownership,
capacity, and checked numeric overflow. Gateway unit tests cover hostile lengths,
IDs, counts and unsupported combined requests. The garage integration also
verifies that a combined request with invalid shells cannot change consumables.

tests/support/ammunition_flow.rs, invoked by the PostgreSQL encrypted garage test,
sends rejected requests followed by unload/reload/replacement commands. It checks
actual update packets, depot deletions, balances, purchase receipts, idempotency
and full resynchronization. Reconnecting yields the identical saved cache.
tests/garage.rs also reopens the database pool, races two commits from one
revision and checks rollback when ownership disappears.

These are PostgreSQL and real Mercury transport tests with synthetic peers and
installed client resources. The ammunition panel has not yet been visually tested
in the running game. Multiple saved ammunition setups, automatic
resupply settings, battles and ammunition consumption remain outside this
milestone. See [Garage Depot](depot.md) for shell sales and
[garage equipment](equipment.md) for optional devices.
