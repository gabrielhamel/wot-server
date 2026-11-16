# Garage directives

The garage supports buying, mounting, swapping and removing one directive on
a vehicle with a directive slot. Installation consumes a copy from the Depot
first. If none is available, it buys one at the catalog price. Removing or
swapping a directive returns the unused copy to the Depot. Applying the same
directive again does not charge the account or advance its revision.

Crew directives in the supplied catalog cost credits; equipment directives cost
bonds. Accounts in this development server start with 10,000 bonds, like the
existing development grants for credits and gold. Migration 0006 initializes
that balance for existing accounts and persists it in PostgreSQL. The client
calls bonds `crystal` in the shop and account cache.

## Client contract

The decompiled reference below is relative to
`docs/references/wot-src/sources/res/scripts/`. The catalog and slot metadata were checked
against the supplied `2.4.1.5498` client's packed XML.

- `client/gui/shared/gui_items/processors/vehicle.py` builds the directive
  Apply request. `client/account_helpers/Inventory.py` packs it into
  `CMD_SET_AND_FILL_LAYOUTS` (108):
  `[shopRev, vehicleInvID, 0, 1, length, ...layout]`. The first zero means no
  shell layout. The equipment type 1 is `battleBoosters`, from
  `common/items/__init__.py`. The length counts integers. The layout contains
  one pair per regular consumable slot, then the directive pair: `[id, 1]`
  to mount or `[0, 0]` to remove. The prefix must match the vehicle’s currently
  installed [consumables](consumables.md). The slot counts come from
  each vehicle's `supplySlots` and the packed `supply_slot_types.xml`.
- The compact descriptor remains inventory item type 11 (`equipment`).
  `equipments.xml` distinguishes directives with `type=battleBoosters`.
  Entries marked `notInShop` can be mounted from stock but cannot be bought.
  The `notForSale` tag prevents Depot sales; it does not remove the item's
  purchase price or override its shop availability.
- Buy More uses `CMD_BUY_ITEM` (302), as shown by
  `client/gui/impl/lobby/instructions/booster_buy_dialog.py`. Purchases add to
  the same Depot stock used by Apply.
- The client reads mounted IDs from `boosters` and the active setup from
  `boostersLayout` (a list containing the setup list), as shown in
  `client/gui/shared/gui_items/vehicle_equipment.py`. The server sends both in
  the inventory diff, using a replacement key for the nested layout.
- Auto-resupply uses `CMD_VEH_SETTINGS` (107) with flag 128 and a value of 0
  or 1. The preference is per vehicle and survives removal and reconnect.
  It is independent of the regular consumable auto-resupply and crew return
  flags. See `common/AccountCommands.py` and
  `client/gui/shared/gui_items/processors/vehicle.py`.

Directive mutations share the garage progression transaction and revision
guard. The server validates ownership, slot availability, request shape,
catalog IDs, shop revision, stock and funds before sending any updated state.
The client can show that an installed equipment directive has no effect with the
currently mounted devices; its `BattleBooster.mayInstall` does not require an
affected device. This milestone covers the garage's active setup and stored
preference. Battle effects, per-battle consumption and automatic restocking
need the battle lifecycle.

See [testing](testing.md#postgresql-garage-integration) for the encrypted
garage peer test. That test checks the server protocol and persistence; the
in-game screen still needs visual confirmation.
