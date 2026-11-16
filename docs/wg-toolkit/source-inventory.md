# Complete Rust source inventory

[Map home](README.md) · [Workspace](workspace.md) · [Gameplay](gameplay.md)

All **122 tracked Rust files** in the inspected checkout are listed below,
including examples and test-only modules. Grouping follows the source packages,
not proposed wot-server bounded contexts. This is a responsibility index, not a
claim that every public type or wire message is fully implemented.

Package manifests, upstream documentation and licensing are linked from the
[workspace guide](workspace.md) and [map home](README.md#historical-documentation).

## wg-toolkit

| Source | Responsibility |
| --- | --- |
| [wg-toolkit/examples/model.rs](../references/wg-toolkit-rs/wg-toolkit/examples/model.rs) | Processed-model parsing/inspection example. |
| [wg-toolkit/src/app/base/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/base/element.rs) | Client-to-base engine messages and entity RPC framing. |
| [wg-toolkit/src/app/base/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/base/mod.rs) | Base application, sessions, typed entities, spatial state, witnesses and replication. |
| [wg-toolkit/src/app/bit.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/bit.rs) | MSB-first packed bit IO. |
| [wg-toolkit/src/app/cell/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/cell/element.rs) | Client-to-cell element IDs and partial/debug codecs. |
| [wg-toolkit/src/app/cell/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/cell/mod.rs) | Partial cell-facing codecs; no standalone CellApp listener. |
| [wg-toolkit/src/app/client/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/client/element.rs) | Server-to-client lifecycle, movement, NRL, resource and property codecs. |
| [wg-toolkit/src/app/client/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/client/mod.rs) | Server-to-client protocol module exports; not a game-client runtime. |
| [wg-toolkit/src/app/dispatch.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/dispatch.rs) | Resource-derived entity/component method/property and creation tables. |
| [wg-toolkit/src/app/entity.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/entity.rs) | Typed schemas, binding, keys, properties and method APIs. |
| [wg-toolkit/src/app/login/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/login/element.rs) | Login request/reply/challenge codecs. |
| [wg-toolkit/src/app/login/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/login/mod.rs) | Login application events, challenges and replies. |
| [wg-toolkit/src/app/login_proxy/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/login_proxy/mod.rs) | Login proxy and encryption handover. |
| [wg-toolkit/src/app/math.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/math.rs) | Packed network positions and directions. |
| [wg-toolkit/src/app/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/mod.rs) | src/app module entry point and public exports. |
| [wg-toolkit/src/app/nrl.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/nrl.rs) | Stateful NRL tree tracking, fragment assembly and property updates. |
| [wg-toolkit/src/app/outbox.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/outbox.rs) | Per-client element batching and entity selection. |
| [wg-toolkit/src/app/proxy/capture.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/proxy/capture.rs) | Gzip packet capture format, writer and tolerant reader. |
| [wg-toolkit/src/app/proxy/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/proxy/mod.rs) | Generic transparent proxy transport and replay mode. |
| [wg-toolkit/src/app/slots.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/slots.rs) | Generational storage and stale-handle protection. |
| [wg-toolkit/src/havok/collision.rs](../references/wg-toolkit-rs/wg-toolkit/src/havok/collision.rs) | Havok collision model/body extraction. |
| [wg-toolkit/src/havok/mesh.rs](../references/wg-toolkit-rs/wg-toolkit/src/havok/mesh.rs) | Collision-shape triangle extraction. |
| [wg-toolkit/src/havok/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/havok/mod.rs) | src/havok module entry point and public exports. |
| [wg-toolkit/src/havok/tagfile.rs](../references/wg-toolkit-rs/wg-toolkit/src/havok/tagfile.rs) | Havok TAG0 section/type/object reading. |
| [wg-toolkit/src/lib.rs](../references/wg-toolkit-rs/wg-toolkit/src/lib.rs) | Engine toolkit public library modules. |
| [wg-toolkit/src/model/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/model/mod.rs) | src/model module entry point and public exports. |
| [wg-toolkit/src/model/primitive.rs](../references/wg-toolkit-rs/wg-toolkit/src/model/primitive.rs) | Processed model vertex/index reading. |
| [wg-toolkit/src/model/visual.rs](../references/wg-toolkit-rs/wg-toolkit/src/model/visual.rs) | Processed visual-tree parsing. |
| [wg-toolkit/src/net/bundle.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/bundle.rs) | Element streams over packet bundles, request/reply framing and round-trip hooks. |
| [wg-toolkit/src/net/codec.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/codec.rs) | Generic network wire codecs. |
| [wg-toolkit/src/net/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/element.rs) | Element traits, length framing and exposed-ID ranges. |
| [wg-toolkit/src/net/filter/blowfish.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/filter/blowfish.rs) | Symmetric session encryption filter. |
| [wg-toolkit/src/net/filter/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/filter/mod.rs) | src/net/filter module entry point and public exports. |
| [wg-toolkit/src/net/filter/rsa.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/filter/rsa.rs) | Asymmetric login encryption filter. |
| [wg-toolkit/src/net/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/mod.rs) | src/net module entry point and public exports. |
| [wg-toolkit/src/net/packet.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/packet.rs) | Datagram storage, flags, headers and footer layouts. |
| [wg-toolkit/src/net/proto.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/proto.rs) | Channels, reliability, acknowledgement state and bundle reassembly. |
| [wg-toolkit/src/net/seq.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/seq.rs) | Protocol sequence-number arithmetic. |
| [wg-toolkit/src/net/socket.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/socket.rs) | UDP-backed bundle-oriented sockets. |
| [wg-toolkit/src/pickle/de.rs](../references/wg-toolkit-rs/wg-toolkit/src/pickle/de.rs) | pickle deserialization. |
| [wg-toolkit/src/pickle/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/pickle/mod.rs) | src/pickle module entry point and public exports. |
| [wg-toolkit/src/pickle/ser.rs](../references/wg-toolkit-rs/wg-toolkit/src/pickle/ser.rs) | pickle serialization. |
| [wg-toolkit/src/pickle/value.rs](../references/wg-toolkit-rs/wg-toolkit/src/pickle/value.rs) | pickle value model and conversions/codecs. |
| [wg-toolkit/src/pxml/de.rs](../references/wg-toolkit-rs/wg-toolkit/src/pxml/de.rs) | pxml deserialization. |
| [wg-toolkit/src/pxml/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/pxml/mod.rs) | src/pxml module entry point and public exports. |
| [wg-toolkit/src/pxml/ser.rs](../references/wg-toolkit-rs/wg-toolkit/src/pxml/ser.rs) | pxml serialization. |
| [wg-toolkit/src/pxml/value.rs](../references/wg-toolkit-rs/wg-toolkit/src/pxml/value.rs) | pxml value model and conversions/codecs. |
| [wg-toolkit/src/res/fs.rs](../references/wg-toolkit-rs/wg-toolkit/src/res/fs.rs) | Unified loose-resource and package filesystem. |
| [wg-toolkit/src/res/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/res/mod.rs) | src/res module entry point and public exports. |
| [wg-toolkit/src/res/package.rs](../references/wg-toolkit-rs/wg-toolkit/src/res/package.rs) | Constrained ZIP/package reading and indexing. |
| [wg-toolkit/src/script/def.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/def.rs) | Entity/interface/component declarations and property/method metadata. |
| [wg-toolkit/src/script/load.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/load.rs) | Resource-driven script loading, extensions, components and entity inheritance. |
| [wg-toolkit/src/script/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/mod.rs) | src/script module entry point and public exports. |
| [wg-toolkit/src/script/parse.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/parse.rs) | Definition parsing into the script model. |
| [wg-toolkit/src/script/ty.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/ty.rs) | See module implementation and parent guide. |
| [wg-toolkit/src/script/value.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/value.rs) | script value model and conversions/codecs. |
| [wg-toolkit/src/space/bin/bsmi.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bsmi.rs) | BSMI compiled-space section reader. |
| [wg-toolkit/src/space/bin/bsmo.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bsmo.rs) | BSMO compiled-space section reader. |
| [wg-toolkit/src/space/bin/bwal.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bwal.rs) | BWAL compiled-space section reader. |
| [wg-toolkit/src/space/bin/bwcs.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bwcs.rs) | BWCS compiled-space section reader. |
| [wg-toolkit/src/space/bin/bwsg.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bwsg.rs) | BWSG compiled-space section reader. |
| [wg-toolkit/src/space/bin/bwst.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bwst.rs) | BWST compiled-space section reader. |
| [wg-toolkit/src/space/bin/bwt2.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/bwt2.rs) | BWT2 compiled-space section reader. |
| [wg-toolkit/src/space/bin/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/mod.rs) | src/space/bin module entry point and public exports. |
| [wg-toolkit/src/space/bin/wgde.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/bin/wgde.rs) | WGDE compiled-space section reader. |
| [wg-toolkit/src/space/destructibles.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/destructibles.rs) | Destructible definitions, health, structure modules and dependencies. |
| [wg-toolkit/src/space/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/mod.rs) | Space loading and terrain/static ray, sphere and cuboid contact queries. |
| [wg-toolkit/src/space/statics.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/statics.rs) | Placed static collision triangles and spatial index. |
| [wg-toolkit/src/space/terrain.rs](../references/wg-toolkit-rs/wg-toolkit/src/space/terrain.rs) | Terrain heights/material data and surface queries. |
| [wg-toolkit/src/util/cuckoo.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/cuckoo.rs) | Cuckoo-cycle login challenge utility. |
| [wg-toolkit/src/util/fnv.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/fnv.rs) | FNV hashing utility. |
| [wg-toolkit/src/util/io.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/io.rs) | Engine binary read/write helpers and formatting. |
| [wg-toolkit/src/util/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/mod.rs) | src/util module entry point and public exports. |
| [wg-toolkit/src/util/thread.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/thread.rs) | See module implementation and parent guide. |

## wg-toolkit-wot

| Source | Responsibility |
| --- | --- |
| [wg-toolkit-wot/examples/dump_entity.rs](../references/wg-toolkit-rs/wg-toolkit-wot/examples/dump_entity.rs) | Resource-derived entity/component ID and dispatch-table inspection example. |
| [wg-toolkit-wot/examples/replay_bundle.rs](../references/wg-toolkit-rs/wg-toolkit-wot/examples/replay_bundle.rs) | Single raw-packet decoding diagnostic example. |
| [wg-toolkit-wot/src/base/account.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/account.rs) | Temporary account inventory, crew, stats, cache and compressed synchronization payloads. |
| [wg-toolkit-wot/src/base/armor.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/armor.rs) | Penetration, armour screens, ricochet and legacy explosion damage. |
| [wg-toolkit-wot/src/base/battle.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/battle.rs) | Per-client battle orchestration, inputs, entity setup, simulation events and replication. |
| [wg-toolkit-wot/src/base/bot.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/bot.rs) | Simple enemy-seeking movement and aim controller. |
| [wg-toolkit-wot/src/base/drive_replay.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/drive_replay.rs) | External captured-drive comparison test harness. |
| [wg-toolkit-wot/src/base/gun.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/gun.rs) | Ammunition, reload, magazines, bursts and shell flight/hits. |
| [wg-toolkit-wot/src/base/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs) | WoT session/account orchestration, queue trigger and battle handoffs. |
| [wg-toolkit-wot/src/base/physics/body.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/body.rs) | Rigid-body dynamics, suspension and track force integration. |
| [wg-toolkit-wot/src/base/physics/client.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/client.rs) | See module implementation and parent guide. |
| [wg-toolkit-wot/src/base/physics/collision.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/collision.rs) | Vehicle contact geometry, overlap/sweep and impulse calculations. |
| [wg-toolkit-wot/src/base/physics/contacts.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/contacts.rs) | Vehicle contact shapes and surface samples. |
| [wg-toolkit-wot/src/base/physics/drive.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/drive.rs) | Ground-plane drive, acceleration, braking and turning model. |
| [wg-toolkit-wot/src/base/physics/fixtures.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/fixtures.rs) | Test-only physics vehicle and surface fixtures. |
| [wg-toolkit-wot/src/base/physics/ground.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/ground.rs) | Terrain surface classes and mobility effects. |
| [wg-toolkit-wot/src/base/physics/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/mod.rs) | Battle physics world, vehicles, hulls and integration/collision orchestration. |
| [wg-toolkit-wot/src/base/physics/rubble.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/rubble.rs) | Per-battle destructible damage, broken pieces and breakage events. |
| [wg-toolkit-wot/src/base/physics/turret.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/turret.rs) | Turret/gun traverse, ballistic aiming and dispersion. |
| [wg-toolkit-wot/src/base/physics/walls.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/physics/walls.rs) | Map-edge constraints, static corrections and ram lookahead. |
| [wg-toolkit-wot/src/base/shop.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/shop.rs) | Compressed shop payload and cache length/CRC checks. |
| [wg-toolkit-wot/src/cluster/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/cluster/mod.rs) | Combined-process command modules. |
| [wg-toolkit-wot/src/cluster/proxy.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/cluster/proxy.rs) | Starts the combined login and base proxy threads. |
| [wg-toolkit-wot/src/cluster/server.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/cluster/server.rs) | Starts the combined login and base server threads. |
| [wg-toolkit-wot/src/link.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/link.rs) | Local/TCP cluster links, messages and connection events. |
| [wg-toolkit-wot/src/login/link.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/link.rs) | Login-side linked-base registry and placement request handling. |
| [wg-toolkit-wot/src/login/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/mod.rs) | Login app selection and startup. |
| [wg-toolkit-wot/src/login/proxy.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/proxy.rs) | Real-login forwarding and base-proxy handoff. |
| [wg-toolkit-wot/src/login/server.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/server.rs) | Prototype login admission and pending base-host handoff. |
| [wg-toolkit-wot/src/main.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/main.rs) | CLI declarations, configuration and command dispatch. |
| [wg-toolkit-wot/src/proxy/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/proxy/mod.rs) | WoT traffic decoding, session state and base proxy. |
| [wg-toolkit-wot/src/proxy/python.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/proxy/python.rs) | Interprets account commands, token replies and resource payloads in proxy logs. |
| [wg-toolkit-wot/src/replay.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/replay.rs) | Offline capture decoding and optional round-trip comparison. |
| [wg-toolkit-wot/src/wot/account_commands.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/account_commands.rs) | WoT account-command, result, request, token and stream constants. |
| [wg-toolkit-wot/src/wot/arena_defs.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/arena_defs.rs) | Map definitions, type IDs, bounds, geometry and spawn data. |
| [wg-toolkit-wot/src/wot/collision.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/collision.rs) | Vehicle-part hit geometry, armour-material lookup and triangle tests. |
| [wg-toolkit-wot/src/wot/item_defs.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/item_defs.rs) | Vehicle/module/shell catalogs and simulation parameters. |
| [wg-toolkit-wot/src/wot/item_prices.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/item_prices.rs) | Resource-derived shop item prices and availability. |
| [wg-toolkit-wot/src/wot/items.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/items.rs) | Item, configured vehicle and crew compact descriptor codecs. |
| [wg-toolkit-wot/src/wot/mod.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/mod.rs) | src/wot module entry point and public exports. |
| [wg-toolkit-wot/src/wot/schema.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/schema.rs) | Typed entity/component schemas used by the emulator. |

## wg-toolkit-cli

| Source | Responsibility |
| --- | --- |
| [wg-toolkit-cli/src/havok.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/havok.rs) | CLI Havok inspection. |
| [wg-toolkit-cli/src/main.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/main.rs) | CLI declarations, configuration and command dispatch. |
| [wg-toolkit-cli/src/pxml.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/pxml.rs) | CLI Packed XML printing/editing. |
| [wg-toolkit-cli/src/res/dokan.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/res/dokan.rs) | Optional Windows resource filesystem mount. |
| [wg-toolkit-cli/src/res/fuse.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/res/fuse.rs) | Optional Unix resource filesystem mount. |
| [wg-toolkit-cli/src/res/mod.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/res/mod.rs) | src/res module entry point and public exports. |
| [wg-toolkit-cli/src/space.rs](../references/wg-toolkit-rs/wg-toolkit-cli/src/space.rs) | CLI compiled-space inspection. |
