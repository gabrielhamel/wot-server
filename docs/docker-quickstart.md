# Docker quickstart and client setup

Run the commands below from the repository root.

## Start with Docker

You need Docker with Compose and a matching World of Tanks client to connect.
The published Linux **x86-64** image includes the server, dashboard and extracted
client resources; you do not need Rust or a game installation on the server host.
Use an image built for your client's game version.

Run the following commands from this repository's root in a POSIX shell (Linux,
macOS or WSL). Set `IMAGE` to a published tag from the
[container workflow](https://github.com/gabrielhamel/wot-server/actions/workflows/container.yml) run summary:

```sh
export IMAGE=ghcr.io/gabrielhamel/wot-server:REPLACE_WITH_PUBLISHED_TAG
docker pull "$IMAGE"
docker compose up -d --wait postgres
```

The repository's `compose.yaml` starts **PostgreSQL only**, on the Docker
network `wot-server_default`. The commands below start the game server on that
same network. This example uses the Compose database's default credentials;
if you override them, update `database_url` accordingly.

### Configure the server

Generate an admin token with `openssl rand -hex 32`. Save the following as
`server.json` in the repository root, replacing the token and `192.168.1.10` with
the server host's IPv4 address reachable by game clients. For a game running on
the same computer, use `127.0.0.1` instead.

```json
{
  "database_url": "postgresql://wot_server:local-development@postgres:5432/wot_server",
  "resource_dir": "resources",
  "login_bind": "0.0.0.0:20016",
  "base_bind": "0.0.0.0:20017",
  "advertised_base": "192.168.1.10:20017",
  "private_key": "/state/login.pem",
  "admin": {
    "bind": "0.0.0.0:8080",
    "token": "REPLACE_WITH_YOUR_GENERATED_TOKEN",
    "dashboard_dir": "dashboard"
  }
}
```

The advertised address must be the Docker **host's reachable address**, because
clients use it to connect after login. For clients outside your LAN, use the
host's public IPv4 address and forward the game ports listed below.

Prepare the login key directory. The image runs as UID/GID `10001`, so
initialize the volume's ownership once:

```sh
docker volume create wot-server-state
docker run --rm --user 0 --entrypoint /bin/sh \
  --mount type=volume,src=wot-server-state,dst=/state \
  "$IMAGE" -c 'chown 10001:10001 /state'
```

### Run and create a player

```sh
docker run -d --name wot-server --restart unless-stopped \
  --network wot-server_default \
  -p 20016:20016/udp \
  -p 20017:20017/udp \
  -p 20017:20017/tcp \
  -p 127.0.0.1:8080:8080/tcp \
  --mount "type=bind,src=$PWD/server.json,dst=/opt/wot-server/server.json,readonly" \
  --mount type=volume,src=wot-server-state,dst=/state \
  "$IMAGE"
```

The server applies database migrations and generates its login key on first
start. Open [the dashboard](http://127.0.0.1:8080/), sign in with your admin token,
and use **Player accounts → Create** to add an account. Use an email-shaped login
such as `player@example.com` for the game client. New accounts receive tier I
tanks, crew and ammunition. Startup does not create demo accounts automatically.

Edit `server.json` and run `docker restart wot-server` to apply configuration
changes. Stop the game server with `docker stop wot-server`; stop PostgreSQL with
`docker compose down`.

### Ports

These are the host ports published by the commands above:

| Port | Transport | Purpose | Reachability |
| --- | --- | --- | --- |
| `20016` | UDP | LoginApp: authentication and game-session handoff | Game clients |
| `20017` | UDP | BaseApp: game session and garage protocol | Game clients |
| `20017` | TCP | Native XMPP chat, contacts and private messages | Game clients |
| `8080` | TCP / HTTP | Admin dashboard and API | Host loopback only |
| `54329` | TCP / PostgreSQL | Compose database, mapped to container port `5432` | Host loopback only |

Allow/forward **all three game mappings**, including both UDP and TCP on `20017`,
through the host firewall and router when needed. Game clients do not need the
admin or database ports. Docker's `EXPOSE` declarations alone do not publish
ports; the `-p` options above do. The admin listener is optional: remove the
`admin` object and its port mapping to disable it.

## Add the server to the game's server list

Close the game first. Its `res/scripts_config.xml` server list uses **BigWorld
Packed XML**. Use the Toolkit's `wgtk pxml` editor to append an entry while
preserving the official servers. This editor needs Rust on the machine where you
run it; the server container does not modify game files.

From the repository root, build the editor:

```sh
git submodule update --init docs/references/wg-toolkit-rs
cargo build --release \
  --manifest-path docs/references/wg-toolkit-rs/Cargo.toml \
  -p wg-toolkit-cli --no-default-features
```

Set the game directory and copy the server's **public** key from the running
container. If the game is on another computer, transfer the exported file to
that computer's game `res/` directory. Keep the private `login.pem` on the server.

```sh
export WOT_GAME_DIR='/path/to/World_of_Tanks'
docker cp wot-server:/state/login.pubkey ./loginapp_wot_server.pubkey
cp ./loginapp_wot_server.pubkey "$WOT_GAME_DIR/res/loginapp_wot_server.pubkey"
cp -n "$WOT_GAME_DIR/res/scripts_config.xml" "$WOT_GAME_DIR/res/scripts_config.xml.wot-server.bak"
```

Inspect the existing entries and choose an unused `periphery_id`. The example
uses `250`, which was free in the validated EU installation; check your own file.

```sh
./docs/references/wg-toolkit-rs/target/release/wgtk pxml \
  -f "$WOT_GAME_DIR/res/scripts_config.xml"
```

Append the entry below **once**. Replace both `127.0.0.1:20016` values with
`SERVER_IPV4:20016` if the game runs on another computer. The single quotes around
the filter are required to preserve its `$tmp` variables. Write to a temporary
file first so an editing failure leaves the original intact.

```sh
./docs/references/wg-toolkit-rs/target/release/wgtk pxml \
  -f "$WOT_GAME_DIR/res/scripts_config.xml" --raw \
  '$tmp=login/host;$tmp/name=str(WoT Server);$tmp/short_name=str(WOTSERVER);$tmp/url=str(127.0.0.1:20016);$tmp/url_token=str(127.0.0.1:20016);$tmp/public_key_path=str(loginapp_wot_server.pubkey);$tmp/periphery_id=int(250);login/host[^]=$tmp' \
  > "$WOT_GAME_DIR/res/scripts_config.xml.wot-server.tmp" && \
  mv "$WOT_GAME_DIR/res/scripts_config.xml.wot-server.tmp" "$WOT_GAME_DIR/res/scripts_config.xml"
```

Start the game, select **WoT Server**, choose **Switch Account** if Game Center
tries its official account, and log in with the player you created in the admin
dashboard. Official Wargaming credentials and launcher tokens are not local
server credentials.

If the server's login key changes, copy its new public key into every client.
Game Center repair/update can replace the server list. To undo the list change,
close the game and restore `scripts_config.xml.wot-server.bak` over
`res/scripts_config.xml`. See [client validation](client-validation.md) for
the tested installation and [server configuration](server.md) for details.

## Build from source

To build a local Docker image, extract resources from the matching game client
into a new `resources/` directory, then build the image:

```sh
cargo run --locked --release -p wot-extract -- /path/to/game --output resources
docker build -t wot-server:local .
export IMAGE=wot-server:local
```

Then follow the Docker configuration and startup steps above, skipping registry
login/pull. For native Rust builds, resource extraction, demo seeding and the
offline executable mapper, see the [documentation hub](README.md).
Research submodules are optional for server builds; see
[research references](references/README.md).

