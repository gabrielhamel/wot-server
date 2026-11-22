# Garage camouflage and exterior customization

Exterior can buy permanent camouflage into shared stock or buy missing copies
while applying an outfit. A placement consumes one copy per surface per season.
The three garage seasons are winter, summer and desert. The supported surfaces
are the hull, turret and gun, as exposed by the vehicle's current configuration.
Scale and palette selections persist with each placement.

Accounts stores total owned quantities and seasonal outfits by vehicle inventory
ID in the existing progression JSON. It derives unmounted stock from ownership
minus the units fitted across all vehicles. Removing a cosmetic, selling a vehicle while keeping
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
camouflage and its garage layouts. Permanent styles from the [Store](store.md)
are fitted through the common outfit. Cosmetic resale and rentals are outside
the current implementation.

## Paints

The Paints tab uses the base `customization/paints/list.xml` and its price
groups. Paint Colour N1A (ID 141) costs 20 gold per unit in the installed EU
catalog and is available for eligible UK vehicles. The XI Executor accepts it
on its chassis, hull, turret and gun. The installed modules' paint regions
determine which placements the server accepts.

The catalog specifies how many units each region uses, with a default of one.
Each of the Executor's four regions uses one N1A unit per season. Applying
paint consumes owned units first and buys any deficit in the same transaction.
Removing paint or changing to a module that lacks the region releases the
units to the depot. Paint shares the seasonal outfit with camouflage,
inscriptions, effects and projected decals.

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

## 3D attachments

A vehicle has one common attachment layout across all map types. Each fitted
attachment uses one depot copy. Removing it or switching to a configuration
without a compatible anchor frees the copy for reuse. The server checks the
installed modules, anchor type, available scale and whether the item can
rotate. Anchors for the default vehicle model are eligible in a custom outfit.

The extractor includes the installed `customization/attachments/list.xml`.
The EU catalog marks River Race Revelry attachments as outside the shop.
An operator can provision a depot copy from the player’s **Resources & Garage**
page in the [admin dashboard](admin.md).
The group's Zongzi Jubilee dragon boat (item 2033) fits the Executor's turret anchor 38000
at scale 2.

## Stat trackers

A compatible tank can fit one tracker in the common outfit. The client catalog
marks the EU Stat Tracker as vehicle-bound and outside the shop. An operator
can grant it to an owned, compatible tank from the [admin dashboard](admin.md).
The spare copy is keyed by vehicle type. Fitting consumes one copy from that
tank's stock, and removing it returns the copy to the same tank. A common
outfit can hold a tracker and 3D attachments together.

The game's client gets the tracker counter from vehicle dossier statistics.
This garage workflow implements grants, fitting and removal. Battle results
for the counter require a battle statistics source.

## Exterior button

The native hangar menu checks for at least one compatible 2D style before it
enables Exterior. Its lookup requires the style descriptor to be present in
`items.itemPrices`. The server adds empty price rows for base 2D descriptors
that the client already hides from the carousel, and for Store styles so
the client can display owned copies. The native price cache marks styles
as unavailable for purchase through `notInShopItems`. The Store sells
a selected catalog of permanent styles for bonds.

The outfit serializer carries the style ID in field 4 of
`CustomizationOutfit`. When applying a 2D or 3D style, the client repeats its
ID in the winter, summer, desert and common (15) outfit entries. The server
checks the seasonal IDs against the common ID, then saves one shared style
and the separate seasonal overrides. A seasonal entry containing only the
style ID clears that season's previous overrides. Common-only requests also
work. Fitting requires a compatible vehicle and an owned copy. It uses one
copy until removal returns it to the depot. The client loads the style's
appearance from its installed definitions.

The carousel enumerates client definitions directly, including extensions.
Unpriced items can appear selectable at zero gold when their IDs are missing
from the shop exclusions. The extractor includes the base and extension lists
for styles and camouflage, so the shop can exclude event cosmetics without
touching the player's stock. The Last Stand 2025 and Corrupted cards on the
Executor came from `last_stand/scripts/item_defs/customization/styles/list.xml`.
Use the striped Camouflage tab in Exterior to edit individual surfaces and seasons.

Reconnect after updating the bundle and restarting the server so the client
fetches the updated shop cache.

## Styles built into a vehicle

First Warrior is the dedicated `china:Ch65_Type59_IC4_3DSt` vehicle. The
2026-11-22 EU bundle (`v.2.4.1.1 #993`, executable SHA-256 recorded in the
[battle replay guide](battle-replays.md#eu-capture-evidence-from-2026-11-21))
defines style 986 with an explicit filter for that vehicle and the
`lockedOnVehicle` tag. Its `modelsSet` selects First Warrior's 3D model parts.
Accounts resolves the style from the current resource catalog, including
the include and exclude filters, when projecting the vehicle.

In the garage, the customization inventory carries the style marker in the
common outfit (season 15), keyed by vehicle type descriptor. The client
uses the common outfit to build each seasonal appearance. The server
includes it in a full sync for existing vehicles and in the inventory delta
when the vehicle is acquired or sold. The built-in style follows the vehicle.
There is no additional depot item or independent purchase or grant.

In Training, the same encoded style marker travels with the equipped vehicle
and is sent in `Vehicle.publicInfo.outfit`. The client reads that field when
assembling the vehicle's appearance, using the current map's season. Training currently supplies catalog-defined locked styles.

The same Vehicle creation payload goes to the driver and to every other
player in the arena. It contains the outfit and the compact descriptor for
the equipped chassis, engine, fuel tank, radio, turret, gun and optional
devices. Player-selected garage outfits still need battle replication.

The client applies each viewer's customization display preference to other
vehicles. First Warrior's style is classified as non-historical, so a
historical-only viewer can see the base Type 59 model even though the
outfit arrived. Player Sessions forwards the Accounts preference at battle
entry. The client packs it into bits 2 and 3 of `GAME_EXTENDED_2` (setting
102), and the server sends it to that viewer as
`Avatar.customizationDisplayType`. Valid settings are 0 (Historical), 1
(non-historical allowed), and 2 (All). A missing value or reserved value
uses the client's default, All.

Client references for this flow are
[`styles/list.xml`](references/wot-src/sources/res/scripts/item_defs/customization/styles/list.xml),
[`Vehicle.createAppliedOutfits`](references/wot-src/sources/res/scripts/client/gui/shared/gui_items/Vehicle.py),
[`Vehicle.onEnterWorld`](references/wot-src/sources/res/scripts/client/Vehicle.py)
and [the client’s outfit builder and historical filter](references/wot-src/sources/res/scripts/client/vehicle_systems/camouflages.py). The preference encoding is in [ServerSettingsManager](references/wot-src/sources/res/scripts/client/account_helpers/settings_core/ServerSettingsManager.py), [options](references/wot-src/sources/res/scripts/client/account_helpers/settings_core/options.py), and [Avatar.def](references/wot-src/sources/res/scripts/entity_defs/Avatar.def).

## Wire contract

| Command | Arguments |
| --- | --- |
| `118` (`CMD_BUY_C11N_ITEMS`) | `doCmdIntArr` with shop revision, vehicle inventory ID, then item descriptor/count pairs. Inventory ID zero buys unbound stock without vehicle context. |
| `119` (`CMD_VEH_APPLY_OUTFIT`) | `doCmdIntArrStrArr` with shop revision, vehicle inventory ID and a season per binary outfit argument. Each entry replaces the named season. An empty descriptor clears that season. |

Outfit descriptors use unsigned base-128 varints for the component type (4),
field mask, camouflage array length and component fields. The camouflage field
uses mask bit 2 and encodes item ID, pattern size, applied-area mask and palette
index in that order when present. Camouflage regions use bits 4, 8 and 12.
The native customization dialog sends a common outfit (season 15)
alongside the regular seasons. Its attachment array uses bit 9 and each entry
contains the item ID, slot ID, scale ID and rotation flag in that order.
The server accepts up to six distinct attachment slots in the common outfit.
The stat tracker array uses bit 12 and holds at most one item ID and anchor ID
pair. Effects use the modification ID array (bit 0).
Paint uses bit 1 and encodes the paint ID and its region mask. Paint regions
use bits 0, 4, 8, 12 and 14 for chassis, hull, turret, gun and the second gun
region. A single entry can cover several regions. Preset inscriptions use the
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
- `client/gui/Scaleform/daapi/view/lobby/customization/shared.py` (`getStyledModeRequestData`) and `client_common/vehicle_outfit/outfit.py` (`adjust` and `pack`) for the style ID repeated in the four outfit entries.
- `common/serializable_types/customizations/customization_outfit.py`,
  `camouflage.py`, `paint.py` and `attachment.py` and `stat_tracker.py` in the same directory and
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
The Colour N1A regression checks the 20-gold shop price, rejects an
ineligible nation and an unavailable region, and applies paint to four regions
while preserving other fitted cosmetics. It verifies no extra charge on retry,
depot reuse, reconnect persistence and trace parentage for successful and rejected requests.
The attachment regression grants a depot copy of the dragon boat and fits it on
the Executor with the four-entry command sent by the client. It verifies an
unowned item, invalid anchor, scale and rotation are rejected, and checks
single-copy accounting, removal, reapplication, reconnect and the grant and
outfit traces.
The stat tracker regression uses the Executor's hull anchor 18003 in the same
EU build. It checks vehicle-bound stock and fitted counts, incompatible anchors,
retry, removal, reconnect and the grant and outfit traces.
The [Store regression](../tests/support/store_flow.rs) buys and fits Hot TOG
and Montu using the four-entry style request. It checks ownership and vehicle
eligibility, fitted counts, retries, removal, persistence after reconnect and
the application trace through the delivered command response.
