# Battle replay research

Use official-client recordings to measure client-visible behavior before
implementing or tuning a battle mechanic. A replay stores timestamped playback
records from one client's perspective, including entity updates and local
playback state. The decompiled client's
[BattleReplay.py](references/wot-src/sources/res/scripts/client/BattleReplay.py)
sets replay state directly for gun marker parameters, control mode and arena
period. A replay provides no guarantee of complete client-to-server commands,
UDP framing, full map visibility or server simulation state. Use a matching
bidirectional capture to establish input messages and wire formats. The pinned
Toolkit's [capture player](references/wg-toolkit-rs/wg-toolkit-wot/src/replay.rs)
and [drive comparison](references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/drive_replay.rs)
read the Toolkit's own proxy format.

## Inspect a capture

The repository includes two raw Random Battle samples under
`docs/replay-samples/`. For additional captures, download an individual replay
from the [Wotlytics replay archive](https://wotlytics.com/replays/) or copy a
recording from the official client's `replays/` directory. Save working
downloads in the checkout's ignored `.local/replays/` directory. Track
provenance, hashes and selected decoded observations in this guide. Raw files
include player metadata and can include chat.

```sh
mkdir -p .local/replays
cargo wot-replay summary docs/replay-samples/wotlytics-1344.wotreplay
cargo wot-replay positions docs/replay-samples/wotlytics-1344.wotreplay > .local/replays/wotlytics-1344.positions.jsonl
```

Use `positions FILE --entity ENTITY_ID` to select a trajectory. The first JSON
block's `vehicles` roster contains the entity IDs as object keys in the
sampled files. To find the recorder, match `playerName` to the roster entry and
use its `avatarSessionID`. The header's `playerID` is an account ID.

The offline [Rust tool](../tools/wot-replay/src/replay.rs) reads length-prefixed
JSON blocks, decodes the Blowfish stream with previous-plaintext XOR chaining
and inflates the zlib stream. It checks advertised lengths, enforces file and
stream size bounds, and reads each record as little-endian
`payload length (u32), type (u32), replay clock in seconds (f32), payload`.
`summary` inventories records by type and payload size. `positions` decodes
type `0x0A` with a 45-byte payload and reports the record header's offset in
the inflated stream. Other payloads, including embedded pickle, stay opaque.

The [Wotto packet research](https://intelliagent.gitlab.io/wotto-wiki/packets/packet0a/)
documented the position layout on an earlier recording and identified a parser
implementation that reads the adjacent error vector as position.
[`wot_replay_parser`](https://github.com/rajesh-rahul/wot-battle-results-parser)
offers a general packet stream and typed events whose layouts depend on client
version. The repository tool verifies the sampled shape and uses the offsets
confirmed below.

## Position record observed in the sample corpus

Offsets are relative to the start of the payload. Keep unknown fields and
entity relationships raw until a controlled capture establishes their meaning.

| Offset | Field | Encoding |
| ---: | --- | --- |
| 0 | Entity ID, which may match a roster entry | `u32` |
| 4 | Secondary ID, zero on most records and equal to the recorder vehicle ID when nonzero in the sample corpus | `u32` |
| 8 | Position X, Y, Z. X/Z are the map plane and Y is vertical on the sampled roster trajectories | Three `f32` values |
| 20 | Position error X, Y, Z, inferred from the small values and the historical [BigWorld movement filter](references/BigWorld-Engine-14.4.1/programming/bigworld/lib/connection/movement_filter.hpp) | Three `f32` values |
| 32 | Yaw, pitch, roll in radians | Three `f32` values |
| 44 | Raw flags observed as 0 or 1 | `u8` |

In replay 1344 an off-roster entity had the recorder vehicle as its secondary
ID and a zero position at clock 0. Use roster IDs and check the secondary ID
when selecting world-space vehicle trajectories. A roster entry can have no
position samples. The recorded interval measures replay cadence, which may
differ from server simulation or wire cadence.

## EU capture evidence from 2026-11-21

All four recordings report `v.2.4.1.1 #993` from XML and `2.4.1.0` from the
executable. The installed EU client reported the same XML label and an
executable SHA-256 of
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`.
The recordings contain version labels, so executable identity is available
only for the local client. The launcher update check could not verify the
installation because it is managed by Wargaming Game Center. [EU release history](https://worldoftanks.eu/en/content/docs/release_notes/)

All four recordings report `battleType: 1` (Random Battle) and
`gameplayID: "ctf"` (Standard). Replays 1344 and 1345 are tracked as
[Prokhorovka](replay-samples/wotlytics-1344.wotreplay) and
[Karelia](replay-samples/wotlytics-1345.wotreplay). Save the other linked files
as `.local/replays/wotlytics-<id>.wotreplay`. All position records in these
captures used the 45-byte layout.

| Replay and download | Battle date | Map and recorder vehicle | Records / position records | Raw file SHA-256 |
| --- | --- | --- | ---: | --- |
| [1343](https://wotlytics.com/replays/cav-mod-71-oooompf-el-halluf-1343/) [Download](https://wotlytics.com/api/v1/replays/1343/download/) | 2026-11-20 | El Halluf, CAV mod. 71 | 133,549 / 63,183 | `6510c4bb459525834190a44941fafa57d1a0be375ce43248b42d04fc1b66869f` |
| [1344](https://wotlytics.com/replays/lt-432-alexgogogo2-prokhorovka-1344/) [Download](https://wotlytics.com/api/v1/replays/1344/download/) | 2026-11-20 | Prokhorovka, LT-432 | 153,939 / 65,105 | `149642699bac91fcaf5b4f848032feb8843b6f0173df754ea3e51c76c8c9fa21` |
| [1345](https://wotlytics.com/replays/sfac-105-mastershadowsoul-karelia-1345/) [Download](https://wotlytics.com/api/v1/replays/1345/download/) | 2026-11-20 | Karelia, SFAC 105 | 139,801 / 69,391 | `7e0cff323bc8313658c3b64a8d42f81861aeeb9f258345d6d6db5d2a113bb948` |
| [1356](https://wotlytics.com/replays/elc-even-90-theblackfoxjumpinthebox-prokhorovka-1356/) [Download](https://wotlytics.com/api/v1/replays/1356/download/) | 2026-11-21 | Prokhorovka, ELC EVEN 90 | 103,219 / 56,541 | `e43d0196e97e7c1fb3e8ce00d5f7c32376c80cb51ebc9b39992f48291eaf98d8` |

The recording vehicle in replay 1344 is entity `553323838`. Its 9,116
position records have a median interval of about 100 ms. These three records
show the LT-432 moving about 24.8 m horizontally over two replay seconds.
All three have secondary ID 0, a zero position error vector and flags 1.
An independent OpenSSL Blowfish and Python zlib decode reproduced the position
count and these three payloads.

| Replay clock (s) | Stream byte offset | Position X, Y, Z (m) | Yaw, pitch, roll (rad) |
| ---: | ---: | --- | --- |
| 36.04 | 479184 | 67.257, 5.041, −414.699 | −0.9790, 0.0132, −0.0025 |
| 37.04 | 493645 | 56.337, 5.020, −410.146 | −1.3354, −0.0045, 0.0011 |
| 38.04 | 508379 | 43.647, 5.488, −407.109 | −1.3363, −0.0739, −0.0058 |

To reproduce this window:

```sh
cargo wot-replay positions docs/replay-samples/wotlytics-1344.wotreplay --entity 553323838 |
  jq -c 'select(.clock_seconds == 36.04 or .clock_seconds == 37.04 or .clock_seconds == 38.04)'
```

For new mechanics, record the capture hash, client label, battle mode, entity
and clock range, record type and payload shape. Compare several observations
and check the matching client definitions. Use a controlled client session to
vary one input at a time when inferring command semantics, timing or physics.
