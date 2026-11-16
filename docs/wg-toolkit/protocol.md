# Protocol and entity model

[Map home](README.md) · [Session flows](session-flows.md) · [Verification](verification.md)

## Layers from socket to game behavior

```text
UDP datagram
  → packet framing / encryption / sequence and acknowledgement handling
  → channel state and reconstructed bundle
  → element ID, length, request or reply framing
  → engine message or entity method/property dispatch
  → runtime script value ↔ typed WoT schema
  → account or battle behavior
```

This is a conceptual receive path; the exact filter calls and buffering are
owned by the linked implementations below. Sending takes the corresponding
encode and outgoing-channel path.

| Concern | Source | Contract |
| --- | --- | --- |
| Packet | [packet.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/packet.rs) | `PACKET_CAP = 1472`, four-byte prefix, two-byte flags, flag-dependent footers; not every possible packet flag is fully supported |
| Sequence | [seq.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/seq.rs) | Checked/wrapping protocol sequence arithmetic |
| Reliability and channels | [proto.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/proto.rs) | On/off-channel state, acknowledgement processing, fragment assembly and outgoing reliability |
| UDP abstraction | [socket.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/socket.rs) | Bundle-oriented socket operations |
| Bundle | [bundle.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/bundle.rs) | Elements spanning packets, request/reply framing, read errors and round-trip hooks |
| Element | [element.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/element.rs) | Fixed/variable lengths and compact exposed-ID ranges with overflow sub-IDs |
| Scalar/container codecs | [codec.rs](../references/wg-toolkit-rs/wg-toolkit/src/net/codec.rs), [util/io.rs](../references/wg-toolkit-rs/wg-toolkit/src/util/io.rs) | Wire primitives, variable-size data and address helpers |
| Cryptographic filters | [RSA](../references/wg-toolkit-rs/wg-toolkit/src/net/filter/rsa.rs), [Blowfish](../references/wg-toolkit-rs/wg-toolkit/src/net/filter/blowfish.rs) | Login request encryption and session packet encryption |

Reuse these implementations before attempting new wire codecs. Protocol support
is partial: for example, `proto.rs` explicitly rejects some off-channel ACK
combinations and contains unresolved fragment-size protection work.

## Client-facing interfaces

| Interface | Direction and implementation |
| --- | --- |
| Login | Client ↔ LoginApp; [elements](../references/wg-toolkit-rs/wg-toolkit/src/app/login/element.rs), [server](../references/wg-toolkit-rs/wg-toolkit/src/app/login/mod.rs) |
| Base | Client → BaseApp and its request replies; [elements](../references/wg-toolkit-rs/wg-toolkit/src/app/base/element.rs), [application](../references/wg-toolkit-rs/wg-toolkit/src/app/base/mod.rs) |
| Client | Server → game client; [elements](../references/wg-toolkit-rs/wg-toolkit/src/app/client/element.rs); `app/client` is not a standalone playable game-client implementation |
| Cell | Client → direct CellApp; [partial element codecs](../references/wg-toolkit-rs/wg-toolkit/src/app/cell/element.rs), no standalone CellApp listener |

The base application also hosts spatial entity state and simulation-facing
replication. The emulator intentionally does **not** send `SetCellAppExtAddress`:
the source reports that doing so makes the client wait for a direct cell login
which it cannot answer. Cell methods therefore reach the base connection.
See [enter_battle](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs).

## The game resources define the ABI

[script/load.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/load.rs) reads:

1. `scripts/entity_defs/alias.xml` and interface definitions, including component interfaces.
2. `scripts/entities.xml` and entity `.def` files; client-server entity type IDs follow declaration order, starting at 1.
3. `scripts/components.xml` and component definitions, deduplicated by name.
4. Extension directories in alphabetical order, their `extension.xml`, components, and additional client-server entities.
5. Client-class availability used to distinguish NRL component numbering.
6. Entity `<Parent>` chains, flattened with parent members preceding child members; extension definitions can shadow base definitions.

[parse.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/parse.rs) and
[ty.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/ty.rs) construct the type
system; [value.rs](../references/wg-toolkit-rs/wg-toolkit/src/script/value.rs)
encodes/decodes runtime values.
[dispatch.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/dispatch.rs) builds exposed
method/property tables and creation payload types. Static component methods
are appended after the core method-table sorting. Dynamic components get
independent type slots; NRL's component IDs skip components without client
classes, so the two numeric spaces can diverge.

**Do not hard-code IDs taken from a different build.** Nor should an exposed
method index be confused with its element byte. For this resource set,
`Account.pingMeAndThenJustTouchMe` has exposed index 43 (`0x2B`) but travels as
element `0x79`. See the [generated snapshot](dispatch-snapshot.md).

The extension-order rule is implemented and historically checked against a
specific Account client-method capture. That does not establish every ordering
case across builds; `script/load.rs` itself records uncertainty about base-game
versus extension component folding. Startup loading is also not fully hardened:
some malformed or missing definition structures reach `unwrap()` calls.

## Typed bindings and identities

[entity.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/entity.rs) provides
`entity_schema!`, `component_schema!`, `ScriptType`, typed entity/component keys,
and method/property access. [wot/schema.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/schema.rs)
registers the subset used by the emulator. Binding checks names and types at
startup; it does not implement all methods declared by the game resources.

Keep these identities distinct:

| Identity | Meaning |
| --- | --- |
| Entity type ID | Resource-defined entity/component class |
| Entity ID | Runtime replicated entity instance |
| `EntityKey` / `ComponentKey` | Local generational handle, avoiding stale slot reuse |
| `ClientKey` | Local connected-client handle |
| `SpaceKey` | Local space handle |
| Account database ID | Persistent-player identity in the game contract; currently a placeholder |
| Inventory vehicle ID | Account's owned vehicle instance, currently allocated from 1 |
| Integer item compact descriptor | Item type, nation and in-nation catalog ID |
| Vehicle compact descriptor bytes | A configured vehicle with modules and state |
| NRL node ID | Node in a replicated component tree |

`Account` is the session's hangar player entity. Battle uses an `Avatar` with a
cell presence and a controlled `Vehicle`. Entity ownership, spatial membership,
and per-client visibility are separate concerns.

## Spatial replication

[app/base/mod.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/base/mod.rs) manages
spaces, cell players, entity creation/destruction, property state and witnesses.
At `end_tick`, witnesses emit visibility changes and updates. `Interest` and
`Visibility` control who receives an entity; the WoT emulator adds manual
spotting decisions. [outbox.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/outbox.rs)
batches messages and selects the addressed entity before methods/properties.

Preserve creation and selection ordering, `EnterAoi`/entity-request/creation
lifecycle, `LeaveAoi`, player control, and tick timing. Packed position/direction
formats are in [math.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/math.rs) and
[bit.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/bit.rs). Their comments report
SDK-derived formulas and partial capture evidence, not universal build parity.

NRL replicates dynamic component trees. Wire framing and partial codecs live in
[client/element.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/client/element.rs),
while [nrl.rs](../references/wg-toolkit-rs/wg-toolkit/src/app/nrl.rs) tracks node identity,
fragment assembly, properties and nested changes for decoding. The base app's
typed component API also sends component creation, updates, and removal.
Property-path decoding may require previously tracked container lengths; knowing
only the declared type is insufficient for every nested/slice update.

## Python payloads and unknown data

The [pickle codec](../references/wg-toolkit-rs/wg-toolkit/src/pickle/mod.rs) handles
Python 2 protocols 0–2. Python `str` is bytes, `unicode` is text, and object class,
arguments and state are represented as data. Encode account dictionary keys as
Python 2 strings. Do not replace bytes with Unicode throughout the schema. The current decoder
builds memoized containers in an arena, but converts self-references to `None`
when materializing values (`pickle/de.rs`, `Builder::build`); it does not
preserve arbitrary cyclic Python object graphs losslessly.

Unknown methods can use a variable-length fallback in specific decoding paths;
unknown properties must not guess a length. Historical
[proxy incidents](../references/wg-toolkit-rs/doc/PROXY.md) show why an incorrect
length can misread later bytes as a handoff and cause packet corruption when
address rewriting runs. Preserve explicit error propagation and raw forwarding
independence. A debug placeholder with a length is not a complete semantic codec.
