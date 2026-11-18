# Admin dashboard and HTTP API

The game server can serve a React Admin dashboard to list and create player
accounts and grant resources, tanks and 3D attachments. Accounts creates the player and starter
garage in one transaction. The garage contains regular tech tree tier I tanks
with stock modules, basic crew, default ammunition and initial settings.
They can use their new credentials in the game client's basic login flow
immediately after creation.

The catalog selects starters from the regular nation tech trees and excludes
collector vehicles. The initial settings are a client-compatible baseline
recorded with the EU 2.4.1.5514 resources. See the
[starter settings](../src/accounts/infrastructure/client/starter_settings.json).

## Enable the dashboard

From the repository root, build the SPA with Node.js 22.12 or later.

```sh
npm --prefix dashboard ci
npm --prefix dashboard run build
openssl rand -hex 32
```

Add this top-level property to the `server.json` beside the game server
executable. Replace the example token with the generated value.

```json
{
  "admin": {
    "bind": "127.0.0.1:8080",
    "token": "REPLACE_WITH_THE_GENERATED_TOKEN",
    "dashboard_dir": "../../dashboard/dist"
  }
}
```

The relative dashboard path above works with the executable in either
`target/debug/` or `target/release/`. For a standalone installation, copy the
built directory and set `dashboard_dir` relative to the executable or use an
absolute path. Omit `dashboard_dir` to serve only the API. The default bind is
`127.0.0.1:8080`. The server enables the listener when the `admin` object is
present.

Restart the server, open <http://127.0.0.1:8080/> and sign in with the admin
token. Use **Create** in **Player accounts** to enter the game login, display
name and password. The list displays player names and logins,
newest first. Select a player to open **Resources & Garage**. The page displays
credits, gold, bonds, free XP and owned tanks. Use **Grant resource** to add a
positive whole amount, or search for a tank by name and use **Grant
tank**. Tank grants include stock modules, standard ammunition and basic crew.
The player is charged nothing and needs a free garage slot. The picker lists
tanks eligible for Random Battles and identifies already-owned tanks. Use
**Grant a 3D attachment** to search by name or item ID, see how many copies
the player owns, and add copies to the depot. Ownership includes fitted copies.
The picker includes reward attachments such as Zongzi Jubilee. The browser
stores the admin token in tab session storage.
For remote administration, keep the server on loopback and use an HTTPS
reverse proxy or an SSH tunnel. The built-in listener serves plain HTTP.

For frontend development, `npm --prefix dashboard run dev` starts Vite
with a local API proxy to the default admin listener.

When implementing a player-usable item that the normal shop cannot sell,
include an authenticated admin dashboard grant. Grants preserve the
item's ownership limits and leave its shop availability unchanged.

## API

Send `Authorization: Bearer <admin-token>` with every request.

| Method and path | Response |
| --- | --- |
| `GET /api/admin/session` | `204 No Content` for a valid token. |
| `GET /api/admin/players?page=1&per_page=25` | `200 OK` with `{ "data": [player records], "total": 2 }`. Pages start at 1. Page size is 1–100. Defaults are page 1 and 25 records. |
| `POST /api/admin/players` | `201 Created` with the new player record. |
| `GET /api/admin/players/{id}/garage` | `200 OK` with balances, garage slot count, owned tanks and an `attachments` map from item ID to total owned copies. Balances are decimal strings. |
| `GET /api/admin/vehicles` | `200 OK` with searchable vehicle entries containing `id`, `key`, `name` and `tier`. |
| `GET /api/admin/attachments` | `200 OK` with grantable 3D attachments containing `id`, `name`, and `max_owned` (null if the catalog sets no limit). |
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

Player records contain `id` (a decimal string), `username` and `display_name`.
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
failures and static dashboard requests. See [request source IP](telemetry.md#request-source-ip)
for Kubernetes routing, validation, and the trust boundary.
