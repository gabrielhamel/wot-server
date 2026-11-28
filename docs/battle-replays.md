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

The repository includes raw Random Battle samples under
`docs/replay-samples/`, including the [Mines spotting sample](#mines-visibility-lifecycle-from-2026-11-27).
For additional captures, download an individual replay
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

## El Halluf Training capture from 2026-11-27

The development server's El Halluf Training battle at 12:20:36 Paris time
was recorded by the EU client as `replay_last_battle.wotreplay`, XML label
`v.2.4.2.0 #1007`, SHA-256
`83614ce511d985ad72712a4987aa191e50dd427b6730d3b396f780fa0154af2f`.
The AMX 30 has 480 type-`0x0a`, 45-byte vehicle position records with
419 distinct timestamps. Speeds below use a centred 200 ms difference
projected onto the middle sample's horizontal heading and right axis.

| Time | Observation |
| --- | --- |
| 16.02 s | During a left turn, the AMX 30 moved at 11.03 m/s forwards and 1.82 m/s to its right. At the centre of the hull, the extracted terrain had a 2.3° grade. Position offset 55913. |
| 22.52 s | On the downhill approach under forward-only input, an `AreaDestructibles` property update at offset 78360 added tree 36 to `fallenTrees` in chunk `0x8181`. Its extracted trunk is at (217.73, 54.59, 237.37). The hull descended throughout the contact and changed heading by about 17° between 22.32 and 23.02 s. Position offsets 77548, 78236 and 80066. |
| 35.12 s | The hull first rolled beyond 90° on a later descent. The terrain beneath its centre had steepened from 3° at 32.02 s to 53° here. |

The recording documents handling on the development server. The [Rapier driving scenarios](battle-mechanics.md#rapier-hull-physics)
use the numbered tree and approach to check the server's response to a break.

## Widepark Training capture from 2026-11-23

User-supplied `damage_physics.wotreplay` records a Standard Training battle
on `17_munchen` at 19:11:52 on EU1. The replay reports `v.2.4.1.1 #993` from
XML and `2.4.1.0` from the executable. The raw file SHA-256 is
`1dbb7aff7ac43d78e76f17a937fb636113301aa2e86dfbfa071d3e8c4f8386c4`.
The locally extracted client reports the same XML label with executable
SHA-256 `fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`.
Inspect with `cargo wot-replay summary` and `cargo wot-replay positions`.
The stream contains 6,860 type-`0x0a`, 45-byte position records.

| Observation | Replay evidence |
| --- | --- |
| Leopard 1 stops on the approach to the railway stock | Entity `3654511` advances about 1.02 m in the map plane from 55.82 to 55.92 s, then about 0.19 m over the next 100 ms. At 56.02–56.32 s its X/Z stays within 1 cm of `(124.93, -35.65)`. Position record offsets are 216981, 217513, 218098 and 219657. |
| TKS z n.k.m. 20 mm turns nearly in place | Entity `3654510` changes yaw by 0.298 rad over 128.12–128.62 s with 0.036 m of net X/Z translation. Position record offsets are 559962 and 562241. The player reported a wall fracture during pivoting. The position records establish the motion and do not identify the fracture or server force. |

The extracted map places the nearest flat carriage at approximately
`(126.26, 8.49, -41.51)` in world X/Y/Z, with `WGDE` chunk `0x807e` and
local index `46`. Its asset is `hd_rw_UNI_004_Carriage_01.havok`. Its intact
shootable body is distinct from the vehicle-only `s_wall_0` helper. A hull
sweep using the recorded position and yaw confirms contact with that
carriage at the stop. Training keeps the authored vehicle wall after
the visual body breaks so the same approach stays blocked. The replay
does not establish how the native server treats the helper after damage.


## Widepark LAN pivot capture from 2026-11-23

User-supplied `replay_last_battle.wotreplay` records a Widepark Training match
on the development server at 19:41:02. The replay reports the EU client
`v.2.4.1.1 #993`, executable label `2.4.1.0`, and contains 4,812 type-`0x0a`
position records with 45-byte payloads. The raw SHA-256 is
`26de01f6666bb95641eb7a83c5de424905160d250a5d82a071136be2d74e1d36`.

The recording AMX 30 B, entity `1963090098`, is at
`(246.39528, 7.99810, -33.58478)` with yaw `0.756306` rad at 132.91 s.
This is position record offset 476216. The position remains fixed during
the surrounding turn samples. The replay records state updates for the
destruction controller centered at `(254.28, 0, -48.61)`. Its cumulative
fragile-object list adds fence index `46` at 127.91 s and index `40` at
129.51 s. Index `45` appears at 137.01 s at stream offset 491130.
The position records do not establish how long the steering key was held.

With the two earlier breaks applied to the arena, the recorded hull pose
and a 0.04 rad turn increase horizontal overlap with fence panel `45`
in WGDE chunk `0x817e`. The previous code compared the footprint's global
reach along a single normal and could reject this corner contact before
applying scenery damage. The bundle-backed regression verifies that the
per-triangle test selects the panel and clears the contact after fracture.


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

## Battle position cadence

Use `cargo wot-replay positions` to select type-`0x0a` records for roster
vehicles with secondary ID zero. Group by entity and remove repeated
timestamps before calculating intervals. The local comparison uses the
[Tundra Training capture](#direction-indicator-in-the-2026-11-22-training-room-capture)
from WoT Server Public.

| Capture | Recorder vehicle | Distinct recorder timestamps | Median recorder interval | Median intervals for other observed vehicles | Other-vehicle intervals of 190–210 ms |
| --- | ---: | ---: | ---: | --- | ---: |
| EU 1344, Prokhorovka | 553323838 | 9,058 | 100 ms | 29 at 100 ms | 11.9% |
| EU 1345, Karelia | 543436810 | 7,127 | 100 ms | 24 at 100 ms, 1 at 200 ms | 22.8% |
| WoT Server Public Training, Tundra | 1925198086 | 2,482 | 100 ms | 3 at 100 ms | 0% |

Both EU replays show 100 ms as the usual recorder cadence, with more gaps
in the other vehicles' position streams. The reason for individual gaps
cannot be determined from playback records. The
[battle simulation](../src/battles/src/infrastructure/mod.rs) produces a
snapshot for each 100 ms tick. It sends each driver's own pose and each visible
moving pose every tick. Other visible parked poses use the configured interval.
[Mercury](../src/shared-infrastructure/src/mercury/app/base/mod.rs) packs
the clock, Avatar pose and selected vehicle poses into one unreliable bundle.
The gateway encodes each recipient's positions and applies channel sequencing,
pending ACKs and encryption. The archived
Battle Mechanics page gives historical intervals for spotting checks and
flags them as outdated after 9.16. It gives no position replication or ACK
frequency. A bidirectional wire capture is needed to compare datagram
counts, bytes, ACK timing and retransmissions.
[Archived Battle Mechanics](https://web.archive.org/web/20250303052442/https://wiki.wargaming.net/en/Battle_Mechanics#Rate_of_Visibility_Checks)

## Widepark parked vehicle cadence from 2026-11-26

An initial native client experiment on WoT Server Mac LAN used a 500 ms
parked refresh for all recipients, including their own hull, with one player
and two Leopard 1 dummies. The EU client reported
`v.2.4.2.0 #1000` and the launcher identified game build `2.4.2.5524`.
The launched `win64/WorldOfTanks.exe` SHA-256 was
`0547c40d8f55bb94437ba54603ddb7cec0075aca1a0d5a51cf181f1e044496f5`.
The working replay `.local/parked-poses-500ms.wotreplay` has SHA-256
`4f1a6eaf805f3eecf6cc86b1a7828d89a2f2aee5544f08e66a568c06ec5352a5`.

Use `cargo wot-replay positions` and remove duplicate timestamps for each
entity. The 45-byte type-`0x0a` records give a median interval of 100 ms for
640 Avatar samples (entity `490780339`, relative to `490780340)). Each dummy
(`490780343` and `490780346`) has 128 samples with a 500 ms median interval.
Leopard 1 `490780340` has 20 samples in the 26–28 s window while pivoting in
place, and four identical samples per two seconds from 52 through 64 s after
stopping. The corresponding server log has 100 consecutive snapshots and 60
vehicle pose records at arena ticks 600–699. A full refresh of three tanks
would include 300 records. At 30 bytes per Mercury VehiclePosition element,
that removes 720 bytes/s of pose elements per recipient before UDP framing.

A second native experiment on Prokhorovka reproduced the sparse own-hull
cadence. The driver reported about a second of movement response delay when
starting from rest. In a control battle on the same map with all vehicles at
100 ms, the driver reported that the delay disappeared. The 500 ms and
100 ms control replays have SHA-256 values
`23a7c090eec3bf0a0bc86c3dac5319e68c5ce78664e5bd95353ecbfbe1e88473` and
`fca1d9437442816a6219138225c9137fb430fd1c6c5d5d85ff0646e0a88c47f7`,
respectively. Historical
[BigWorld interpolation](references/BigWorld-Engine-14.4.1/programming/bigworld/lib/connection/avatar_filter_helper.cpp)
uses a buffer measured in recent input intervals. This is a possible
explanation for the delay in the current client.

The server now appends a driver's own parked pose on ticks that skip its
shared refresh. The encrypted dashboard E2E checks that each tick includes
the driver's own hull, the Avatar pose and pending ACKs, that other parked
vehicles refresh every fifth tick, and that motion resumes full-rate poses.
The native replay records client poses. The encrypted E2E inspects the
Mercury ACK footer.

## Vehicle movement and chassis animation

Replay 1344's LT-432 (entity `553323838`) was stationary through 32.04 s.
Measuring horizontal displacement over each following second gives 0.50, 5.27,
8.61, 10.57, 11.83, 13.05 and 13.85 m/s for windows starting at 32.04 through
38.04 s. The position records at those endpoints span stream offsets 424652
through 522385. The 71 position samples from 32.04 through 39.04 s all fall on
medium terrain in the installed `05_prohorovka` map at the hull center.
Steering, slope, loadout and throttle can all affect these rates.

In the same replay, type `0x07` has a 12-byte property header with entity ID,
property index and argument length, all little-endian `u32`.
The recorder's Vehicle property 15 (`engineMode`) carries a two-byte tuple.
It changes from `01 00` at 32.74 s (offset 433382), through `03 05` at
32.84 s (434922) and `03 01` at 33.94 s (449788), to `02 01` at 35.14 s
(465782). The second byte carries the movement flags. The older local Tundra
training capture below has no property 15 updates for its recorder Vehicle
(`1925198086`).

The supplied client executable has the SHA-256 in the EU evidence above.
Its Prokhorovka, Karelia and Tundra packages contain version 1
`terrain2/dominanttextures` maps. Each sampled tile has a 128 × 128 zlib-compressed,
row-major cell grid after the texture-name table. The high nibble of each cell
is the ground class (1 hard, 2 medium, 3 soft), matching the pinned Toolkit
[decoder](references/wg-toolkit-rs/wg-toolkit/src/space/terrain.rs).
The low nibble is the texture index. Four chassis samples blend the equipped
chassis's resistance and rolling friction at surface boundaries. Missing and
unpainted cells fall back to medium.

The server models longitudinal acceleration from engine power per unit mass,
surface resistance, rolling friction and hull pitch. It separately ramps the
yaw rate and integrates speed through throttle release, braking, and reversal.
The acceleration curve and its low-speed cap are approximations informed by
the replay and the Toolkit's
[capture comparisons](references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/drive.rs).
The server now uses [Rapier hull physics](battle-mechanics.md#rapier-hull-physics)
for terrain and scenery contact, suspension, and track grip.

For the driver, `Avatar.ownVehicleAuxPhysicsData` carries separate left and
right track scroll speeds in bits 40–47 and 48–55, from −15 to 30 m/s.
`Avatar.__onSetOwnVehicleAuxPhysicsData` forwards them to the appearance.
For other vehicles, the client forwards `Vehicle.engineMode` to the track
controller. Chassis with general wheels use the byte tuples `wheelsScroll`
and `steeringAngles`. The client unpacks each element into its wheel filters.
The wire layouts are validated against the extracted definitions at startup.
See [Avatar handling](references/wot-src/sources/res/scripts/client/Avatar.py#L1033),
[Vehicle handling](references/wot-src/sources/res/scripts/client/Vehicle.py#L749)
and the [Toolkit's field measurements](references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/client.rs).

Validation with the extracted local bundle covered 16,976 surface tiles.
The encrypted Training Room E2E verified driver track differential during
a turn, the remote engine mode, and the drive command's simulation and
delivery spans. A separate two-peer check with the EBR 105 decoded eight
wheel speeds and four steering angles on each peer and observed their
return to neutral after releasing the controls. Native visual verification
is pending.

## Direct hit in replay 1344

The recorder's vehicle is `553323838` and its Avatar entity is `553323883`.
The following records show its shot at vehicle `553323845`. Record type
`0x08` carries three little-endian `u32` fields before the method arguments.
They identify the receiving entity, the exposed method index in that entity's
dispatch table, and the argument byte count. The stream offsets below point
to the record headers. Match the method indices against the [dispatch
snapshot](wg-toolkit/dispatch-snapshot.md) and validate the argument layout
against the resource definitions.

| Replay clock (s) | Stream byte offset | Recipient entity | Exposed method index and argument size | Observation |
| ---: | ---: | --- | --- | --- |
| 87.74 | 999971 | Avatar | 37, 50 bytes | `showTracer` identifies the recorder as the shooter and assigns shot ID `0x0b9451ab`. |
| 87.74 | 1000252 | Shooter Vehicle | 1, 3 bytes | `showShooting` has burst count 1, gun 0 and shell kind 1. |
| 88.14 | 1003333 | Avatar | 28, 16 bytes | `stopTracer` supplies the endpoint for the same shot ID. |
| 88.14 | 1003373 | Avatar | 54, 10 bytes | `showShotResults` contains one result for the target with flags `0x00100010` and gun installation 0. |
| 88.14 | 1003797 | Target Vehicle | 10, 34 bytes | `showDamageFromShot` identifies the shooter and contains one packed hit-point entry and 281 damage. |
| 88.14 | 1003881 | Target Vehicle | 4, 10 bytes | `onHealthChanged` attributes the drop from 800 to 519 HP to the shooter. |

A type `0x07` record at offset 1003855 updates the target's health to 519
before the health-change callback. The recorder also received a hit at
77.14 s. The corresponding impact at offset 918302 and health callback
at 918398 identify the same attacker and show a drop from 1100 to 825 HP.
The two impacts involve different pairs of vehicles. The new and previous
health fields account for the reported damage in each case.

The impact at 88.14 s uses static network ID 0, hull index 1 and
penetration effect 4. At 77.14 s the point uses static network ID 0,
chassis index 0 and critical hit effect 5. The client decodes the segment against
the selected part's bounding box and tests it against the local collision
mesh. Its impact handler exits before audiovisual feedback when the
decoded point list is empty. The legacy shot effect tables can select a separate `impactNPC_PC` sound
when the local player receives a penetrating hit. See [point decoding](references/wot-src/sources/res/scripts/client/VehicleEffects.py),
[vehicle impact handling](references/wot-src/sources/res/scripts/client/Vehicle.py#L461)
and [shot effects](references/wot-src/sources/res/scripts/item_defs/vehicles/common/shot_effects.xml).

Component record `0x37` at offset 1003496 contains a dealt-damage event
(type 7) for `553323845`. Its packed details, `0x0007f8023200fe10`, decode
to 281 HP, primary reason 0 (shot), damage-log shell type 127 (unknown), secondary
reason 16 (none) and extended reason −1 (none). The component record at
918123 contains a received-damage event (type 10) for attacker `553323830`.
Its packed details, `0x0007f80226000210`, decode to 275 HP, reason 0 and shell
type 1 (AP). See the [battle feedback field
definitions](references/wot-src/sources/res/scripts/common/BattleFeedbackCommon.py)
and the [client damage-log shell enum](references/wot-src/sources/res/scripts/common/constants.py).
The tracer uses the projectile-effect enum, where AP is 2. Training sends
feedback through `Avatar.onBattleEvents` using the damage-log enum, with the
shell's mechanics and stun flag distinguishing the HE variants.

Replay 1345 corroborates the target and health fields for lethal shots. At
524.42 s, the recorder's shot result at offset 4741469 names target
`543436828` and carries flags `0x00100111`. The health callback at
4742054 reports 60 to 0 HP. At 704.76 s, offsets 6056615 and 6057018 show
flags `0x00100511` and a drop from 165 to 0 HP for `543436820`. Both carry
the kill bit `0x1` and additional device flags.

The fired command, the other client's private feedback for the same shot,
delivery acknowledgments, and the remaining component updates require a
controlled capture of both live connections. The approximately 400 ms
between the launch and impact records measures this replay's shot. The
sample does not establish a fixed delay or a collision algorithm.

## Direction indicator in the 2026-11-22 Training Room capture

The local EU client identified above saved a Tundra training replay after
midnight. The working copy is
`.local/replays/training-2026-11-22-tundra.wotreplay` with SHA-256
`c0470b410aa72d471a9e02e34026d27581d1b05f18cbcccb1dce2ca9bad57734`.
`cargo wot-replay summary` reports three 33-byte entity method records.
Each has the 12-byte entity method header, Avatar method index 32
(`showOwnVehicleHitDirection`) and 21 argument bytes. At 38.81 s, stream
offset 214528, the old server sent −2.824 radians. The contemporaneous
attacker-to-victim positions give approximately 0.319 radians. The
opposite yaw matches the player's report of a rear hit highlighting the
front. Battles now sends the projectile travel yaw. The encrypted E2E
asserts the wire angle, while native rendering still needs a walkthrough.

The [Widepark Training capture](#widepark-training-capture-from-2026-11-23)
contains two incoming non-penetrations from TKS entity `3654510` to Leopard 1
entity `3654511`. At 172.42 s and 172.52 s, type-`0x37` records at stream
offsets 772294 and 773009 contain a 21-byte hit direction value starting at
payload byte 7. In [`VEHICLE_HIT_DIRECTION`](references/wot-src/sources/res/scripts/entity_defs/alias.xml)
field order, the yaw is about −1.04 radians, the amount is 11 HP, and
`isBlocked` is true. The matching `showDamageFromShot` records at offsets
772391 and 773142 carry impact group 3 and zero actual damage. The
[TKS 20 mm AP shell definitions](references/wot-src/sources/res/scripts/item_defs/vehicles/poland/components/shells.xml)
list 11 nominal armor damage.

For armor blocks, ricochets and screen stops with zero HP loss, Battles sends
the same Avatar method with the incoming projectile's travel yaw and
`isBlocked=true`. The [client Avatar](references/wot-src/sources/res/scripts/client/Avatar.py)
forwards the flag to the battle HUD, whose [marker builder](references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/battle/shared/indicators.py)
chooses the blocked marker before checking the amount. The extended marker
uses that amount for its label and size. Battles supplies nominal shell damage
for the blocked indicator. The resource-backed projectile test checks
victim-only delivery and shot traces for armor blocks, ricochets and screen
stops.

## Scene impacts and gun audio

The two tracked EU replays carry Avatar method `explodeProjectile` at exposed
index 67 in record type `0x08`. Match its first field to `showTracer`'s shot ID
on the same Avatar. The little-endian argument layout is `i32` shot ID,
four `u8` values (legacy effect, prefab effect, material and shell kind),
`f32` effect caliber in millimetres, two three-component `f32` vectors
(endpoint and travel direction), `f32` speed, and a packed-length array of
`u32` destructible references. An empty array uses one zero byte, giving
41 argument bytes. Nonempty arrays in replay 1344 have 45, 49 and 73
argument bytes. The direction vector can have a length greater than one.

| Replay | Shot ID | Tracer clock / offset | Impact clock / offset | Material | Shell and caliber | Legacy / prefab |
| --- | ---: | --- | --- | --- | --- | --- |
| 1344 | 194388395 | 140.24 s / 1443772 | 140.34 s / 1444839 | 0, ground | APCR, 85 mm | 34 / 3 |
| 1344 | 195316139 | 613.64 s / 4967341 | 613.84 s / 4968844 | 0, ground | AP, 85 mm | 29 / 2 |
| 1344 | 194294187 | 108.74 s / 1178233 | 109.04 s / 1181009 | 2, stone | AP, 90 mm | 29 / 2 |
| 1344 | 194937259 | 325.84 s / 2921297 | 326.24 s / 2924145 | 5, metal | APCR, 128 mm | 42 / 3 |
| 1345 | 31690887 | 690.56 s / 5910446 | 690.86 s / 5913979 | 0, ground | APCR, 105 mm | 34 / 3 |
| 1345 | 31187079 | 61.22 s / 852581 | 61.52 s / 856720 | 2, stone | AP, 100 mm | 29 / 2 |

Replay 1344 also includes eight destructible references in the wood impact at
13.14 s, offset 221491. The material names follow first occurrence in
[`material_kinds.xml`](references/wot-src/sources/res/system/data/material_kinds.xml),
as read by the [client material table](references/wot-src/sources/res/scripts/common/material_kinds.py).
The [projectile mover](references/wot-src/sources/res/scripts/client/ProjectileMover.py)
looks up the material-specific hit effect and emits a scene event carrying
the shell kind and caliber. The [shot effect table](references/wot-src/sources/res/scripts/item_defs/vehicles/common/shot_effects.xml)
includes different sound switches for ground, stone and metal.

For muzzle audio, the [shooting extra](references/wot-src/sources/res/scripts/client/vehicle_extras.py)
loads effects from the equipped gun in the vehicle descriptor and forwards the
shell kind from `Vehicle.showShooting`. The [gun effect table](references/wot-src/sources/res/scripts/item_defs/vehicles/common/gun_effects.xml)
defines sound families such as automatic, small and large, with separate
local and other-player events. Gun definitions select the effect family.
For incoming hits, `Vehicle.showDamageFromShot` selects the hit group and
forwards the caliber and shell kind decoded from the packed hit point. The
87.74 s tracer in replay 1344 identifies an 85 mm HE shell with legacy/prefab
indices 31/1. Its `showShooting` also carries HE kind 1.

Training selects the first hit along the shot ray from the outdoor
heightfield, placed obstacle meshes, and eligible vehicles. The heightfield
uses a one-metre search and a binary refinement. The server bounds the
obstacle search with the nearer of the terrain and vehicle candidates. It
sends the tracer and muzzle event at launch, then resolves contact once the
distance divided by the advertised shell speed has elapsed. For a scene impact,
`explodeProjectile` carries the collision point, material effect index and
loaded shell data. Contacts use the poses at launch and
are processed on the battle clock. The trajectory is straight.

The obstacle extractor adapts the pinned Toolkit's
[compiled space readers](references/wg-toolkit-rs/wg-toolkit/src/space/bin)
and [Havok mesh decoder](references/wg-toolkit-rs/wg-toolkit/src/havok).
`BSMI` supplies transforms and visibility masks, `BSMO` links models to
their collision hashes, and `BWST` resolves those hashes to resource paths.
The current client's `BSMI` model records have an eight-byte stride and
the model index is the first word. The extractor stores the placement and
the material of each collision triangle. It skips bodies marked as passing
shells and bodies named as destruction remains, per the reference's
classification. Battles filters placements by gameplay visibility and
translates raw material kinds to the client's effect table.

As a check against the extracted EU client geometry identified above, a
ray through the recorded stone impact in replay 1345 at 61.52 s struck
material kind 111 (stone, effect index 2) approximately 0.9 mm from the
recorded endpoint. The scene extraction found 2,979 usable collision meshes
across 74 compiled spaces. It reported 10 unavailable or unsupported
models. SpeedTree collision, destructible damage and destruction replication
remain open. Training sends an empty destructibles list. Live client
playback is needed to verify audio and visuals.

## Persistent vehicle impact marks

`Vehicle.showDamageFromShot` plays the transient impact. The client's
[`Vehicle.set_damageStickers`](references/wot-src/sources/res/scripts/client/Vehicle.py#L668)
adds and removes marks as the `Vehicle.damageStickers` array changes.
[`setDamageSticker`](references/wot-src/sources/res/scripts/common/helpers_common.py#L277)
uses the shell's configured `targetStickers.armorPierced` or
`targetStickers.armorResisted` sticker as the low byte of the segment and
the prefab effect index as the high byte of the hit parameters. Battles
uses the penetration sticker for a penetration that removes HP. It uses the
resisted sticker for ricochets, armor blocks and screen stops. Both use the
hit part and segment from the armor solver and retain the impact group in
the hit parameters. The [client sticker renderer](references/wot-src/sources/res/scripts/client/VehicleStickers.py)
selects the prefab decal or the legacy sticker. New arena viewers receive the current
array as part of entry. The server keeps the 15 most recent distinct marks
per vehicle, because a one-byte count plus 16 bytes per point fits the
property's Variable8 wire framing. This is a server limit. The client's
renderer limit remains unmeasured.
The blocked-impact regression verifies the serialized property for both
viewers and its trace. The EU replay confirms the blocked hit effects.
Rendering the new persistent marks in a live native battle remains unverified.

The November 23 Mac LAN Training Room capture on Munich at 14:04 Paris
time used the EU executable identified above. Its SHA-256 was
`b9400e54b5be220dedb3cd58a222a3ea314c4313dffc9bc0aaa5297c0fc1d121`.
The replay contained three `showDamageFromShot` calls with legacy effect 0
and prefab 255, and no Vehicle property 33 updates. The running server's
bundle lacked the shot effect and damage sticker definitions. That bundle
explains the absence of sticker updates even though the impacts reached the
client.

After restarting Mac LAN with the newly extracted bundle, the next Training
Room replay recorded property 33 with a single hull sticker at 44.62 s
(offset 170370, APCR, sticker 12, prefab 3) and at 62.22 s (offset 220972,
HE, sticker 14, prefab 5). The in-progress replay snapshot had SHA-256
`e0852264206934c55573f78fae7b612db22650731df0828a7605fe9870fb7934`.
The tester confirmed visible marks on the vehicles in that battle.

## Range expiry

The 2026-11-22 WGUS lookup returned EU production build `2.4.1.5514` for
both Windows x64 client profiles. The local executable identified above and
the extracted bundle used for E2E report the same XML label as the replays.

In the tracked Random Battle recordings identified above, the last four-byte
float in the 50-byte `showTracer` arguments is the range in metres (offset
44, followed by gun and installation indices). For these examples the same
Avatar receives no matching `stopTracer` (index 28) or `explodeProjectile`
(index 67) later in the recording.

| Replay | Clock (s) | Stream offset | Shot ID | Range (m) |
| --- | ---: | ---: | ---: | ---: |
| 1344 | 101.54 | 1119649 | 194279851 | 720 |
| 1344 | 219.14 | 2103468 | 194632107 | 431.99 |
| 1345 | 89.12 | 1212381 | 31221895 | 720 |
| 1345 | 700.96 | 6020152 | 31694983 | 720 |

These observations are consistent with the client expiring tracers locally.
[`Avatar.showTracer`](references/wot-src/sources/res/scripts/client/Avatar.py)
passes the range to [`ProjectileMover.add`](references/wot-src/sources/res/scripts/client/ProjectileMover.py),
which gives it to the client ballistics engine. The records measure the range
advertised to that client. The shorter values may reflect shots first
replicated partway through flight. The direct hit at 87.74 s in replay
1344 (shot `194269611`) also advertises 720 m. The separate stop packet
defines the earlier endpoint.

Training starts with the equipped gun's nominal `maxDistance` and limits it
by the zero crossings of the shell's penetration and damage curves. The
server and tracer use the resulting range and the same launch velocity.
Artillery tracers also receive the configured gravity used by the server.
An unobstructed shell expires without an impact message. A missing or
invalid shot definition rejects the shot before it consumes ammunition.
The client’s
[`computeShotMaxDistance`](references/wot-src/sources/res/scripts/common/helpers_common.py)
is the reference for the range calculation.

## Round end and movement

Both recorded Avatars receive `onRoundFinished` (index 10, arguments
`02 01` for team 2 and elimination), immediately followed by `updateArena`
(index 58, update type 3) on the same replay clock. Its inflated pickle
contains period 4, an arena-clock deadline, a `60.0` second period length
and the tuple `(2, 1)`. The recordings continue to carry position samples
for about six seconds, considerably shorter than the advertised period.

| Replay | Clock (s) | Outcome / period offsets | Last recorded pose clock (s) |
| --- | ---: | --- | ---: |
| 1344 | 899.34 | 6579777 / 6579803 | 905.745 |
| 1345 | 710.26 | 6107541 / 6107567 | 716.66 |

In 1345, vehicle `543436815` travels about 0.70 m between the outcome and
rest. At 710.56 s (offset 6109356) it makes its last individual position
step greater than 2 cm. Its pose is still replicated at 716.66 s (offset
6159939). Replay 1344's remaining vehicles are stationary at the outcome.

Training sends the two outcome methods in the recorded order. It preserves
the 60-second period on the wire and uses six seconds before returning
remaining participants to the garage. Vehicles coast on the server with
neutral drive input while position and chassis updates continue. The native
[game messages controller](references/wot-src/sources/res/scripts/client/gui/battle_control/controllers/game_messages_ctrl.py)
handles the in-arena outcome banner. Detailed post-battle reports and
Random Battle progression are outside the implemented Training end sequence.
The replay alone cannot establish the complete live transport teardown.

## Turret and reticle replication

In the same EU sample corpus, replay record `0x07` carries an entity ID,
exposed property index and value length as three little-endian `u32` fields,
followed by the value. The current resource dispatch identifies Vehicle index
13 as `gunAnglesPacked`, a two-byte value. The same property appears 5,604
times for the LT-432 in replay 1344 and 2,514 times for the SFAC 105 in 1345.
These offsets point to the start of each replay record.

| Replay | Clock (s) | Stream offset | Recorder vehicle ID | Packed value |
| --- | ---: | ---: | ---: | --- |
| 1344 | 87.64 | 999394 | 553323838 | `0x3199` |
| 1344 | 87.74 | 1000323 | 553323838 | `0x311c` |
| 1345 | 32.42 | 425277 | 543436810 | `0x81ba` |

A local Lost City training recording from 21:04 Paris time on 2026-11-21
used the same XML client label. The running `win64/WorldOfTanks.exe`
had SHA-256 `58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11`.
The Lost City replay's SHA-256 was
`e4272373a3afb3cd383b2660be5770211cfb3b75f7a814827f9c842960f02d74`.
`cargo wot-replay summary` found 810 type-`0x26` 40-byte playback records
and no type-`0x07` records. The Sand River recording from 22:02
(SHA-256 `1f021e714e574a2fb71c522a9a754d3743b1cc29710fa4d610d6a91ada20c925`)
also had no type-`0x07` records. The `0x26` counts describe playback
records whose purpose is unconfirmed here.

The [client gun rotator](references/wot-src/sources/res/scripts/client/VehicleGunRotator.py)
compares its predicted turret yaw with the decoded server yaw and corrects it
when the difference exceeds its latency-adjusted tolerance.
The client calls `Avatar.setServerMarker` through its cell mailbox when the
server aim setting changes. The [gun marker component](references/wot-src/sources/res/scripts/client/GunMarkerComponent.py)
feeds the optional server marker using a launch position, initial velocity
and angular dispersion. [Toolkit's vehicle replication](references/wg-toolkit-rs/wg-toolkit-wot/src/base/battle/vehicle.rs)
confirms the selected Vehicle property and owner component routes.

Training sends live packed angles as standalone Vehicle property updates
and uses the current resource definitions for their index and size. It attaches
the gun marker component to the owner's Vehicle node, then sends changed
marker values to that owner when the setting is enabled. The marker uses the
same launch origin and direction as the Training shot, the Training tracer
speed, and the configured gun's base dispersion angle. Training resolves
hits along a straight line. The client projects the server marker using its
own collision and trajectory logic.

The Ensk recording from 22:15 Paris time on 2026-11-21 (SHA-256
`550f08b6bc8d074798108d109bdbbeaf2adee2d9a865c844dcd467373c590691`)
contains 420 turret updates for the recording AMX 30, entity `1508983177`.
It accompanies a screen recording of the turret snapping during a stationary
crosshair sweep near a railcar. The hull stayed at approximately
`(15.99, 0.63, −9.82)` during the following samples. The type-`0x1c`
playback payload is a little-endian `f32` consistent with the recorded
client turret yaw, including samples that snap to the decoded server yaw.

| Replay clock (s) | Server record offset | Server yaw | Client yaw |
| ---: | ---: | ---: | ---: |
| 103.73 | 325426 | 21.80° | 37.77° |
| 103.83 | 325779 | 18.63° | 41.93° |
| 103.93 | 326132 | 18.63° | 18.63° |

The angle updates establish that native aim reached the server. The
[Avatar client script](references/wot-src/sources/res/scripts/client/Avatar.py)
uses `ownVehicleAuxPhysicsData` for the stabilised rotation, and
[the Toolkit capture](references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/client.rs)
records its bit layout. Training now initializes that field with the spawn
orientation and updates the owning Avatar as the hull changes. The server solves
aim in hull coordinates using the equipped chassis, turret and gun offsets.
Solving the lateral gun offset directly makes a stationary target independent
of the current turret angle. The Training simulation uses a fallback gun mount
when the equipped resource definitions have no usable geometry.

## Vehicle fire

EU playback examples show `Vehicle.onHealthChanged` in type `0x08` records.
Each payload is 22 bytes: three little-endian `u32` fields (vehicle, method
index and argument length), then new and previous HP (`i16`), attacker
entity ID (`u32`), reason (`u8`) and extended reason (`i8`). In these replays
the method index is 4, the argument length is 10, and fire uses reason 1
with extended reason −1.

| Sample | Client label | Observation | Inflated record offsets |
| --- | --- | --- | --- |
| [1499, Redshire](https://wotlytics.com/api/v1/replays/1499/download/) | EU `v.2.4.2.0 #1000`, exe label `2.4.2.0` | Remote vehicle `6663015` goes from 725 to 642 HP 0.50 s after the hit. Attacker `6663022` appears in both health calls. | 4923274 shot, 4923308 component creation, 4927985 fire tick |
| [1500, Klondike](https://wotlytics.com/api/v1/replays/1500/download/) | EU `v.2.4.2.0 #1000`, exe label `2.4.2.0` | Remote vehicle `543402060` goes from 574 to 493 HP 0.50 s after the hit. Attacker `543402078` appears in both health calls. | 2983919 shot, 2983953 component creation, 2988239 fire tick |
| [1497, Prokhorovka](https://wotlytics.com/api/v1/replays/1497/download/) | EU `v.2.4.1.1 #993`, exe label `2.4.1.0` | Vehicle `549043364` loses 88, 80, 73, 65, 57, 49, 42, 34, 26, 19, 11 and 3 HP at 0.50 s intervals. The recorder later starts a fire on another vehicle, which loses HP over 14 ticks. | 3020425 component creation, 3026574 first tick, 3091428 component removal, 3092001 last tick |
| [1356, Prokhorovka](https://wotlytics.com/api/v1/replays/1356/download/) | EU `v.2.4.1.1 #993`, exe label `2.4.1.0` | The recorder's vehicle `539169059` takes a 42 HP fire tick after the ignition shot. The component appears in the same frame as the shot and is removed on the tick frame. | 4516922 shot, 4516980 component creation, 4521852 component removal, 4521994 fire tick |

The raw SHA-256 values are
`0d2932946f88e5b28db836a6faa9d99eb3c8f08a8e7acfe7582c9eae73c8d4d2` (1499),
`efc5cb46ae0af757fcc48724904675f60573fbffc88f5de7e991ed5d08d8a0a0` (1500),
`2a6c9f90618ff9be01c1ebe58234f0bd18a3a152ebd1691e0c770bc922f15096` (1497)
and `e43d0196e97e7c1fb3e8ce00d5f7c32376c80cb51ebc9b39992f48291eaf98d8` (1356).
The local compiled EU client for the current bundle reports
`v.2.4.2.0 #1000` and executable SHA-256
`0d7d61866dfe8f884a76f31a6c91308662b27e0ff09f14f08647234048c939f6`.
On 2026-11-26, `wg-launcher inspect` listed EU production release
`2.4.2.5524` with EN localization revision `2665505`, which also appears
in the installed client's `loc_version.xml`.

The creation samples are replay type `0x32`. They contain a four-byte length
prefix followed by 12 bytes of component creation state. Replay type
`0x37` carries buffered replication updates. At offset 4521852 its final two
bytes match the created component node `0x0ce3`. The server uses the
[Toolkit's captured component creation and unlink framing](references/wg-toolkit-rs/wg-toolkit/src/app/entity.rs)
and resolves the `Fire` component type from each installed bundle. The
[client script](references/wot-src/sources/res/scripts/client/Fire.py)
uses component lifetime for the world effect and `set_fireInfo` for driver
notifications. Replay records establish visible state and damage cadence.
A bidirectional capture would be needed to establish client extinguisher
commands and production wire ordering.

## Mines visibility lifecycle from 2026-11-27

The [Mines replay](replay-samples/mines-2026-11-27.wotreplay) is the
user-supplied EU1 recording `20261127_0901_china-Ch52_WZ_122_6_F3_10_hills.wotreplay`.
Its SHA-256 is
`a0088ae4d967b627ca018aea716af75d0bd740baccffd8bce5df3a9f7a3e5c74`.
Its metadata reports `2.4.2.0 #1007` and map `10_hills`. The matching
local `win64/WorldOfTanks.exe` has SHA-256
`a13b2bac5f76e5399f399f8421630730da4b02ff35f9d866f0bc36d10f8cae80`.

The playback stream has 345 type-`0x05` creation records and 217
four-byte type-`0x04` removal records. The first little-endian 32-bit
field identifies the entity in both. Entity `9028` appears at 48.17 s
(offset 709192), leaves at 61.78 s (offset 1006608), and appears again
at 74.68 s (offset 1211030). The two create payloads are 187 bytes.
These records establish observable entity lifetime. They do not expose
the reason for an individual exit, the radio graph, or raw UDP framing.

The installed bundle declares `OwnVehicle.onObservedByEnemy(INT16)`.
`OwnVehicleBase.pyc` forwards it to `Avatar.pyc`, which triggers Sixth
Sense. `constants.pyc` assigns 0 to ordinary ray detection. The roster
`position` is an X/Z pair of signed 16-bit integers. The client
`AvatarVehiclesInfoBase.pyc` forwards position updates to the
`ClientArena` minimap state. A bidirectional client capture would show
minimap transitions on the wire.

### Training client crash and Sixth Sense

The unfinished Training recording `temp.wotreplay` from 2026-11-27 (SHA-256
`13f12ce4983087f441b2edcd935381998fb935ad7204a9947dc7faaf893a7210`)
contains the raw playback stream and ends mid-record. It has 9,559 complete
records. At 128.34 s a remote ELC EVEN 90 is created. At 131.34 s the
`0x37` record at offset 346205 contains the six-byte NRL payload
`04 1e 00 02 00 00`. The client's log reports `ClientHub::processUpdateNode:
Can't find nodeID 0` in four Training runs on the same installed executable
(`2.4.2.1580 #2671428` in `game.log`).

In the official Mines replay above, the `OwnVehicle` creation at stream
offset 14961 registers method index 5 (`onObservedByEnemy`) as node `0x0303`.
The first corresponding `0x37` record is at 150.675 s (offset 2562126).
It contains four NRL bytes, `03 03 00 00`, and is followed by the `0x36`
apply record. The six subsequent calls use the same bytes. The big-endian
node ID is followed immediately by the little-endian detection reason.
An NRL method reads its declared arguments directly, as shown in the
[Toolkit NRL decoder](references/wg-toolkit-rs/wg-toolkit/src/app/nrl.rs).
The old Training payload used property framing between the node and the
arguments. It therefore supplies a reason of 512 and leaves `00 00` in
the stream for a later update to consume as node 0.

The client also logged `No such entity` for a volatile pose immediately
before a revealed vehicle's appearance became ready. The server now sends
pending reliable lifecycle messages before its unreliable poses. The
snapshot names the latest reliable sequence in its Mercury footer so a
receiver drops a pose that arrives ahead of creation or trails removal.

### Prokhorovka visibility follow-up

The local Training recording `replay_last_battle.wotreplay` from 2026-11-27
reports EU `2.4.2.0 #1007` and `05_prohorovka`. Its SHA-256 is
`67c8b4697f48fdd8667ead159c833fd0d538197f624c212cd903d095a2e95650`.
The installed x64 executable has the same SHA-256 as the Mines test above.

The AMX 12 t recorder and the ELC EVEN 90 are stationary about 305 m apart
near the end. The client's ELC entity leaves at 572.425 s (stream offset
3312770). The ELC fires at 573.925 s and reappears at 574.425 s. It leaves
again at 592.425 s, fires at 597.825 s and reappears at 598.425 s. Its last
removal at 614.425 s is about 16.6 seconds after its last shot. The timing
fits a 5 s firing penalty followed by the far visibility check and 10 s
retention. The installed vehicle definitions give the recorder 360 m base
view range and the ELC 0.388 stationary concealment and a 0.25 firing
multiplier. Under the server's formula its uncovered detection threshold
changes from about 300 m to 345 m when firing. These observations cover
the recorder's view of the ELC.

The `0x06` records contain 28 distinct native tree falls. All 28 identities
are present in the older local `#1000` extraction, including
`0x7c7a008e` at 522.825 s. Only five are present in the fresh `#1007` trunk
catalog. This points to the older bundle, whose Prokhorovka geometry has no
`foliage.bin`. Its trees can fall visually while providing zero concealment.
The fresh `#1007` extraction has 5,393 foliage placements on this map,
including the late high-crown tree `0x7c7a0019` that fell at 543.425 s.

All five ELC creations start at the same hull pose as the next recorded
position sample. The reveal at 94.915 s (offset 544987) initializes
the gun angle to `0x8025`, then a separate property sets it to `0x552a`
(offset 545148). At 598.425 s (offset 3457834), creation again uses
`0x8025` and full health before publishing `0x87ec` and 668 HP. The first
parked pose follows about 0.4 s later. Vehicle creation now carries live
angles, health and chassis state, and a reveal refreshes pose cadence
immediately. Native rendering of that change still needs verification.
