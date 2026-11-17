# Tools, evidence, and verification

[Map home](README.md) · [Source inventory](source-inventory.md) · [Dispatch snapshot](dispatch-snapshot.md)

## Checks performed

On 2026-11-11, in the inspected toolkit checkout:

| Check | Result | What it establishes |
| --- | --- | --- |
| `git status --short`, `git rev-parse HEAD` | Clean source tree, commit `357a7f556a197079c21a371be5718e5211b41eda` | Reproducible source baseline |
| `cargo test -p wg-toolkit -p wg-toolkit-wot --quiet` | Library: 60 passed. Emulator: 77 passed, 1 ignored. Doctests: 1 passed, 3 ignored. No failures. | Selected packages compile and their enabled tests pass locally |
| `dump_entity` example with supplied resources | Account model/dispatch loaded successfully; snapshot includes additional entities | These resources are readable by the loader and dispatch builder |

The test run emitted a dead-code warning for `BaseHandler.avup_samples`.
The CLI package and its filesystem mounts were not built/tested in that run.
No Windows client connection, official-server proxy capture, multiplayer session,
full map/vehicle matrix, executable disassembly or external drive fixtures were
validated during this audit. Tests that return early without fixture environment
variables can appear as passed; the count does not establish capture fidelity.

## Reproduce local inspection

Run from the toolkit checkout:

```sh
# From the wot-server repository root:
export WOT_GAME_DIR="$(cargo game-client path)"
cd docs/references/wg-toolkit-rs
cargo test --locked -p wg-toolkit -p wg-toolkit-wot
cargo run --locked -p wg-toolkit-wot --example dump_entity -- "$WOT_GAME_DIR"
cargo run --locked -p wg-toolkit-wot --example dump_entity -- "$WOT_GAME_DIR" Account
cargo run --locked -p wg-toolkit-wot --example dump_entity -- "$WOT_GAME_DIR" Avatar
cargo run --locked -p wg-toolkit-wot --example dump_entity -- "$WOT_GAME_DIR" Vehicle
```

[dump_entity.rs](../references/wg-toolkit-rs/wg-toolkit-wot/examples/dump_entity.rs)
lists all modeled entity/component IDs or one type's properties and method
signatures. It does not itself bind all emulator schemas or establish server
behavior. For a local emulator scenario:

```sh
cargo run --locked -p wg-toolkit-wot -- --game-dir "$WOT_GAME_DIR" server --help
```

Use `server` with the documented [scenario flags](gameplay.md#current-configuration-surface)
once the client is configured to contact it. This guide does not modify the
client's login configuration or start a listener. The current CLI's top-level
help contains its historical Packed XML recipe; back up client configuration
and verify build-specific settings before applying such changes.

For resource inspection, build the tooling without native filesystem mounts:

```sh
cargo run --locked -p wg-toolkit-cli --no-default-features -- --help
cargo run --locked -p wg-toolkit-cli --no-default-features -- res "$WOT_GAME_DIR/res" ls scripts
```

The inspected CLI's **default features include both `dokan` and `fuse`**; target
platform guards select the relevant mount implementation. Use
`--no-default-features` for inspection that does not need native mount libraries.
Do not assume a prior installation workaround describes this revision's defaults.

## Capture and offline replay

[app/proxy/capture.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/proxy/capture.rs)
records gzip-compressed `WGTKCAP` streams: version, session start time and
per-packet timestamps, direction, peer/real addresses, decryptability flag and
packet bytes. Both base directions are recorded after decryption, except the
initial undecryptable login packet; LoginApp RSA traffic is not captured here.
A truncated capture can be read to its last complete record.

[proxy/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/proxy/mod.rs) maintains
entity selections, dispatch/property state and NRL context.
[proxy/python.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/proxy/python.rs)
interprets account commands, tokens and completed resource streams.
[replay.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/replay.rs) uses the live
handler offline and writes `replay-trace.jsonl`.

```sh
cargo run --locked -p wg-toolkit-wot -- --game-dir "$WOT_GAME_DIR" replay /path/to/session.wgcap --round-trip
```

`--round-trip` re-encodes decoded elements and reports matches, differences and
missing encoders. It proves byte agreement for exercised cases only, not semantic
correctness of a whole session. Handler elapsed timings during replay are not
original network timings; capture timestamps remain available.

[replay_bundle.rs](../references/wg-toolkit-rs/wg-toolkit-wot/examples/replay_bundle.rs)
accepts `<game-dir> <entity-name> <hex-packet>` for focused decoding of one raw
packet. It is not a replacement for a whole capture when decoding needs prior
entity, property-container or NRL state.

The [proxy safety notes](../references/wg-toolkit-rs/doc/PROXY.md) explain why raw
forwarding must remain independent of decoding failures and why handoff address
rewriting is sensitive to framing errors. Use offline replay first for codec
changes. Preserve captures as local evidence, not as public documentation
attachments: they contain decrypted session data.

## Physics comparison

[drive_replay.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/drive_replay.rs)
uses external `*.csv` drive records and resources selected by `WGT_DRIVES_PATH`
and `WGT_RES_PATH`. Records include timestamped movement flags and poses, vehicle
descriptors, optional setup corrections, other-vehicle tracks and per-window
error thresholds. It compares simulation windows started from captured state.
The fixtures are not in this checkout. Without both environment variables, the
fixture-driven test returns without comparing a drive.

```sh
WGT_RES_PATH="$WOT_GAME_DIR/res" WGT_DRIVES_PATH=/path/to/drives \
  cargo test --locked -p wg-toolkit-wot drive_replay -- --nocapture
```

Do not use a generic unit-test pass as evidence that captured-drive thresholds
have been met. Record fixture identity and the measured errors when running it.

## Reconciling old notes

| Older claim or path | Current source evidence / status |
| --- | --- |
| CLI bootstrap/generated per-game Rust code | Replaced by `script`, `dispatch`, and typed `entity` schemas; old paths absent |
| Cell/AoI entirely unimplemented | Base app implements spatial entities/witnesses; a separate direct CellApp remains absent |
| `LeaveAoi` is only an undecoded placeholder | Current client elements have a structured `LeaveAoi` codec |
| `doCmdInt3` cannot be decoded | Current resource dump gives its full signature; server has concrete handlers; old failing session not replayed here |
| Account debugger fixed-size signature missing 8 bytes | Current runtime table is `Int64, Int32, Int64`, totaling 20 bytes; historic failure is not proof of a current bug |
| Pickle uses a serde-pickle fork with missing globals | Current code has its own protocols 0–2 codec and represents object globals/state as data; old exact failures need replay before classifying |
| Dynamic component IDs are unknown | Dispatch now distinguishes entity type slots from NRL IDs filtered by client-class availability |
| Vehicle/module catalog lookup still missing | Current `item_defs`, `items` and `item_prices` provide loaders and descriptors |

These are source-level reconciliations, not proof that every old capture now
passes. Recover the original evidence for cases where runtime behavior matters.

## Recording future discoveries

For each finding, record client/resource build, toolkit/server revision, exact
source symbols, a reproducible scenario or capture reference, observed bytes or
behavior, and confidence. Avoid marking an inferred field meaning as confirmed
because its current name sounds plausible. Keep research artifacts available
outside prose links if they are required to reproduce a conclusion.
