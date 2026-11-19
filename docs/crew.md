# Garage crew and barracks

The account owns individual crew members. Each member has a stable inventory ID,
nation, qualification, vehicle specialization, passport, skill-efficiency XP,
major and bonus perks, unspent XP and an optional vehicle seat and skin. An active
member without a seat occupies one barracks berth. An empty seat is represented by
`None` in the vehicle's crew list, preserving slot indices for multi-role crews.

## Garage operations

- Hire into the barracks or hire directly into an owned vehicle slot. The server
  validates the requested qualification against the vehicle definition. Vehicle
  purchases can also create and seat their crew in the purchase transaction.
- Transfer, replace or unload crew. A seat accepts the corresponding primary
  qualification and nation. A member can occupy one seat at a time. Returning a
  crew uses the remembered assignments, and the vehicle's auto-return preference
  is persisted for the client's automatic return workflow.
- Buy barracks berths in packs of 16. The shop advertises 300 gold per paid pack.
  An account can hold up to 2,048 berths and 8,192 crew records including dismissed
  crew. The remaining record capacity is advertised for free recruitment.
- Dismiss and restore crew. Dismissed members release their seats and berths, and
  worn skins return to stock. Up to 100 dismissed members are recoverable for 32
  days. The first two days are free, then restoration costs 60,000 credits.
- Retrain for another vehicle of the same nation or change primary qualification.
  A change of qualification preserves compatible perks and refunds the XP of
  removed perks. Retraining uses the shop course, discounts based on full perks,
  and the advertised skill-efficiency values.
- Convert free XP to crew XP at the advertised 1-to-5 rate. Choose major perks and
  eligible secondary-role bonus perks. Major perk selection can end with a perk
  at 0% when XP runs out. Each earlier major perk must reach 100% before the next
  can be selected. Bonus perk slots unlock at zero, two and four full major
  perks. The client derives bonus perk levels from major perk progress, so
  with exactly two full major perks a member can select the 100% and 0%
  bonus perks in the same command. Reset perks with the advertised credit,
  gold or free courses and their corresponding XP refunds.
- Toggle Accelerate Crew Training per owned Elite vehicle. Elite status is
  projected from the installed vehicle's research steps and the account's
  unlocks, including stock unlocks. Vehicles that earn Elite status through a
  vehicle skill tree remain outside this rule until that progression exists.
  The toggle preserves any Combat XP already saved on the vehicle.
- Buy and sell crew books through the shop and depot. An administrator can
  [grant any catalog crew book](admin.md#grants). Applying a crew-wide book
  requires a full, trained crew and the book's nation where specified. Personal
  books target an individual. XP and prices are read from installed resources.
- Change an ordinary recruit's name and portrait using the nation's available
  passport groups. Owned crew skins can be equipped and removed. Catalog skins
  with an available price can be acquired through the generic buy-item command.
- Convert undertrained standard crew in the barracks to national crew books.
  The client previews conversion using a 210,060 skill-XP threshold, divides a
  member's XP by the size of their trained vehicle's crew, and rounds the
  national reward into books. Seated and dismissed members are excluded.

Recruitment preserves the milestone's fully qualified initial recruits. The
legacy recruitment price indices remain 0, 1 and 2 for free, 20,000 credits and
200 gold. These prices apply to both individual hires and vehicle purchases.
The three courses have distinct effects when used for retraining.

## Persistence and wire contract

The migration `0001_initial_schema.sql` defines `accounts.crew_roster`. For a null roster,
Accounts reconstructs the existing stock crew using `owned_vehicles.has_crew`,
vehicle inventory order and the installed vehicle roles. The first crew write or
vehicle purchase persists that roster with its ID counter. Returning, dismissing
or converting members leaves their historical inventory IDs reserved.

The server proposes the next snapshot, builds the client diff, and checks the
source revision when committing the roster, balances, depot stock and any new
ownership row. An invalid request or a stale revision cannot spend currency,
apply XP or change a seat. The gateway queues up to 32 pending crew requests per
session while a crew write is running. Each request commits separately in arrival
order. Native group metadata can accompany those requests.

| Command | ID | Arguments after the command header |
| --- | --- | --- |
| Assign or unload | 105 | `[vehicleInvID, slot, tankmanInvID, groupID, groupSize]`. Member `-1` unloads, with slot `-1` for the whole crew. |
| Accelerate Crew Training | 107 | `vehicleInvID, 1, enabled, source` |
| Auto-return preference | 107 | `vehicleInvID, 512, enabled, source` |
| Learn perk or perks | 151, 156 | `tankmanInvID, utilization, perkID` or `[tankmanInvID, utilization, ...perkIDs]` |
| Reset perks | 152 | `shopRev, tankmanInvID, course` |
| Retrain | 153 | `[shopRev, tankmanInvID, course, vehicleCD, groupID, groupSize]` |
| Passport | 154 | `[shopRev, tankmanInvID, premium, sex, firstNameGroup, firstName, lastNameGroup, lastName, portraitGroup, portrait]` |
| Convert free XP | 155 | `[shopRev, tankmanInvID, freeXP, groupID, groupSize]` |
| Return crew | 157 | `vehicleInvID, 0, 0` |
| Change qualification | 158 | `[shopRev, tankmanInvID, role, vehicleCD, groupID, groupSize]` |
| Equip or remove skin | 160, 161 | `tankmanInvID, skinID, 0` or `tankmanInvID, 0, 0` |
| Use crew book | 162 | `[bookCD, count, vehicleInvID, tankmanInvID, groupID, groupSize]`. Unused target is `-1`. |
| Convert undertrained crew | 165 | `0` |
| Buy berth packs | 206 | `shopRev, packCount, 0` |
| Hire | 303 | `shopRev, vehicleCD, role, course` |
| Dismiss | 306 | `tankmanInvID, 0, 0` |
| Hire and seat | 309 | `shopRev, vehicleInvID, slot, course` |
| Restore | 10015 | `[tankmanInvID, groupID, groupSize]` |

Vehicle inventory includes `crew`, `lastCrew` and `settings`. Accelerated
training uses mask `0x01` in `settings`. The enabled vehicle inventory IDs are saved
in the crew roster, with older rosters defaulting to off. Repeating the same
setting succeeds without creating another revision. The client uses
`stats.eliteVehicles` to update availability after research.
Tankman inventory
maps IDs to compact descriptors and assigned vehicle IDs. The recycle bin
contains `tankmen.buffer` rows of `(compact descriptor, dismissal timestamp)`.
Crew skin inventory uses subtable 1 for available stock by skin ID and subtable
2 for worn skins by member ID. Incremental updates remove rows with `None`.
Successful hires include the new member's ID and descriptor in the response.

## Battle XP routing

The Accounts domain can apply a trusted battle award to the crew and training
setting recorded for that battle. Each participating member receives the base
Crew XP. With acceleration enabled, Combat XP goes to the eligible member
with the largest efficiency deficit, then the least skill XP, then the earliest
seat. Combat XP accrues on the vehicle when acceleration is off or every member
has reached the current training cap. Crew awards use the existing cap. The
server still needs a battle settlement path to call this rule and commit the
result. Veteran XP conversion to Crew Books also remains pending.

Quick Training looks up the post-progression reward book if any crew member
on the account has reached the skill XP cap. The server supplies `rewardBookId`
(Universal Guide, raw book ID 62) and `XppToConvert` (100,000). Without the
reward ID, the client defaults to ID 0 and fails to render the entire book
list. The client defaults the unearned `stats.XPpp` balance to zero.
The reward ID comes from `scripts/item_defs/crewBooks/crewBooks.xml` in the
installed EU client (executable SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`).
The lookup and fallback are in the reference client's
`gui/impl/lobby/crew/container_vews/quick_training/` components and
`helpers/server_settings.py`.

## Evidence and verification

- `docs/references/wot-src/sources/res/scripts/common/AccountCommands.py` and
  `client/account_helpers/{Inventory,Shop,Stats,client_recycle_bin}.py` record
  command IDs, argument layouts and callback shapes.
- `client/gui/shared/gui_items/{Vehicle,processors/vehicle}.py` records the
  Elite cache lookup and training flag. `common/items/tankmen.py`
  (`getLessMasteredIDX`) gives the crew ranking used by the hangar highlight.
  The [official Crew Training guide](https://worldoftanks.eu/en/content/guide/game-mechanics-and-achievements/crew-training/)
  describes the Combat XP transfer.
- `common/items/{tankmen,vehicles,crew_junk_convert_helper}.py` and
  `common/items/components/skills_constants.py` record descriptors, secondary
  roles, perk eligibility, XP curves and conversion previews.
  `client/gui/shared/gui_items/Tankman.py` (`bonusSlotsLevels`) records the
  inclusive bonus slot milestones, and `tankmen.py` derives the levels from
  major perk progress.
- `scripts/item_defs/{vehicles,tankmen,crewBooks,crewSkins}` from the installed EU
  resources supply catalog data. The directory name with `2.4.1.5498` is an
  alias to the installed EU game, as described in [client validation](client-validation.md).
- The [PostgreSQL garage integration](testing.md#postgresql-garage-integration)
  exercises encrypted commands, inventory and recycle-bin diffs, bounded grouped
  requests, wallet updates, and full sync after reconnect. Native GUI rendering
  of these flows remains unverified.

Battle settlement, crew dossiers, reward and token recruits with preset or zero perks,
mentoring licenses, recertification forms and event-specific crew remain pending.
