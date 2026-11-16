# Gameplay simulation and game-data dependencies

[Map home](README.md) · [Session flows](session-flows.md) · [Reproduction plan](reproduction-plan.md)

## Battle ownership and controls

[battle.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/battle.rs) owns a
`Battle` per client: avatar, space, configured player vehicle, bot vehicles,
physics `World`, shots in flight, arena-info components and destructible state.
Controllers distinguish player and bot input; both feed the same vehicle model.
Cell RPCs handle movement flags, aim, shooting and settings; base RPCs handle
readiness and leaving. Received avatar-position updates are logged rather than
used as authoritative vehicle simulation.

This organization demonstrates bot gameplay and entity replication. It does
not implement two human clients controlling vehicles in a common battle.

## Physics module map

[physics/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/mod.rs)
explains the integration order and tuning provenance in detail.

| Module | Responsibility |
| --- | --- |
| [mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/mod.rs) | `World`, vehicles/hulls, tick orchestration and collision passes |
| [drive.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/drive.rs) | Acceleration, braking, steering and traction tuning |
| [body.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/body.rs) | Rigid-body integration, gravity, suspension, track forces |
| [contacts.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/contacts.rs) | Suspension/track/hull/turret contact sampling |
| [ground.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/ground.rs) | Firm, medium and soft ground effects |
| [collision.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/collision.rs) | Vehicle box contact, swept contact timing and impulse response |
| [walls.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/walls.rs) | Map bounds, static-model correction, ram lookahead |
| [rubble.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/rubble.rs) | Accumulated ram/shot damage, broken pieces and fresh breakage events |
| [turret.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/turret.rs) | Traverse/elevation limits, ballistic aim and dispersion |
| [client.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/client.rs) | Engine mode, approximate gear/RPM and packed auxiliary state for client animation/aim |
| [fixtures.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/fixtures.rs) | Test-only vehicle/terrain fixtures |

A world step remembers poses, advances vehicles, integrates their bodies in
substeps, resolves intervehicle collisions, and collects breakages. This is a
custom simplified model tuned against reported drives, not the original
`WGTankPhysics` engine or recovered official server code. Some constants are
measured, some fitted, and some explicit choices; preserve these distinctions.
[drive_replay.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/drive_replay.rs)
contains a comparison harness, with external fixtures described in [verification](verification.md).

Auxiliary physics packing uses yaw 15 bits, pitch 12, roll 13, left/right track
speeds 8 each, RPM 8, in a 64-bit value. The source records ranges and rounding;
reuse its codec and tests instead of independently approximating the packing.

## Weapons, damage, and bots

| Source | Existing behavior | Compatibility limit |
| --- | --- | --- |
| [gun.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/gun.rs) | Ammo inventory, loaded/next shells, reloads, magazines, bursts, trajectories, vehicle/terrain/static hits, ricochet continuation and destructible traversal | Does not establish all modern special weapon mechanics; flight uses 5 ms sampling and a ricochet cap |
| [armor.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/armor.rs) | Penetration, screen/main-armour distinctions, ricochet and explosion damage | Explicitly uses legacy HE explosion rules; terrain skim ricochets are an intentional behavior differing from the game |
| [battle.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/battle.rs) | Firing/impact/tracer messages, health, kill reporting, ammo/reload state, hit effects and destruction replication | No complete durable postbattle reward/statistics pipeline established |
| [bot.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/bot.rs) | Approach and aim toward enemies, optional firing/driving | Simple steering, not pathfinding or official AI |

Enemy spotting uses horizontal distance ≤445 m; allies are always visible.
There is no terrain/building occlusion or full camouflage/view-range/perk
calculation in `spot`. Engine AoI replication and gameplay spotting are distinct:
a working witness mechanism alone does not reproduce World of Tanks visibility.

## Game-data ingestion

```text
res loose files + .pkg archives
  ├─ script .def / aliases / extensions → runtime entity ABI
  ├─ item_defs → vehicle, crew, modules, shells, prices
  ├─ arena_defs → map identity, geometry, bounds, spawn positions
  └─ spaces + collision assets + destructibles.xml
       → terrain + placed static collision + client destructible numbering
       → battle physics, shell hits and replicated breakages
```

| Data | Reader / adapter | Why the server needs it |
| --- | --- | --- |
| Loose resources and packages | [res/fs.rs](../references/wg-toolkit-rs/wg-toolkit/src/res/fs.rs), [res/package.rs](../references/wg-toolkit-rs/wg-toolkit/src/res/package.rs) | Common read-only resource view and archive indexing |
| Packed XML | [pxml/](../references/wg-toolkit-rs/wg-toolkit/src/pxml/) | Definition trees; not a general text XML replacement in every reader |
| Vehicles/modules/shells | [item_defs.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/item_defs.rs) | Stock descriptors, health, mobility, gun/aiming/magazine data, materials and collision references |
| Owned item identities | [items.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/items.rs) | Integer descriptors plus configured vehicle and crew byte descriptors |
| Shop prices | [item_prices.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/item_prices.rs) | Prices and not-in-shop items from catalogs |
| Maps | [arena_defs.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/arena_defs.rs) | Arena type IDs, geometry paths, bounds and team spawns |
| Vehicle armour/collision | [wot/collision.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/collision.rs) | Part transforms, triangle hits, armour material groups |
| Compiled spaces | [space/bin/](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/) | BWTB table and known sections: BWST, BWT2, BWSG, BWCS, BWAL, BSMO, BSMI, WGDE |
| Terrain | [space/terrain.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/terrain.rs) | Height data, terrain triangles and dominant material information |
| Static geometry | [space/statics.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/statics.rs) | Placed collision triangles, transforms, body kinds and spatial indexing |
| Destructible definitions | [space/destructibles.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/destructibles.rs) | Kind, health, structure modules/dependencies and client material numbering |
| Havok assets | [havok/](../references/wg-toolkit-rs/wg-toolkit/src/havok/) | TAG0 data/type/index structures and supported collision shapes; a reader, not the Havok simulation engine |
| Visual/primitive models | [model/](../references/wg-toolkit-rs/wg-toolkit/src/model/) | Inspection and geometry tooling; not the main runtime collision source |

`Space::load` joins terrain and static geometry for a gameplay visibility mask.
Statics combine `space.bin` string/model/instance tables, Havok collision assets,
`destructibles.xml`, and WGDE numbering. Client breakage messages must refer to
the same chunk/index/module identities; arbitrarily numbering server obstacles
would break the visual contract.

Map geometry is shared immutable data (`Arc<Space>`); each world's `Rubble`
tracks which pieces have broken. Keep that separation for shared multiplayer
battles: map catalog state should not be mutated globally by one match.

## Current configuration surface

The [CLI](../references/wg-toolkit-rs/wg-toolkit-wot/src/main.rs) supports repeated
`--give-vehicle <nation>:<name>`, `--map` (default `02_malinovka`), `--allies`,
`--enemies` (default 1), repeated `--bot-vehicle`, `--bots-fire`, `--bots-drive`,
`--enemy-distance`, and `--spawn <x>,<z>,<yaw>` (degrees). These are useful
repeatable scenario inputs; they are not account administration, matchmaking,
or durable battle configuration APIs.
