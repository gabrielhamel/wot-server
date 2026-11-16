# wot-server

A **work-in-progress server implementation for World of Tanks EU 2.4.1.5514
and later versions**. Newer client versions may require compatibility updates.

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
[localhost:8080](http://127.0.0.1:8080/) and sign in with your admin token.
See the [dashboard guide](docs/admin.md) for setup.

![Admin dashboard showing a player's balances, resource grants and garage](docs/images/dashboard.png)

*Dashboard preview with sample player data.*

## Documentation

For development and agent research, [register or install the compiled game
client](docs/game-client.md), then run `python3 tools/game-client.py status`.

See the [documentation hub](docs/README.md) for setup, protocol research and
implementation details.

## License

[MIT](LICENSE), with [third-party notices](THIRD_PARTY_NOTICES.md) for adapted
WG Toolkit code. Research submodules and game resources retain their respective
licenses and ownership.
