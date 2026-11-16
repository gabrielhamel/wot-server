# Session, hangar, and battle flows

[Map home](README.md) · [Protocol model](protocol.md) · [Gameplay](gameplay.md)

## Login and admission

The concrete flow is implemented in [login/server.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/server.rs),
[login/link.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/login/link.rs), and
[base/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs).

1. Client contacts LoginApp, providing login fields and a Blowfish key, with RSA request encryption if configured.
2. Emulator sends a login challenge. It records challenge completion by client address.
3. After completion, it generates a random login key and asks a linked base app to host the client.
4. Base app stores the pending client and acknowledges hosting. Only then does LoginApp return the base address and login key; absence of a base yields `NoBaseApp`.
5. Client logs into BaseApp with that key. Emulator consumes the pending entry and checks the original address, initializes the session and sends update frequency.
6. Client enables entities. The base creates its `Account` and calls `showGUI`.

This is compatibility admission, **not production authentication**: the emulator
accepts logins after its challenge flow, uses a configured player name, and has
no durable credential/account service. Inspect the engine challenge handler
before deciding what validation and abuse controls to retain or strengthen.

## Hangar bootstrap

`enter_account` creates `AccountData` for the entity with `name`, `incarnationID`,
`requiredVersion_2410`, and `initialServerSettings`; `showGUI` supplies further
context. `requiredVersion_2410` is loaded from the client's `AccountVersion.def`.
The field name itself is version-specific even though most dispatch is dynamic.

The client then requests account/shop information. The hangar scene itself is
client-local according to the existing [hangar research](../references/wg-toolkit-rs/doc/HANGAR_LOADING.md):
there is no server-created `HangarVehicle` or required server hangar space.
The server must supply consistent inventory/descriptors/settings so the client
can construct that scene and carousel locally. This audit did not rerun the
original client-script/capture investigation.

### Account command envelope

[wot/account_commands.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/wot/account_commands.rs)
defines command, result, stream, request and token constants, adapted from client
2.4.1 scripts. The existence of a command constant does not imply a handler.

```text
doCmd*(request_id, command, arguments...)
  → onCmdResponse(request_id, result, error)
  or onCmdResponseExt(request_id, result, error, encoded extension)

RES_STREAM
  → ResourceHeader(resource_id = request_id, description)
  → ResourceFragment... (compressed pickle bytes)
```

The toolkit supplies framing/resource transfer; the emulator supplies command
semantics. Preserve correlation IDs, stream descriptions, compression, and cache
revision behavior together.

### Implemented command behavior

The authoritative dispatch is `BaseThread::account_request` in
[base/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs).

| Request | Current answer or effect | Limit |
| --- | --- | --- |
| `CMD_REQ_SERVER_STATS` | Connected-player counts | Process-local, not cluster totals |
| `CMD_SYNC_SHOP` | Cache response when length/CRC match; otherwise resource stream | Shop constants and prices, no purchasing transaction |
| `CMD_SYNC_DATA` | Revision 0 sends full account; later revisions return revision metadata | No durable change history or robust reconciliation |
| `CMD_SYNC_DOSSIERS` | Empty dossiers response with a version | No recorded vehicle statistics |
| `CMD_ADD_INT_USER_SETTINGS` | Sends a settings delta and advances revision | No durable settings storage |
| `CMD_ENQUEUE_IN_BATTLE_QUEUE` | Remembers selected vehicle and emits `onEnqueued` | No real queue matching |
| `CMD_REQ_QUEUE_INFO` | Returns queue info and starts an enqueued client's battle | Immediate local battle trigger |
| `CMD_LOG_CLIENT_XMPP_EVENTS` | Acknowledges when a response is requested | No XMPP service |
| `requestToken` | Emits placeholder token data | No corresponding real external service |
| Chat action | Logs/ignores | No chat delivery |
| `CMD_SET_ACTIVE_VEH_SEASON`, `CMD_RESET_SESSION_STAT` | Success acknowledgment | No substantive effect |
| Other calls | Warning, unanswered | Client operations can remain incomplete |

### Account and shop state

[account.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/account.rs) builds the
full synchronization dictionary: `rev`, `inventory`, `stats`, `potapovQuests`,
and `cache`. Inventory includes configured vehicles, crew, ammunition and loadout
maps, plus required empty tables. Vehicle inventory IDs start at 1. Crew records
use resource-derived passport names/icons and generated compact descriptors.

Every account starts from the same server-wide `AccountData`: 100 million
credits, one million gold, one million free XP, 100 garage slots and 100 berths.
Player name/revision are tracked per session; database ID is the fixed value 1.
These placeholders must not become wot-server's persistent account model.

`Account._update` semantics reported in source distinguish replacement keys
`(name, '_r')` from plain keys merged into existing data. Deltas include
`prevRev`/`rev`. [shop.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/shop.rs)
builds compressed shop data with resource prices and captured EU constants.
Required server settings include disabled/empty feature configurations and
historical dates, not current official configuration.

## Battle entry and return

Implemented in [base/mod.rs](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/mod.rs)
and [battle.rs](https://github.com/theorzr/wg-toolkit-rs/blob/357a7f556a197079c21a371be5718e5211b41eda/wg-toolkit-wot/src/base/battle.rs).

```text
Account / hangar
  → enqueue vehicle → queue-info request
  → onArenaCreated → SwitchBaseApp (same base address)
  → reconnect with session key → enable entities
  → Avatar base player → space properties and geometry mapping
  → own Vehicle → Avatar cell player → bots and arena information/components
  → entity visibility/request lifecycle → vehicle ready
  → setClientReady → 10-second countdown → battle
  → leaveArena → SwitchBaseApp → reconnect → Account / hangar
```

The handoff uses pending switch/return records holding session key, client IP,
Blowfish key, player and vehicle configuration. It is a reconnect to the same
listener in this prototype, not an implemented remote battle-host migration.
`enter_space` sets manual interest, creates each client's own battle world and
populates it from configured bot options. Returning destroys that battle's space.

The loop runs **10 Hz** (`TICK = 100 ms`), catches up overdue ticks and drives
replication. Countdown length is 10 seconds; battle-period metadata advertises
900 seconds. Do not infer a complete timeout/winner/results/reward lifecycle
from that advertised duration: the period scheduler only automatically moves
prebattle to battle in this code.

## What wot-server must preserve

Preserve the order and contracts of admission, enable-entities, account bootstrap,
command synchronization, battle handoff, avatar/vehicle creation, readiness,
ticks and return. Replace prototype state with durable account identity and
explicit session/battle lifecycle. Test transitions as well as steady-state
movement: a correct physics implementation cannot compensate for a missing
bootstrap field or an incorrectly framed response stream.
