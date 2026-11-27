# Web panel and HTTP API

The game server can serve a web panel for player registration, read-only access
to a player's garage, and server administration. Accounts creates each web
account with 500 gold, 200,000 credits, zero bonds, zero free XP, and a starter
garage in one transaction. The balance defaults apply to new accounts. The garage contains the regular Tech Tree tier I tanks with stock
modules, basic crew and default ammunition. Players can use their new
credentials in the game client's basic login flow immediately after creation.

The catalog selects the tier I starters from regular nation tech trees and
excludes collector vehicles. The initial settings use the
[starter settings](../src/accounts/src/infrastructure/client/starter_settings.json).

Players and administrators share the **My garage** and **Store** views when
the server enables the [Store](store.md#player-dashboard). Administrators can
open player management and grants from the **Admin** tab.

## Enable the dashboard

Cargo builds the dashboard and Store bundle when building the server. To build
just the browser bundle, run this command from the repository root:

```sh
cargo web-build
```

Both builds require Node.js and npm. Use Node.js 22.22.2 or a newer supported
release. Cargo installs the locked dependencies as needed and writes the
browser bundle to `src/server-runtime/src/infrastructure/web/dist/`.

The dashboard listens on all IPv4 interfaces at port 8080 by default. Pass
`--web-bind 127.0.0.1:8080` to limit it to the local machine. Pass
`--web-ui-dir /path/to/dist` to serve a separate build, or `--disable-admin`
to disable the dashboard and admin API.

Restart the server and open <http://127.0.0.1:8080/dashboard/>. If the database has
no players, the dashboard opens the first administrator form. Enter an email
and password that meet the player registration rules below, and choose a
display name. After creating the administrator, sign in with those credentials.
Limit access to the listener until setup is complete.

### Player registration and sign-in

Once the first administrator exists, choose **Create a player account** on the
sign-in page. The email address is the sign-in name for the game and the panel.
The username is the name other players see in the game. Use 3–24 characters
from A–Z, a–z, 0–9 and underscores. This follows the character set published
by [Wargaming registration](https://wargaming.net/registration/en/?change_realm=1&login_form=enabled).
Both forms show a field error for an invalid username and check availability
as soon as the name has a valid format.
The email field shows a message for an invalid address or an address already
in use. The API checks both email and username for case-insensitive duplicates
when saving the account. Sign in with the email as entered during registration.

Use a password of 8–128 characters with at least one digit, one uppercase
letter, one lowercase letter and one symbol or punctuation mark. Enter it
twice. The confirmation page shows how to sign in to the game immediately.
The panel sends no verification email. Returning players can sign in to see
their own credits, gold, bonds, free XP and tanks in **My garage**.

The panel keeps a session for 60 days from sign-in, across browser and server
restarts. Choose **Sign out** to revoke that browser session. Changing an
account's role revokes all of its web sessions. The account can sign in again
with its current permissions.

### Administration

Administrators sign in with their game credentials and open **Admin** to see
**Player accounts**.

In **Player accounts**, choose **Create** and enter the player's email and
in-game username. The form uses the same validation and username availability
check as registration. Choose **Create player** to generate a password and
create the account. Copy the sign-in email and generated password from the
confirmation screen before choosing **Done**.

Use the role action in another player's row to promote them to **Administrator**
or demote them to **Player**. Promotion requires an email address as the player's game login.
Administrators can change other players' roles only, so at least one
administrator always retains access. The list displays player names and logins,
newest first. Select a player to open **Resources & Garage**. The page displays
credits, gold, bonds, free XP, Experimental Equipment components and owned
tanks. Choose a resource, including **Experimental Equipment components**,
from the **Resource** dropdown, enter a positive whole amount, and select
**Grant resource**. To add a tank, search by name and
select **Grant tank**. Tank grants include stock modules, standard ammunition
and basic crew.
The player is charged nothing and needs a free garage slot. The picker lists
tanks eligible for Random Battles and identifies already-owned tanks. Use
**Grant a 3D attachment** to search by name, see how many copies
the player owns, and add copies to the depot. Ownership includes fitted copies.
The picker includes reward attachments such as Zongzi Jubilee. Use **Grant a
stat tracker** to select a compatible owned tank and add a vehicle-bound copy
to its depot stock. Use **Grant crew books** for national, universal and
personal crew books, **Grant Bounty Equipment** for Basic and Upgraded
pieces, and **Grant Experimental Equipment** for tiers 1–3. These pickers
show the player's unmounted depot stock.

Use **Grant a 2D style** or **Grant a 3D style** for selectable styles such as
Veteran and Proserpina. The 3D picker shows compatible tank names in parentheses
and accepts searches by style or tank name. Styles restricted to outfit-locked
tanks show that restriction alongside the tank name. The 2D picker also labels
nation or tier restrictions, such as **Black Widow (France)**, and known color
variants. Entries with the same remaining label get a variant number. Use
**Grant a crew skin** for EU skins such as Birthday Skill4ltu. The pickers show
total ownership, including copies in use. Use **Grant a special recruit** to
add an event or other special crew member to the player's barracks. The
recruit picker lists each catalog group once. Choose a country from that
recruit's available countries. The picker preselects a single eligible country
and disables country options the player already owns. A special recruit uses
one berth and is limited to one copy per nation and group.

For remote administration, serve the dashboard through an HTTPS reverse
proxy and start the server with `--secure-cookies`. Route
`/dashboard/`, `/assets/*` and `/api/admin/*` to the web listener. By default,
the dashboard supports sign-in over HTTP.

### Launch a Training battle

Open **Admin → Battles** with Training enabled. Select the map and battle
type, then enter a whole number from 5 to 30 for the round duration in minutes.
Add one or more players connected to the game in their garage. Assign each
player to a team and choose an owned vehicle or the Observer seat when
available. The picker disables tanks with incomplete crews and shows why a
tank or player is unavailable.
Players already in battle or in a Training Room appear disabled. **Refresh
players** updates availability after a crew change, login, disconnect or
garage return. The server checks readiness again when starting the battle.

Copy the address from **Battles** to reopen the setup or share it with another
administrator on the same server. The link restores the map, duration, battle
parameter overrides, teams, vehicle choices and applied player and dummy
loadouts. Select **Apply loadout** before copying the link. The setup stays in
the address after launch, and the dashboard refreshes player availability.
Replace any selections that are unavailable when you reopen the link.

Expand **Battle parameters** to tune the arena's round timing, movement,
contacts, projectile and damage simulation, and tracer visibility. Each field
starts at the server default and shows its supported range when the entry is
invalid. **Reset parameters to defaults** clears all overrides. Edits apply to
the battle started from this setup. Training launched from the native room
uses server defaults.

All battles use Rapier. Adjust suspension and lateral grip under **Hull physics**. See [battle physics and tuning](battle-mechanics.md#rapier-hull-physics).

A player's seat starts with their owned vehicles. Select **Customize tank**
to open the selected vehicle's equipped modules, equipment, consumables,
crew perks, perk efficiency, hit points and ammunition. Edits apply to one
battle. The arena uses the player's saved style and cosmetics for the map
season and their other garage settings.

Select **Choose custom tank** to pick a vehicle from the shared battle catalog,
including tanks the player does not own. Use **Customize tank** to open the same loadout form used
by dummies. The player controls the configured tank for this battle and
returns to their existing garage afterward. **Show owned tanks** restores
the previous owned vehicle or Observer selection and keeps the custom draft
available for this setup.

Use **Add dummy** to fill either team with stationary targets. Each dummy starts
with stock modules, default ammunition and 100% perk efficiency in every crew
seat. Custom player tanks start with the same defaults. Open **Customize Dummy** to select equipment, consumables and fully
trained perks, then use each seat’s slider to set perk efficiency from 0% to
100%. The simulator scales supported perk effects by that percentage. A crew
member at 50% contributes half the effective training for their selected perks.
[Wargaming's crew training guide](https://worldoftanks.eu/en/content/guide/game-mechanics-and-achievements/crew-training/)

Set **Hit points** from 1 to 32,767 to override both starting and maximum
HP. Leave it blank to use equipped HP, including any gain from Improved
Hardening. Changing the turret updates compatible guns and equipped HP,
including applicable equipment and consumable bonuses. The selected turret
and gun determine available shell types and the rack capacity. The first
compatible shell type fills a custom tank's rack by default. The panel shows
the combined hull and module weight against the selected suspension's limit.
Apply stays disabled above that limit until you pick a higher-capacity
suspension or lighter modules. The server checks the combination at launch. An owned loadout keeps its ammunition when the new weapon
can carry it and uses the new weapon's default when it cannot.
Set all counts to zero to test a dummy with an empty ammo rack. Dummies remain
in place when pushed, take hits, and have no player controls to fire or activate
a consumable.

Start with at least one connected player and a combat vehicle on each team.
A single player can face a team of dummies. A battle accepts up to 30 entrants
total. You can add players to a draft already filled with dummies. Remove the
number of surplus entrants shown by the dashboard before starting the battle.
The selected game clients transition directly from the garage to the Training
arena. Players already in a Training Room or battle become available once
they return to the garage and leave their room.

### Upgrade an existing database

Existing accounts start with the **Player** role. If the database already has
players, apply the server migrations and use a trusted PostgreSQL connection
to promote a known account. Choose a player whose game login is an email
address and whose password you know. Confirm that the statement updated one
row before signing in.

```sql
UPDATE accounts.accounts SET role = 'admin' WHERE username = 'operator@example.com';
```

### Frontend development

From the repository root, run `npm ci` and `npm run dev`. Open `/dashboard/`
or `/shop/` on the URL Vite prints. Vite development uses MSW by default.
Both pages open with demo data without Rust, PostgreSQL, or a connected game
client. The Store replaces old garage session fragments with its demo session.
The first visit starts with the demo administrator. The browser remembers the
demo sign-in choice across refreshes, including sign-out. Sign in again with
`admin@example.test` and `demo-admin`. Player registration, account creation,
role changes, garage grants and Store purchases update the demo data in memory.
Reload the page to reset those changes.

Use `npm run dev:server` to proxy API requests to a running server at
`http://127.0.0.1:8080`.

When implementing a player-usable item that the normal shop cannot sell,
include an authenticated admin dashboard grant. Grants preserve the
item's ownership limits and leave its shop availability unchanged. The dashboard
uses display names and descriptive fallbacks. Technical IDs and internal keys
belong in the API and application state.

## API

The self-service garage requires a session cookie. Player management,
grants and battle launches require a current administrator session. Send
`X-Wot-Admin: 1` on every POST and `Content-Type: application/json` when
sending a JSON body. The browser sends the cookie on same-origin API calls.

| Method and path | Response |
| --- | --- |
| `GET /api/admin/session` | `200 OK` with `status` set to `setup_required`, `signed_out`, or `authenticated`. A signed-out response includes `registration_open`. An authenticated response includes the current `player` record and role, plus the `store_enabled` flag. |
| `POST /api/admin/setup` | Creates the first administrator with the player creation fields below. Returns `201 Created` without signing in, `422 Unprocessable Entity` for invalid fields, or `409 Conflict` when the database already has players. |
| `POST /api/admin/session` | Accepts `{ "email": "...", "password": "..." }`. Returns `204 No Content` and sets the session cookie on success, or `401 Unauthorized` for invalid credentials. |
| `POST /api/admin/logout` | Revokes the current browser session and clears its cookie. Returns `204 No Content`. |
| `POST /api/admin/registration/username` | Accepts `{ "username": "NewPlayer" }` and returns `{ "available": true }` or `{ "available": false }` in a `200 OK` response. Returns `409 Conflict` while registration is closed. |
| `POST /api/admin/registration` | Accepts `email`, `username`, `password` and `confirm_password`. Returns `201 Created` with the new Player record. Returns `409 Conflict` for a duplicate email or username or when registration is closed, and `422 Unprocessable Entity` for invalid fields. |
| `GET /api/admin/me/shop/catalog`, `POST /api/admin/me/shop/orders` | Browse or buy from the [Store](store.md#player-dashboard) as the signed-in account when enabled. |
| `GET /api/admin/me/garage` | `200 OK` with the signed-in player's balances, garage slot count and owned tank names and tiers. Balances are decimal strings. |
| `GET /api/admin/players?page=1&per_page=25` | `200 OK` with `{ "data": [player records], "total": 2 }`. Pages start at 1. Page size is 1–100. Defaults are page 1 and 25 records. |
| `POST /api/admin/players` | `201 Created` with the new player record. The new account has the `player` role. |
| `POST /api/admin/players/{id}/role` | Accepts `{ "role": "admin" }` or `{ "role": "player" }` and returns `204 No Content`. Self-role changes return `403 Forbidden`. |
| `GET /api/admin/players/{id}/garage` | `200 OK` with balances, garage slot count, owned tanks, an `attachments` map from item ID to total owned copies, and a `depot` map from grantable crew book, Bounty Equipment and Experimental Equipment IDs to depot counts. The `styles_2d`, `styles_3d`, `crew_skins` and `special_recruits` maps track owned copies. Each tank also exposes its inventory ID and compatible tracker IDs mapped to owned counts. Balances are decimal strings. |
| `GET /api/admin/battles` | Returns `maps`, connected `players` and their owned `vehicles`, with `unavailable_reason` for disabled choices, plus `observer_available`, the shared custom vehicle catalog in `dummy_vehicles` and `parameters` with each tunable field’s key, default, bounds and units in its label. |
| `GET /api/admin/battles/vehicles/{vehicle}` | Returns equipment and consumable slot counts, compatible items, perks by crew seat, `stock_hit_points`, `ammunition_capacity` and compatible `ammunition` entries with default round counts, and the stock modules, module choices and turret/gun combinations for the catalog vehicle. |
| `GET /api/admin/battles/players/{account}/vehicles/{inventory}` | Returns the same loadout options, the connected player's equipped loadout in `current`, and `equipped_hit_points`. |
| `POST /api/admin/battles` | Accepts `arena_type`, `duration_seconds`, `players`, `dummies` and optional `parameters`. Returns `battle_id` and `players_handed_off` after queuing activation, `400` for an invalid setup, `409` for an unavailable player or owned vehicle, or `503` when the queue is busy. |
| `GET /api/admin/vehicles` | `200 OK` with searchable vehicle entries containing `id`, `key`, `name` and `tier`. |
| `GET /api/admin/crew-books`, `GET /api/admin/improved-equipment`, `GET /api/admin/bounty-equipment`, `GET /api/admin/experimental-equipment` | `200 OK` with grantable depot items containing `id` and `name`. |
| `GET /api/admin/attachments` | `200 OK` with grantable 3D attachments containing `id`, `name`, and `max_owned` (null if the catalog sets no limit). |
| `GET /api/admin/styles-2d`, `GET /api/admin/styles-3d`, `GET /api/admin/crew-skins` | `200 OK` with grantable items containing `id`, `name`, and `max_owned`. The grant request kinds are `style2d`, `style3d` and `crew_skin`, each with an `id` and `count`. |
| `GET /api/admin/special-recruits` | `200 OK` with each recruit group's `id`, `name`, and `countries` (each with an `id` and `name`). The `special_recruit` grant takes the selected country's `id`. An already-owned group returns `409 Conflict`. |
| `GET /api/admin/stat-trackers` | `200 OK` with grantable trackers containing `id`, `name`, and `max_owned`. |
| `POST /api/admin/players/{id}/grants` | `200 OK` with `garage` and `delivery` after commit. |

Administrator setup and managed player creation accept the following JSON fields.

```json
{
  "username": "newplayer@example.com",
  "display_name": "NewPlayer",
  "password": "Example2!Password"
}
```

For managed players, `username` is the sign-in email and `display_name` is
the in-game username. The API uses the registration rules for email, username
and password, including case-insensitive duplicate checks for both names.
The admin browser generates a 24-character password. The confirmation
screen displays it until you leave. Login matching is case-sensitive.

First-administrator setup uses the player registration rules for the sign-in
email and password. Enter the password once. The display name accepts 1–128
characters, including internal spaces, and rejects control characters and
surrounding whitespace. The API stores salted Argon2id hashes.

Player records contain `id` (a decimal string), `username`, `display_name`
and `role` (either `player` or `admin`).
Errors include a `message` and may include field-specific `errors`. Invalid
credentials yield `401`, invalid account fields yield `422`, and duplicate
logins yield `409`. Creation returns `503` while two other creations are in
progress. Request bodies are capped at 8 KiB. A successful creation commits the account and inventory together.
If the connection drops during a creation, look up the login in the list
before retrying.

### Battle roster payload

The optional `parameters` object maps server-provided keys to numeric
overrides, for example `{"countdown_seconds": 2}`. Omitted keys use their
defaults. Unknown keys and values outside the advertised bounds return HTTP
400. Fields marked `integer` accept whole numbers.

Each player entry contains `account_id` and `team` (1 or 2). Supply an
`inventory_id` from that player's available vehicles for an owned tank. An
optional `owned_overrides` uses the shared loadout format for that inventory
entry and must name its catalog vehicle. Omitted edits use the live garage
snapshot. Supply a catalog `loadout` with an omitted or null `inventory_id` for a
custom tank. With both fields omitted or null, the seat is an Observer when
`observer_available` is true.

Each dummy entry contains `team` and a `loadout`. Both custom player tanks
and dummies use the same loadout format with a catalog `vehicle` ID.
Optional `modules` maps module positions to IDs. Omitted positions use stock
on a custom tank and the installed module on an owned tank. Optional
`equipment` and
`consumables` arrays contain item IDs in slot order with `0` for an empty
slot. Optional `crew_perks` contains one array of perk IDs per crew seat.
Optional `crew_perk_efficiency` contains whole percentages in crew seat order.
Omitted or null seats use the underlying crew values, including 100%
efficiency for an unedited custom seat. Optional `hit_points` overrides the equipped
vehicle's starting and maximum HP. Optional `ammunition` contains pairs of
shell ID and round count. Omitted shell types in a custom layout carry zero
rounds. Omit the entire field to keep an owned tank's compatible ammunition
or to fill a custom tank's rack with the first compatible shell type.

### Grants

Send one of these JSON bodies to the grants endpoint.

```json
{ "kind": "currency", "currency": "equipment_components", "amount": 500 }
```

```json
{ "kind": "vehicle", "vehicle": 58625 }
```

```json
{ "kind": "attachment", "id": 2033, "count": 1 }
```

An attachment grant adds 1–100 copies to the depot, subject to the item's
ownership limit. Use an item ID from `GET /api/admin/attachments`. ID 2033 is the
Zongzi Jubilee dragon boat from River Race Revelry in the installed EU build. The HTTP endpoint
accepts the grant even when the catalog marks the attachment outside the shop.

```json
{ "kind": "stat_tracker", "vehicle": 12, "id": 1, "count": 1 }
```

A stat tracker grant uses the owned tank's inventory ID to validate its current
anchor and eligibility. The copies belong to that vehicle type, including
after removal.

Currency identifiers are `credits`, `gold`, `bonds`, `free_xp`, and
`equipment_components`. Components use the same account balance as
Experimental Equipment transactions in the game. Both garage APIs return
balances as decimal strings. The resulting balance must fit the grant ceiling
of 2,147,483,647.

```json
{ "kind": "crew_book", "id": 286, "count": 3 }
```

```json
{ "kind": "experimental_equipment", "id": 39161, "count": 1 }
```

```json
{ "kind": "bounty_equipment", "id": 13305, "count": 1 }
```

```json
{ "kind": "improved_equipment", "id": 28665, "count": 1 }
```

Depot grants add 1–100 copies at a time. Use the compact descriptor returned
by the appropriate catalog endpoint. The crew book catalog comes from
`scripts/item_defs/crewBooks/crewBooks.xml`. Experimental Equipment is
identified by `modernized_1`, `modernized_2`, or `modernized_3`. Bounty
Equipment is identified by `trophyBasic` or `trophyUpgraded`. Improved Equipment is identified by `deluxe`. All three use
`scripts/item_defs/vehicles/common/optional_devices.xml`. The Bounty picker
appends the level since both versions can share a name. Catalog names use
the bundle's localized strings when available. The catalog and garage sync
were verified with resources extracted from EU `v.2.4.1.1 #993`
(`WorldOfTanks.exe` SHA-256 `fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`).
Grants add stock for the existing training and mounting flows. Equipment
purchase and mounting rules
still apply. The resulting depot stock per item must fit a signed 32-bit
integer.

Use a vehicle ID from `GET /api/admin/vehicles`. The picker and grant endpoint
accept tier I–XI garage vehicles, including eligible `private` and `testTank`
variants. The server checks that those variants have a garage descriptor,
crew, stock ammunition, health and battle performance. An eligible variant can
be granted regardless of its vehicle definition price. The catalog excludes
mode-specific vehicles, observers, pillboxes, premium IGR variants, vehicles
hidden by `mode_hidden`, and entries explicitly forbidden by
`forbidden_vehicles_to_battle_config.xml`. The pinned client reference shows
the carousel exclusions in `carousels/basic/carousel_data_provider.py` and
prebattle checks in `prb_control/entities/base/limits.py`. Names come from the
installed localization files when present. The granted tank and its stock
modules become available in account progression.

An unknown player returns `404`. An invalid amount, unknown or unsupported tank,
unknown catalog item, or stock, ownership or balance over the limit returns
`422`. A duplicate tank, full garage or account write already in progress
returns `409`. Refresh the player's garage if
a submission loses its HTTP response before deciding whether to submit again.

After a revision-checked commit, the game gateway refreshes its account snapshot
and queues a reliable Mercury `Account.update` for the synced garage session
(`queued_for_garage`). The balance diff carries the new absolute value in
`stats.credits`, `stats.gold`, `stats.crystal`, `stats.freeXP`, or
`stats.equipCoin`. Crew book grants update the crew book (14) table. Bounty
and Experimental Equipment grants update optional device inventory (9). Tank grants use
the same inventory tables as garage purchases. Attachment grants use the
customization depot table. `on_next_sync` means the
committed snapshot will be available at the next sync, including for an offline
player or one still connecting.

The decompiled `client/Account.py` calls `onClientUpdated`, which dispatches the
diff through `client/gui/shared/personality.py` to the garage and wallet. The
client's `client/gui/game_control/SoundEventChecker.py` listens for `stats`
changes and plays native earn sounds for credits, gold and bonds. The money
currency set in `client/gui/shared/money.py` excludes free XP, whose balance
refreshes via the wallet callback.

### Client IP telemetry behind a proxy

Set `WOT_ADMIN_TRUSTED_PROXY_IPS` to the exact comma-separated proxy peer IPs
only when your proxy replaces `X-Forwarded-For` with the visitor's socket IP.
The server otherwise records the direct peer, including on authentication
failures. Shared UI requests appear under `web.ui.request`. See [request source IP](telemetry.md#request-source-ip)
for Kubernetes routing, validation, and the trust boundary.
