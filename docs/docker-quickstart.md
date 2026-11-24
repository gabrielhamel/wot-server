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
same network. Set the PostgreSQL connection URL for the container. This example
uses the Compose database's default credentials.

```sh
export WOT_DATABASE_URL='postgresql://wot_server:local-development@postgres:5432/wot_server'
```

In the launch command below, replace `192.168.1.10` with the Docker host's
reachable IPv4 address. Clients use that address to connect after login. If
clients run outside your LAN, use the host's public IPv4 address and forward
the game ports listed below.

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
  -e WOT_DATABASE_URL \
  --mount type=volume,src=wot-server-state,dst=/state \
  "$IMAGE" --resource-dir resources --private-key /state/login.pem \
  --advertised-base 192.168.1.10:20017
```

The server applies database migrations and generates its login key on first
start. Open [the dashboard](http://127.0.0.1:8080/dashboard/), create the first
administrator, then sign in with the administrator email and password. Use
**Player accounts → Create** to add another account. Use an email-shaped login
such as `player@example.com` for the game client. New accounts receive tier I
tanks, crew and ammunition. Startup does not create demo accounts automatically.

Recreate the container with the new arguments when changing settings. Keep the
`wot-server-state` volume to reuse the login key. Stop the game server with
`docker stop wot-server` and PostgreSQL with `docker compose down`.

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
ports. The `-p` options above publish them. To disable the web listener,
omit the HTTP port mapping and pass `--disable-admin --disable-store`.
For [Store access from another computer](store.md#configuration), configure a
reachable origin. If serving the Store directly on a LAN, publish port 8080 on
the Docker host's LAN address.

## Add the server to the game's server list

Close the game first. Export the server's public key from the running
container. If the game is on another computer, transfer the public key to
the computer where you will run the editor. Keep the private `login.pem`
on the server.

```sh
docker cp wot-server:/state/login.pubkey ./loginapp_wot_server.pubkey
```

Run the Rust tool from the repository root on a machine that can access the
game installation. Replace `192.168.1.10:20016` with the Docker host's
reachable LoginApp address. Use `127.0.0.1:20016` if the game runs on the host.

```sh
cargo run --locked -p wot-server-list -- insert --game-dir '/path/to/World_of_Tanks_EU' \
  --address 192.168.1.10:20016 --key-name loginapp_wot_server_250.pubkey
cp ./loginapp_wot_server.pubkey '/path/to/World_of_Tanks_EU/res/loginapp_wot_server_250.pubkey'
```

The tool keeps the existing entries in the Packed XML list. If another
server uses periphery ID 250, choose a free ID with `--periphery-id` and set
the same value on the game server for Training Room invitations.

Start the game, select **WoT Local**, choose **Switch Account** if Game Center
tries its official account, and log in with the player you created in the admin
dashboard. If the server's login key changes, manually replace the public key
in the game folder. Game Center repairs and updates can replace the server list.
See the [server guide](server.md#connecting-the-game) for more options.

## Build from source

To build a local Docker image, extract resources from the matching game client
into a new `resources/` directory, then build the image:

```sh
cargo run --locked --release -p wot-extract -- /path/to/game --output resources
docker build -t wot-server:local .
export IMAGE=wot-server:local
```

Then follow the Docker configuration and startup steps above, skipping registry
login/pull. For native Rust builds, resource extraction and the
offline executable mapper, see the [documentation hub](README.md).
Research submodules are optional for server builds; see
[research references](references/README.md).

