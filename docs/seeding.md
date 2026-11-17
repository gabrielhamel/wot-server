# Demo database seed

Run independently of the game server:

```sh
docker compose up -d --wait
cargo seed --config target/release/server.json
```

The alias runs `cargo run -p wot-seed --`. It opens no game/chat sockets and
runs embedded migrations. PostgreSQL and an [extracted resource bundle](resources.md) must be available.
No server startup is required.

It reads `server.json` beside its executable when present (`target/debug/` for
`cargo seed`). Without that file, it uses the server's local database and `resources` directory
beside the seed executable. Overrides:

```sh
cargo seed --config target/release/server.json
cargo seed --resource-dir '/path/to/resources' --database-url 'postgresql://user:password@localhost/database'
cargo seed --help
```

Relative JSON resource paths resolve against the configuration directory; CLI paths
resolve against the working directory. No configuration files are written.

## Demo accounts

| Display name | Login email | Password |
| --- | --- | --- |
| Player_1 | player1@demo.demo | demo |
| Player_2 | player2@demo.demo | demo |

Both receive the tier I tanks listed in the installed nation's regular tech
tree. The command reads the `nodes` in `gui/flash/techtree/<nation>-tree.xml`
and looks up their tier in `scripts/item_defs/vehicles/<nation>/list.xml`.
Collector vehicles are excluded. Installed EU `2.4.1.5514` yields **11 tanks**,
with Cruiser I as the only British starter. Vehicle and component definitions
supply stock modules, ammunition and crew roles through the existing catalog.
The older decompiled client in `gui/Scaleform/daapi/view/lobby/techtree/`
uses the same regular tree path in `settings.py` and reads its nodes separately
from premium nodes in `techtree_dp.py`.

Each tank has persisted research, researched stock modules, an installed loadout,
a fully seated basic crew specialized for that tank, and full-capacity default
ammunition. Basic crew use 100,000 skill-efficiency XP without perks or bonus XP.
These fixture grants incur no purchase or research charges.

Fresh balances match the schema and `Balances::default()`: 100,000,000 credits,
1,000,000 gold, 10,000 bonds, 1,000,000 free XP, 100 garage slots and 100 barracks
berths. Unmounted stock and other progression start empty; the normal account
projection supplies defaults for unsupported features.

`src/accounts/infrastructure/client/demo_settings.json` is a demo UI baseline
captured from the working Player_2 account on 2026-11-13 with those installed
resources. Selected vehicle is normalized to inventory ID 1 and integration-only
key 500 is omitted. These are working demo preferences, not universal factory
defaults for every client version. The older decompiled
`client/account_helpers/settings_core/IntUserSettings.py` corroborates the integer
dictionary shape, without establishing current values.

## Repeats and existing players

Both players are seeded in one transaction; any error rolls back both. Account
row locks serialize competing writes. An existing email with a different display
name or password is rejected without reassigning the account.

Existing credentials, balances, stock, settings, other vehicles, installed modules,
crew and nonempty ammunition choices are preserved. Missing settings, tier I tech tree
ownership, research and crew seats are filled. Absent or completely empty shells
receive the installed gun's defaults. Capacity increases only when necessary.
Changes advance account revisions; an unchanged repeat does not duplicate tanks
or crew or advance revisions. The existing insert-only `PostgresAccounts::seed`
test fixture remains separate.

Run with demo players logged out; reconnect to refresh cached session snapshots.
This prepares garage readiness. Battle simulation and matchmaking remain separate
server work.

## Verification

The integration test creates a temporary database, runs the real seed binary,
checks both logins, all 11 tech tree tanks, modules, crew, shells, settings and cache
encoding, then verifies repeats, preservation of edits, atomic rollback and
repairing missing ownership:

```sh
WG_TEST_DATABASE_URL='postgresql://wot_server:local-development@127.0.0.1:54329/wot_server' \
WG_RESOURCE_DIR='/path/to/resources' \
cargo test -p wot-seed --test seeding -- --ignored --nocapture
```

The test user needs CREATE DATABASE permission. The temporary database is removed
after verification, including reported verification errors.
