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
Ground following still uses the height map. Suspension dynamics, lateral slip
and handbrake drifts require separate work.

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
distance divided by the advertised 450 m/s has elapsed. For a scene impact,
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
uses the shell's configured penetration sticker as the low byte of the
segment and the prefab effect index as the high byte of the hit parameters.
Training uses that encoding for hits that remove health and whose legacy
effect defines a penetration mark. The client applies the prefab or legacy
appearance from its effect tables. New arena viewers receive the current
array as part of entry. The server keeps the 15 most recent distinct marks
per vehicle, because a one-byte count plus 16 bytes per point fits the
property's Variable8 wire framing. This is a server limit. The client's
renderer limit remains unmeasured.
The hit segment still crosses an approximate hull volume.

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

Training uses the equipped gun's nominal `maxDistance` for the loaded shell
and retains the straight-ray simulation at 450 m/s. Every tracer advertises
that range. A miss has no impact message. An invalid or missing range rejects the shot before
consuming ammunition. The parser accepts positive distances through 10 km,
the largest value present in the reference shot definitions, which bounds
the one-metre heightfield search. The official client also computes effective
range limits from piercing power and damage curves in
[`computeShotMaxDistance`](references/wot-src/sources/res/scripts/common/helpers_common.py).
Training still uses nominal range because it does not simulate those curves.

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
