# wot-server

A **work-in-progress server implementation for the latest World of Tanks EU
client**. Game updates can require a new server release.

## Play on the public server

**WoT Server** is online at `82.66.254.119:20016` and automatically follows
the project's latest published server release.

1. Go to [wot.gabrielhamel.fr](https://wot.gabrielhamel.fr/) and choose
   **Create a player account**.
2. Close World of Tanks. In your installation's `res` folder, back up your
   existing `scripts_config.xml`.
3. Download [scripts_config.xml](docs/public-server/scripts_config.xml) and
   [loginapp_wot_server.pubkey](docs/public-server/loginapp_wot_server.pubkey).
   On GitHub, use **Download raw file** to save each file with its original
   name. Copy both into `res` and replace `scripts_config.xml` when prompted.
4. Launch World of Tanks, select **WoT Server**, and sign in with the email
   and password you registered with. If Game Center signs you in automatically,
   choose **Switch Account** first.

The supplied server list includes the official EU entries.
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

## Features

<details open>
<summary><strong>Implemented</strong></summary>

<details>
<summary>Accounts and garage</summary>

- Sign in from the game client and sync your garage from PostgreSQL. New player
  accounts start with regular Tier I tanks, 500 gold, and 200,000 credits.
  [Account setup](docs/admin.md#player-registration-and-sign-in)
- Research, buy, and sell Tech Tree tanks, buy garage slots, and research, buy,
  and install vehicle modules. [Tanks](docs/vehicles.md) · [Modules](docs/modules.md)
- Configure ammunition, consumables, and crew and equipment directives. Save
  consumable auto-resupply preferences.
  [Ammunition](docs/ammunition.md) · [Consumables](docs/consumables.md) · [Directives](docs/directives.md)
- Buy, mount, and demount equipment. Buy eligible Improved Equipment for
  bonds, upgrade Bounty and Experimental Equipment, and disassemble
  Experimental Equipment for components.
  [Equipment](docs/equipment.md)
- Recruit, transfer, retrain, dismiss, and restore crew. Manage barracks, perks,
  crew books, and crew skins, and toggle Accelerate Crew Training on eligible
  Elite tanks. [Crew](docs/crew.md)
- Apply permanent styles, seasonal camouflage, paints, inscriptions, decals,
  effects, 3D attachments, and stat trackers. [Exterior](docs/camouflage.md)
- View Depot stock and sell eligible unmounted items. [Depot](docs/depot.md)

</details>

<details>
<summary>Store and Premium Account</summary>

- Shop from the in-game garage or the player dashboard. Buy vehicles for gold
  or bonds, Collectors’ vehicles for credits, and eligible equipment, consumables,
  directives, 2D and 3D styles, and crew skins. The Improved Equipment section
  includes eligible bond-priced items hidden in the upstream shop.
- Buy and extend WoT Premium Account with gold, and see its status and expiration
  in the garage. [Store](docs/store.md)

</details>

<details>
<summary>Contacts and private chat</summary>

- Search for players, send and respond to friend requests, and see friends’
  online status.
- Send private messages through the native messenger, with offline delivery and
  saved conversation history. [Social](docs/social.md)

</details>

<details>
<summary>Training Rooms and battles</summary>

- Browse public and invitation-only Training Rooms, invite players, and use room
  text chat. Hosts can set the map and time limit, organize teams, and start
  a shared battle once both teams are ready.
  [Training Rooms](docs/wg-toolkit/session-flows.md#training-room-visibility)
- Drive forward and in reverse, aim and fire using the equipped vehicle's
  movement and gun settings, and switch shells or reload the magazine. Direct
  hits apply the loaded shell's nominal damage, capped by the target's remaining
  health. Eliminating the opposing team ends the round. Players return to the
  garage when they leave or the round closes.
  [Training battles](docs/wg-toolkit/session-flows.md#starting-a-training-room-battle)

</details>

<details>
<summary>Player and admin dashboard</summary>

- Register a player account, sign in, and view your own tanks and balances.
- Create player accounts, manage roles, and inspect player garages as an
  administrator.
- Grant tanks, credits, gold, bonds, free XP, Experimental Equipment components,
  crew books, Improved Equipment, Bounty and Experimental Equipment, 3D
  attachments, and stat trackers.
- Enable the dashboard in `server.json` and use the first-administrator setup
  to get started. [Dashboard guide](docs/admin.md)

![Admin dashboard showing a player's balances, resource grants and garage](docs/images/dashboard.png)

*Dashboard preview with sample player data.*

</details>

<details>
<summary>Server and development tools</summary>

- Run the server natively or in Docker. PostgreSQL stores player progression
  and the server applies database migrations at startup. [Server guide](docs/server.md)
- Export OpenTelemetry traces. Extract runtime resources from the game client and
  inspect protocol binaries offline.
  [Telemetry](docs/telemetry.md) · [Resource extraction](docs/resources.md) · [Binary mapping](docs/binary-mapping.md)

</details>
</details>

<details>
<summary><strong>Upcoming</strong></summary>

- **Battle results and progression.** Apply battle results and rewards to
  account progression. [Battle results](https://github.com/gabrielhamel/wot-server/issues/28)
- **More Training Room support.** Voice audio and in-battle spectating.
  [Training Room behavior](docs/wg-toolkit/session-flows.md#creating-a-training-room)
- **More Store and exterior support.** Editable and progressive styles, Personal
  Reserves, and Premium Account battle benefits remain open.
  [Store gaps](docs/store-missing-features.md)

</details>

Follow the [project roadmap](https://github.com/users/gabrielhamel/projects/9/views/1)
for milestone and issue updates.

## Documentation

For development and agent research, [register or install the compiled game
client](docs/game-client.md), then run `cargo game-client status`.

See the [documentation hub](docs/README.md) for setup, protocol research and
implementation details.

## License

[MIT](LICENSE), with [third-party notices](THIRD_PARTY_NOTICES.md) for adapted
WG Toolkit code. Research submodules and game resources retain their respective
licenses and ownership.
