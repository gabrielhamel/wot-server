# Admin dashboard and HTTP API

The game server can serve a React Admin dashboard to list and create player
accounts and grant resources, tanks, 3D attachments and stat trackers. Accounts
creates the player with 500 gold, 200,000 credits, and a starter garage in one
transaction. The garage contains the regular Tech Tree tier I tanks with stock
modules, basic crew and default ammunition. Players can use their new
credentials in the game client's basic login flow immediately after creation.

The catalog selects the tier I starters from regular nation tech trees and
excludes collector vehicles. The initial settings are a client-compatible
baseline recorded with the EU 2.4.1.5514 resources. See the
[starter settings](../src/accounts/infrastructure/client/starter_settings.json).

## Enable the dashboard

From the repository root, build the SPA with Node.js 22.12 or later.

```sh
npm ci
npm run build --workspace tools/web
```

Add these top-level settings to the `server.json` beside the game server
executable for local HTTP development.

```json
{
  "web": {
    "bind": "127.0.0.1:8080",
    "ui_dir": "../../tools/web/dist"
  },
  "admin": {
    "secure_cookies": false
  }
}
```

The relative UI path works with the executable in `target/debug/` or
`target/release/`. Use `web.ui_dir` to point to the built web project. Omit it
for admin API-only mode. The shared listener defaults to `127.0.0.1:8080`.
Legacy `admin.bind` and `admin.dashboard_dir` still supply the listener
settings when there is no `web` object.

Restart the server and open <http://127.0.0.1:8080/admin/>. If the database has
no players, the dashboard opens the first administrator form. Enter an email,
a display name and a password, then sign in with those credentials. Limit
access to the listener until setup is complete.

Administrators sign in with the email and password they use in the game.
The dashboard keeps the session for 60 days from sign-in, across browser
and server restarts. Choose **Sign out** from the profile menu to revoke
that browser session. Demoting an
administrator revokes all their dashboard sessions immediately.

In **Player accounts**, use **Create** to add a player. Use the role action in
another player's row to promote them to **Administrator** or demote them to
**Player**. Promotion requires an email address as the player's game login.
Administrators can change other players' roles only, so at least one
administrator always retains access. The list displays player names and logins,
newest first. Select a player to open **Resources & Garage**. The page displays
credits, gold, bonds, free XP and owned tanks. Use **Grant resource** to add a
positive whole amount, or search for a tank by name and use **Grant
tank**. Tank grants include stock modules, standard ammunition and basic crew.
The player is charged nothing and needs a free garage slot. The picker lists
tanks eligible for Random Battles and identifies already-owned tanks. Use
**Grant a 3D attachment** to search by name, see how many copies
the player owns, and add copies to the depot. Ownership includes fitted copies.
The picker includes reward attachments such as Zongzi Jubilee. Use **Grant a
stat tracker** to select a compatible owned tank and add a vehicle-bound copy
to its depot stock.

For remote administration, serve the dashboard through an HTTPS reverse
proxy and use `"admin": {}` in `server.json`. Secure cookies are enabled by
default. Use `"secure_cookies": false` only when accessing the listener over
local HTTP, including an SSH tunnel to the loopback listener.

### Upgrade an existing database

Existing accounts start with the **Player** role. If the database already has
players, apply the server migrations and use a trusted PostgreSQL connection
to promote a known account. Choose a player whose game login is an email
address and whose password you know. Confirm that the statement updated one
row before signing in.

```sql
UPDATE accounts SET role = 'admin' WHERE username = 'operator@example.com';
```

### Frontend development

From the repository root, run `npm ci` and `npm run dev`. Open `/admin/`
or `/shop/` on the URL Vite prints. Vite development uses MSW by default.
Both pages open with demo data without Rust, PostgreSQL, or a connected game
client. The Store replaces old garage session fragments with its demo session.
The dashboard starts signed in, even when a saved URL points to the login page.
After signing out, use `admin@example.test` and `demo-admin`, or reload.
Player creation, role changes, garage grants, and Store purchases update
the demo data in memory. Reload the page to reset it.

Use `npm run dev:server` to proxy API requests to a running server at
`http://127.0.0.1:8080`.

When implementing a player-usable item that the normal shop cannot sell,
include an authenticated admin dashboard grant. Grants preserve the
item's ownership limits and leave its shop availability unchanged. The dashboard
uses display names and descriptive fallbacks. Technical IDs and internal keys
belong in the API and application state.

## API

Protected routes require the session cookie returned by sign-in. Send
`X-Wot-Admin: 1` on every POST and `Content-Type: application/json` when
sending a JSON body. The browser sends the cookie on same-origin API calls.

| Method and path | Response |
| --- | --- |
| `GET /api/admin/session` | `200 OK` with `status` set to `setup_required`, `signed_out`, or `authenticated`. An authenticated response includes the current `player` record. |
| `POST /api/admin/setup` | Creates the first administrator with the player creation fields below. Returns `201 Created` without signing in, or `409 Conflict` when the database already has players. |
| `POST /api/admin/session` | Accepts `{ "email": "...", "password": "..." }`. Returns `204 No Content` and sets the session cookie on success, or `401 Unauthorized` for invalid credentials or a player without the administrator role. |
| `POST /api/admin/logout` | Revokes the current browser session and clears its cookie. Returns `204 No Content`. |
| `GET /api/admin/players?page=1&per_page=25` | `200 OK` with `{ "data": [player records], "total": 2 }`. Pages start at 1. Page size is 1–100. Defaults are page 1 and 25 records. |
| `POST /api/admin/players` | `201 Created` with the new player record. The new account has the `player` role. |
| `POST /api/admin/players/{id}/role` | Accepts `{ "role": "admin" }` or `{ "role": "player" }` and returns `204 No Content`. Self-role changes return `403 Forbidden`. |
| `GET /api/admin/players/{id}/garage` | `200 OK` with balances, garage slot count, owned tanks and an `attachments` map from item ID to total owned copies. Each tank also exposes its inventory ID and compatible tracker IDs mapped to owned counts. Balances are decimal strings. |
| `GET /api/admin/vehicles` | `200 OK` with searchable vehicle entries containing `id`, `key`, `name` and `tier`. |
| `GET /api/admin/attachments` | `200 OK` with grantable 3D attachments containing `id`, `name`, and `max_owned` (null if the catalog sets no limit). |
| `GET /api/admin/stat-trackers` | `200 OK` with grantable trackers containing `id`, `name`, and `max_owned`. |
| `POST /api/admin/players/{id}/grants` | `200 OK` with `garage` and `delivery` after commit. |

Send `Content-Type: application/json` and the following fields for creation.

```json
{
  "username": "newplayer@example.com",
  "display_name": "NewPlayer",
  "password": "a-player-password"
}
```

Login matching is case-sensitive. Logins are 1–128 bytes and contain no
whitespace or control characters. A leading `{` is reserved for the game
protocol's login envelope. Display names are 1–128 characters with no control
characters or surrounding whitespace. Passwords are 1–1024 bytes and contain
no control characters. The API stores salted Argon2id hashes in PostgreSQL.

Player records contain `id` (a decimal string), `username`, `display_name`
and `role` (either `player` or `admin`).
Errors include a `message` and may include field-specific `errors`. Invalid
credentials yield `401`, invalid account fields yield `422`, and duplicate
logins yield `409`. Creation returns `503` while two other creations are in
progress. Request bodies are capped at 8 KiB. A successful creation commits the account and inventory together.
If the connection drops during a creation, look up the login in the list
before retrying.

### Grants

Send one of these JSON bodies to the grants endpoint.

```json
{ "kind": "currency", "currency": "bonds", "amount": 500 }
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

Currency identifiers are `credits`, `gold`, `bonds`, and `free_xp`.
The resulting balance must fit the server's grant ceiling of 2,147,483,647.
Use a vehicle ID from `GET /api/admin/vehicles`. Both the picker and the grant
endpoint restrict grants to tier I–XI Random Battles vehicles. The filter uses
the installed catalog's tags and its `forbidden_vehicles_to_battle_config.xml`.
It follows the mode tags in `common/constants.py` and the Random Battles platoon
criteria in `client/gui/impl/lobby/platoon/platoon_config.py`, including the
Maps Training exclusion. Permanent grants also exclude private, test, observer,
pillbox and premium IGR entries. This excludes the private WT 12 variant
(`germany:G98_Waffentrager_E100_WO`) while allowing regular rewards such as
the ISU-130. Names come from the installed localization files when present.
The granted tank and its stock modules become available in account progression.

An unknown player returns `404`. An invalid amount, unknown or unsupported tank,
unknown attachment, or ownership or balance over the ceiling returns `422`. A duplicate tank, full garage or
account write already in progress returns `409`. Refresh the player's garage if
a submission loses its HTTP response before deciding whether to submit again.

After a revision-checked commit, the game gateway refreshes its account snapshot
and queues a reliable Mercury `Account.update` for the synced garage session
(`queued_for_garage`). The balance diff carries the new absolute value in
`stats.credits`, `stats.gold`, `stats.crystal` or `stats.freeXP`. Tank grants
use the same inventory tables as garage purchases. Attachment grants use the
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
