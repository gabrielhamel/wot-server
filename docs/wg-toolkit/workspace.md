# Workspace and dependency map

[Map home](README.md) · [Protocol](protocol.md) · [Source inventory](source-inventory.md)

## Packages and entry points

| Package | Artifact | Responsibility | Relevance to wot-server |
| --- | --- | --- | --- |
| [wg-toolkit](../references/wg-toolkit-rs/wg-toolkit/Cargo.toml) | library imported as `wgtk` | Engine protocol, applications, script model, codecs, resource and geometry readers | Protocol and format reference |
| [wg-toolkit-wot](../references/wg-toolkit-rs/wg-toolkit-wot/Cargo.toml) | `wot` binary | Emulator, proxy, captures, WoT schemas, account and battle behavior | Behavioral reference; its private modules cannot be consumed as a public library unchanged |
| [wg-toolkit-cli](../references/wg-toolkit-rs/wg-toolkit-cli/Cargo.toml) | `wgtk` binary | Resource extraction, Packed XML editing, space and Havok inspection, optional mounts | Development tooling; not a runtime server dependency |

Both binaries depend on `wg-toolkit`; the library does not depend on either binary.
`wg-toolkit` exposes `net`, `app`, `script`, `pickle`, `pxml`, `res`, `space`,
`havok`, `model`, and `util` from its [library root](../references/wg-toolkit-rs/wg-toolkit/src/lib.rs).
There are no separate server/client library crates or generated per-game crates.

```text
wg-toolkit-cli ───► wg-toolkit ◄─── wg-toolkit-wot
 resource tools      engine          server / proxy / replay
                       ▲                  │
                       └──────────────────┘
                         runtime calls
```

The arrows show code dependencies, not a deployment topology.

## Runtime paths

The [wot entry point](../references/wg-toolkit-rs/wg-toolkit-wot/src/main.rs) offers:

| Command | Execution path |
| --- | --- |
| `server` | [cluster/server.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/cluster/server.rs): login and base threads, linked in process |
| `proxy` | [cluster/proxy.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/cluster/proxy.rs): login and base proxies |
| `app login` | Login server or login proxy, depending on `--real-login-app`; accepts links from base processes |
| `app base` | Emulator base process connecting to the login link |
| `app proxy` | Base proxy process connecting to the login proxy's link |
| `replay` | [replay.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/replay.rs): offline captured-packet decoding, optionally checking encode/decode round trips |

`--game-dir` or `WOT_GAME_DIR` supplies resources. Schemas are bound against the
loaded script at startup. `server` uses two threads, not separate LoginApp,
BaseApp, CellApp, and database deployments.

## Responsibility map

| Area | Owning sources | What to preserve |
| --- | --- | --- |
| Transport | [net/](../references/wg-toolkit-rs/wg-toolkit/src/net/) | Packet framing, sequence arithmetic, reliability, channel and bundle handling |
| Engine application protocol | [app/](../references/wg-toolkit-rs/wg-toolkit/src/app/) | Login, base lifecycle, typed entities, spaces, witnesses, NRL, outboxes |
| Script compatibility | [script/](../references/wg-toolkit-rs/wg-toolkit/src/script/) and [dispatch.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/dispatch.rs) | Resource-derived types and numeric dispatch tables |
| WoT wire schemas | [wot/schema.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/schema.rs) | Typed subset the emulator uses, checked against loaded definitions |
| Account/session orchestration | [base/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs), [account.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/account.rs), [shop.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/shop.rs) | Required bootstrap payloads, command correlation, cache revisions, handoffs |
| Battle orchestration | [battle.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/battle.rs) | Input routing, vehicle creation, replicated state and battle events |
| Simulation | [physics/](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/), [gun.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/gun.rs), [armor.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/armor.rs), [bot.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/bot.rs) | Existing algorithms and fixtures, while documenting approximation limits |
| Game-data ingestion | [wot/](https://github.com/theorzr/wg-toolkit-rs/tree/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/), [res/](../references/wg-toolkit-rs/wg-toolkit/src/res/), [space/](../references/wg-toolkit-rs/wg-toolkit/src/space/), [havok/](../references/wg-toolkit-rs/wg-toolkit/src/havok/) | Definition resolution, geometry, collisions, destructible identities |
| Observation | [proxy/](../references/wg-toolkit-rs/wg-toolkit-wot/src/proxy/), [capture.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/proxy/capture.rs) | Reproducible evidence and offline regression inputs |

## Internal cluster link

[link.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/link.rs) supplies local and TCP
link machinery and messages such as `Hello`, `BaseStatus`, `ClientHost`,
`ClientHosted`, `BattlePlace`, `BattleHost`, and `BaseStop`.
[login/link.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/link.rs) tracks
linked base apps and outstanding placement requests. A base app acknowledges
that it can host a client before login returns that base's address to the client.

The link's comments describe an optional manager and placement topology. There
is no manager command/module in this workspace. Do not mistake that design
narrative or the existence of battle-placement message types for an implemented
distributed matchmaker. These are the emulator's own interprocess contracts;
client compatibility does not require reproducing official internal BigWorld
cluster protocols.

## Reuse choices

The engine library is MIT licensed ([license](../references/wg-toolkit-rs/LICENSE)).
A local path dependency is a practical first integration, but it ties builds to
the sibling checkout; pin and record the eventual distribution strategy. Preserve
license notices when copying code. WoT-specific code is currently inside a binary:
extract a supported library or adapt selected responsibilities into wot-server
with explicit ownership rather than reaching through private implementation.

Do not carry the prototype's `base/mod.rs` orchestration wholesale into a single
wot-server context. Use the [proposed responsibility boundaries](reproduction-plan.md#proposed-bounded-contexts)
and keep wire types and resource parsers at the infrastructure boundary.
