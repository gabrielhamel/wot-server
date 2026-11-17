# Game client tool

This standalone CLI locates the compiled game client used for research and
validation. Run `cargo game-client status --json` or
`cargo game-client --help` from the workspace.

It resolves the checkout's selected client, validates its layout, and hashes
the executable locally. Install and update operations call `wg-launcher` as a
separate process. The tool has no dependency on the server.

See the [game client setup guide](../../docs/game-client.md) for registration,
selection precedence, and launcher setup.
