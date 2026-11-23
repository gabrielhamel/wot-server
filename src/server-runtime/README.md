# Server runtime

The `wot-server-runtime` library owns process startup, dependency wiring,
and graceful shutdown. Its public `run` entry point is called by the
`wot-server` binary. The context libraries own game behavior and persistence
rules. The runtime connects their published contracts to the process adapters.

| Source | Responsibility |
| --- | --- |
| [Composition](src/lib.rs) | Load startup settings, construct shared dependencies, start the game gateway and web listener, and coordinate shutdown. |
| [Infrastructure](src/infrastructure) | Command-line configuration, PostgreSQL setup, login key storage, logging, OTLP, static asset serving, and the [web build and test workspace](src/infrastructure/web). Cargo builds and embeds the browser bundle as part of compiling the server runtime. `--web-ui-dir` can override the bundle with a directory. |
| [Presentation](src/presentation/web) | Browser route composition. The admin and Store pages live in their owning contexts. |

| Term | Meaning |
| --- | --- |
| Working directory | Directory used to resolve relative command-line paths. |
| Composition root | Entry point that constructs the server’s adapters and connects context services. |
| Shared web listener | HTTP listener that serves static pages, admin and Store APIs, and browser telemetry. |
