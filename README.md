# wot-server

A **work-in-progress server implementation for World of Tanks EU 2.4.1.5514
and later versions**. Newer client versions may require compatibility updates.

## Play on the public server

**WoT Server** is online at `82.66.254.119:20016` and automatically follows
the project's latest published server release. The current release uses
the World of Tanks EU client build **2.4.1.5514**.

1. DM **`gabouchet`** on Discord to request a server account.
2. Close World of Tanks. In your installation's `res` folder, back up your
   existing `scripts_config.xml`.
3. Download [scripts_config.xml](docs/public-server/scripts_config.xml) and
   [loginapp_wot_server.pubkey](docs/public-server/loginapp_wot_server.pubkey).
   On GitHub, use **Download raw file** to save each file with its original
   name. Copy both into `res` and replace `scripts_config.xml` when prompted.
4. Launch World of Tanks, select **WoT Server**, and sign in with the account
   you received on Discord. If Game Center signs you in automatically, choose
   **Switch Account** first.

The supplied server list keeps the five official EU entries for this build.
To undo the change, restore your backup. Game Center updates and repairs can
replace the server list.

## Getting started

The server needs a **PostgreSQL database**. Start one with the included
[Compose file](compose.yaml):

```sh
docker compose up -d --wait postgres
```

With the default Compose settings, use this connection string in `server.json`:

```json
{
  "database_url": "postgresql://wot_server:local-development@127.0.0.1:54329/wot_server"
}
```

The server applies database migrations automatically.

See the [Docker quickstart](docs/docker-quickstart.md) to run the server and add
it to your game's server list, or the [server guide](docs/server.md) for a native
installation and configuration.

## Dashboard

The web dashboard lets you create player accounts, view their garages, and grant
tanks, credits, gold, bonds and free XP. Enable it in `server.json`, then open
[localhost:8080](http://127.0.0.1:8080/) and follow the first-administrator setup. After setup, players can register and sign in to view their own garage.
See the [dashboard guide](docs/admin.md) for setup.

![Admin dashboard showing a player's balances, resource grants and garage](docs/images/dashboard.png)

*Dashboard preview with sample player data.*

## Documentation

For development and agent research, [register or install the compiled game
client](docs/game-client.md), then run `cargo game-client status`.

See the [documentation hub](docs/README.md) for setup, protocol research and
implementation details.

## License

[MIT](LICENSE), with [third-party notices](THIRD_PARTY_NOTICES.md) for adapted
WG Toolkit code. Research submodules and game resources retain their respective
licenses and ownership.
