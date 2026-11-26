# wot-server

A **work-in-progress server implementation for the latest World of Tanks EU
client**. Game updates can require a new server release.

## Play on the public server

These steps assume you have the latest World of Tanks EU client installed on Windows.

**WoT Server** is online at `82.66.254.119:20016` and automatically follows
the project's latest published server release.

1. Go to [wot.gabrielhamel.fr](https://wot.gabrielhamel.fr/) and choose
   **Create a player account**.
2. Close World of Tanks. In File Explorer, open `C:\Games\World_of_Tanks_EU\res`.
   If you installed the game somewhere else, open the `res` folder under that
   installation. You can optionally keep a copy of `scripts_config.xml`.
3. Download [scripts_config.xml](https://raw.githubusercontent.com/gabrielhamel/wot-server/main/docs/public-server/scripts_config.xml)
   and [loginapp_wot_server.pubkey](https://raw.githubusercontent.com/gabrielhamel/wot-server/main/docs/public-server/loginapp_wot_server.pubkey).
   Put both files in the `res` folder and replace `scripts_config.xml` when prompted.
4. Launch World of Tanks. **WoT Server** will appear in the server list on the
   sign-in screen. If Game Center signs you in automatically, choose **Switch
   Account** to reach that screen. Select **WoT Server** and sign in with the
   email and password you registered with.

The supplied server list keeps the official EU entries. Game Center updates
and repairs can replace the server list.

## How to run the server in development

Run these commands from the repository root. You’ll need Docker Compose, Rust
with Cargo, Node.js and npm for the [dashboard build](docs/admin.md#enable-the-dashboard),
and access to the latest World of Tanks EU installation.

### 1. Start PostgreSQL

Start the database with the included [Compose file](compose.yaml). The command
waits for PostgreSQL to be ready on `127.0.0.1:54329`.

```sh
docker compose up -d --wait postgres
```

### 2. Set the game installation path

Replace the example path with the directory that contains `WorldOfTanks.exe`.

**Linux (Bash)**

```bash
game_dir='/path/to/World_of_Tanks_EU'
```

**Windows (PowerShell)**

```powershell
$game_dir = 'C:\Games\World_of_Tanks_EU'
```

### 3. Extract the game resources

Run this command in the same terminal. The [extractor](docs/resources.md)
requires a new output directory.

```sh
cargo run -p wot-extract -- --output .local/extracted-client "$game_dir"
```

### 4. Generate a login key pair

Generate a 2048-bit RSA key pair in `.local`. The tool asks before replacing
either key file.

```sh
cargo run -p wot-login-key -- --name login --output .local/
```

### 5. Run the server

```sh
cargo run -- --resource-dir .local/extracted-client --database-url 'postgresql://wot_server:local-development@127.0.0.1:54329/wot_server' --private-key .local/login.pem
```

The server applies database migrations automatically. Leave it running and
use a second terminal for the client setup commands if needed.

### 6. Create the first account in the dashboard

Open the [local dashboard](http://127.0.0.1:8080/dashboard/) and complete the
first-administrator form with an email address, a display name, and a password.
Use the same email and password to sign in to the game.

### 7. Connect the game client

Close World of Tanks. From the repository root, add **WoT Local** to the game’s
server list and copy the public key into the game’s `res` folder. If you opened
a new terminal, set `game_dir` again as shown in step 2.

**Linux (Bash)**

```bash
cargo run -p wot-server-list -- insert --game-dir "$game_dir" \
  --address 127.0.0.1:20016 \
  --name 'WoT Local' --short-name WOTLOCAL \
  --key-name loginapp_wot_server_250.pubkey \
  --periphery-id 250
cp .local/login.pubkey "$game_dir/res/loginapp_wot_server_250.pubkey"
```

**Windows (PowerShell)**

```powershell
cargo run -p wot-server-list -- insert --game-dir "$game_dir" --address 127.0.0.1:20016 --name 'WoT Local' --short-name WOTLOCAL --key-name loginapp_wot_server_250.pubkey --periphery-id 250
Copy-Item -LiteralPath '.local\login.pubkey' -Destination "$game_dir\res\loginapp_wot_server_250.pubkey"
```

If the game runs on another computer, use the server’s reachable IPv4 address
and UDP port in `--address`. The public key filename must match `--key-name`,
and the periphery ID must match the running server’s setting. See the
[server guide](docs/server.md#connecting-the-game) to list existing IDs or
delete an entry.

Launch the game. **WoT Local** will appear in the server list on the sign-in
screen. Choose **Switch Account** if Game Center signs you in automatically,
then select **WoT Local** and sign in with your local account.

See the [server guide](docs/server.md) for more configuration options or the
[Docker quickstart](docs/docker-quickstart.md) to run the server in a container.

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
- Drive with terrain-dependent grip and slopes, push other tanks, and collide
  with or destroy supported scenery. Aim, select shells, and reload single-shot,
  magazine-fed, or autoreloading guns.
- Shells take time to reach their targets. Battles resolve penetration, screens,
  ricochets, artillery trajectories and splash, and damage to modeled modules
  and crew. The client receives tracers, impact effects, and directional
  indicators for blocked shots. Vehicle impacts can leave persistent marks.
- Finish a round by team elimination, departures, or a time-limit draw. Players
  return to the garage when they leave or the round closes.
  [Battle mechanics](docs/battle-mechanics.md)

</details>

<details>
<summary>Player and admin dashboard</summary>

- Register, check your garage and balances, and shop in the browser.
- Set up the first administrator, create player accounts, manage roles, and
  inspect players' garages and balances.
- Grant tanks, credits, gold, bonds, free XP, Experimental Equipment components,
  crew books, Improved, Bounty and Experimental Equipment, 2D and 3D styles,
  3D attachments, stat trackers, crew skins, and special recruits.
- Launch Training battles directly from players' garages. Choose the map and
  teams, assign owned tanks or Observer seats where available, and add
  stationary dummy tanks with custom equipment, consumables, and crew perks.
  [Dashboard guide](docs/admin.md)

![Admin dashboard showing a player's balances, resource grants and garage](docs/images/dashboard.png)

_Dashboard preview with sample player data._

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
- **Spotting and radio.** Add vehicle detection, line of sight, concealment,
  and sharing of spotted positions with allies over radio.
  [Visibility and replication](docs/battle-mechanics.md#visibility-and-replication)
- **More combat mechanics.** Complete module damage and crew injury effects,
  implement repair kits, first-aid kits and fire extinguishers, and add ramming
  and fall damage. [Penetration and damage](docs/battle-mechanics.md#penetration-damage-and-feedback)
- **More battle modes and Training Room support.** Add Random Battle
  matchmaking, base capture, Training Room voice chat, and in-battle spectating.
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
