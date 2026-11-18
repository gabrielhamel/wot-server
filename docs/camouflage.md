# Garage camouflage, decals, inscriptions and effects

Exterior can buy permanent camouflage into shared stock or buy missing copies
while applying an outfit. A placement consumes one copy per surface per season.
The three garage seasons are winter, summer and desert. The supported surfaces
are the hull, turret and gun, as exposed by the vehicle's current configuration.
Scale and palette selections persist with each placement.

Accounts stores total owned quantities and seasonal outfits by vehicle inventory
ID in the existing progression JSON. It derives unmounted stock from ownership
minus placements across all vehicles. Removing camouflage, selling a vehicle while keeping
its cosmetics, or swapping to a module without the corresponding
surface frees the copies for reuse. The wallet, ownership, and layout commit
together against the current account revision. Reapplying the saved layout
succeeds without charging or advancing the revision.

Regenerate bundles created before camouflage support using the updated
[resource extractor](resources.md). The server uses the merged Packed XML
`scripts/item_defs/customization/camouflages/list.xml` and
`scripts/item_defs/customization/priceGroups/list.xml` for definitions and prices.
It uses the same prices for the shop stream and for charges. The server checks
vehicle filters, vehicle and item surface availability, season, palette and
scale before saving. Offer restrictions include unavailable items, token-gated
items and style-only components. This implementation covers unbound, permanent
camouflage and its garage layouts. Styles, attachments, cosmetic resale,
rental and reward grants are outside this workflow.

## Inscriptions

The Inscriptions tab supports preset inscriptions and personal numbers such
as Stenciled rounded. Each item occupies one inscription slot on one season.
The server checks the vehicle's installed modules for available slots and
uses the extracted decal and number definitions for eligibility and price.
Personal numbers use the font's exact digit count and the client's prohibited
number list. Leading zeroes are preserved. Editing a mounted number reuses
the same copy, and removing it returns the copy to stock.

An outfit can contain camouflage and inscriptions together. Buying missing
copies, applying the layout and updating the wallet all share one account
revision. The client receives the resulting stock and fitted counts for each
item subtype.

## Effects

Effects such as Worn paint 01 occupy one slot per seasonal outfit. The server
checks the vehicle's effect anchor, including the client's fallback to the style
anchor, and charges the price from `customization/modifications/list.xml` and
its price group. The installed EU catalog prices Worn paint 01 at 300 gold per
copy. The same effect can be applied to multiple seasons using one copy per
season. Removal frees a copy and reapplication uses stock before buying.

## Decals

The Decals tab uses projected decals. These have a vehicle anchor, a scale
selection and mirroring options. The server validates the anchor against the
installed modules and checks the decal's shape and mirroring rules. A seasonal
outfit can hold up to three projected decals, each using one copy. Moving,
resizing or mirroring a fitted decal reuses its copy. Removing it returns the
copy to stock.

The expanded bundle includes `projection_decals/list.xml` and the progression
definitions. The installed EU catalog prices the Tiger decal at 160 gold.
Progression-locked and style-only decals remain unavailable through this
purchase flow.

## Exterior button

The native hangar menu checks for at least one compatible 2D style before it
enables Exterior. Its lookup requires the style descriptor to be present in
`items.itemPrices`. The server adds empty price rows for base 2D descriptors
that the client already hides from the carousel. It also excludes styles from
purchase using `notInShopItems`. Style purchases and application remain
unsupported.

The carousel enumerates client definitions directly, including extensions.
Unpriced items can appear selectable at zero gold when their IDs are missing
from the shop exclusions. The extractor includes the base and extension lists
for styles and camouflage, so the shop can exclude event cosmetics without
touching the player's stock. The Last Stand 2025 and Corrupted cards on the
Executor came from `last_stand/scripts/item_defs/customization/styles/list.xml`.
Use the striped Camouflage tab in Exterior to edit individual surfaces and seasons.

Reconnect after updating the bundle and restarting the server so the client
fetches the updated shop cache.

## Wire contract

| Command | Arguments |
| --- | --- |
| `118` (`CMD_BUY_C11N_ITEMS`) | `doCmdIntArr` with shop revision, vehicle inventory ID, then item descriptor/count pairs. Inventory ID zero buys unbound stock without vehicle context. |
| `119` (`CMD_VEH_APPLY_OUTFIT`) | `doCmdIntArrStrArr` with shop revision, vehicle inventory ID and a season per binary outfit argument. Each entry replaces the named season. An empty descriptor clears that season. |

Outfit descriptors use unsigned base-128 varints for the component type (4),
field mask, camouflage array length and component fields. The camouflage field
uses mask bit 2 and encodes item ID, pattern size, applied-area mask and palette
index in that order when present. Camouflage regions use bits 4, 8 and 12.
The native customization dialog can send an empty common outfit (season 15)
alongside the regular seasons. Effects use the modification ID array (bit 0). Preset inscriptions use the
decal field (bit 3), projected decals use bit 5 and personal numbers use bit 7.
Projected decal entries carry an item ID, options, slot ID and scale in that order.
A personal number carries the item ID, length-prefixed digit string and region mask.
The decoder also accepts the vehicle's default
national emblem fields as ancillary data. All other outfit component edits and
invalid or overlapping placement areas in the request are rejected.

The inventory cache projects customization type 12 with stock in table 1,
outfits in table 2 and fitted counts in table 4. Stock and fitted counts are
keyed by customization subtype and item ID. The client looks up unbound stock
using binding key zero. Outfits and fitted vehicle counts use vehicle *type*
descriptors in the cache. A committed mutation sends the inventory and wallet
diff before the response. Reconnect builds the same inventory from PostgreSQL.

## Evidence

Protocol and filter behavior were traced in the pinned `wot-src` revision
`447437a25a2222a0d10dde96f65a20254d3a7d0d`. Relevant sources under
`sources/res/scripts/` establish the following details.

- `client/gui/impl/lobby/hangar/presenters/vehicle_menu_entries/customization_entry_sub_presenter.py` and `client/gui/shared/utils/requesters/vehicle_items_getter.py` for the Exterior button’s compatible-style check and price-table membership.
- `client/account_helpers/Shop.py` for commands 118 and 119 and their argument order.
- `client/gui/Scaleform/daapi/view/lobby/customization/context/custom_mode.py` for seasonal outfit replacements and the common outfit entry.
- `common/serializable_types/customizations/customization_outfit.py`,
  `camouflage.py` in the same directory and
  `common/serialization/component_bin_serializer.py` for descriptor types, masks and field defaults.
- `common/items/readers/c11n_readers.py`, `common/items/components/c11n_components.py`
  and `common/items/vehicles.py` for inherited definitions, vehicle filters and customizable vehicle areas.
- `client/gui/shared/utils/requesters/InventoryRequester.py` and
  `ItemsRequester.py` for stock, seasonal layouts, fitted counts and diff invalidation.

The [encrypted garage test](../tests/support/camouflage_flow.rs) uses the Account
schema and customization definitions
extracted from installed EU `v.2.4.1.1 #993`
(executable SHA-256 `fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`).
It exercises the shop stream, rejected purchases and outfits, buying against
shared stock, three-surface application, removal, reapplication and a full
sync after reconnect. It also checks exported trace parentage through the
command, purchase decision, parameterized PostgreSQL update and queued response.
The compiled client opened Exterior on the Executor. The garage regression
checks the Last Stand shop exclusions and applies UK camouflage to all three
Executor surfaces. It also checks Stenciled rounded with a leading zero,
rejection of a prohibited number, editing and removing without an extra charge,
a preset inscription in the second turret slot and reconnect persistence.
The Executor effect regression checks the 300-gold price, a rejected unavailable
effect, preservation of inscriptions, removal, stock reuse and reconnect.
The Tiger regression exercises the extracted price, invalid anchors and mirroring,
stock reuse and persistence of its anchor, scale and orientation.
