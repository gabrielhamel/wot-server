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
the required version property, and `initialServerSettings`. The `showGUI` call
supplies further context. Player Sessions reads the version property's name
and default from the bundled `AccountVersion.def` and validates the Account
creation layout at startup. See the [client breaking changes](../client-breaking-changes.md)
for the observed version field rename.

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

## Training Room visibility

The decompiled EU client checks the global `isTrainingBattleEnabled` setting
when building and refreshing the Training Room item. The setting defaults to
`false`, and the mode selector skips items marked invisible. See
[`helpers/server_settings.py`](../references/wot-src/sources/res/scripts/client/helpers/server_settings.py#L2283),
[`battle_selector_items.py`](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/header/battle_selector_items.py#L956),
and [`mode_selector_data_provider.py`](../references/wot-src/sources/res/scripts/client/gui/impl/lobby/mode_selector/mode_selector_data_provider.py#L171).

The previous account bootstrap omitted that key from `initialServerSettings`,
which explains the missing card. The current
[account bootstrap](../../src/player_sessions/infrastructure/bootstrap.rs#L13)
sends the [`training_room_visible` setting](../server.md#running), which
defaults to `true`. The client reads the same server-wide setting regardless
of the player's inventory, battle count or access level.
Opening Training Rooms requests command `520` with reserved stream ID `200`
and `[2, 7, 0, 0, 50]` for type, sort key, idle filter and page bounds. The
[client Account](../references/wot-src/sources/res/scripts/client/Account.py#L600)
decodes a zlib-compressed pickle of `(prebattle_type, total_count, rooms)`.
The [Training list requester](../references/wot-src/sources/res/scripts/client/gui/prb_control/entities/training/legacy/requester.py#L43)
only accepts type `2`. The [list view](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/trainings_list_base.py#L49)
then closes its waiting overlay, including for an empty list. A generic command
error alone leaves the overlay waiting. Each room row is
`(create_time, room_id, info)`, where the [client list item](../references/wot-src/sources/res/scripts/client/gui/prb_control/items/prb_seqs.py#L29)
reads the numeric `PREBATTLE_CACHE_KEY` fields for the host, map, comment, open
flag, player count and cluster. Rooms are ordered newest first. The list view
counts returned rows and sums their membership. The
[mode selector](../references/wot-src/sources/res/scripts/client/gui/impl/lobby/mode_selector/items/trainings_mode_selector_item.py#L19)
counts returned rows as well. Both views request up to 50 rooms every five
seconds. The server includes the full room count in the stream header tuple.

Rooms marked **By invitation** remain visible with joining disabled. The
[list view](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/trainings_list_base.py#L75)
passes the inverse of `isOpened` to its renderer's `disabled` field. The server
sends the matching flag, and direct join requests for closed rooms receive
`PRIVACY` (10).

This is a source-level diagnosis using the pinned decompiled client revision
`447437a25a2222a0d10dde96f65a20254d3a7d0d`. The registered executable reported
EU `v.2.4.1.1 #993` with SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`.
The garage integration test exercises the protocol with two UDP test peers and
a resource bundle from that build.

## Creating a Training Room

The client sends [`accountPrebattle_createTraining`](../references/wot-src/sources/res/scripts/entity_defs/interfaces/AccountPrebattle.def#L33)
with an arena type, duration in seconds, open flag and comment. The server
checks the arena ID against the selected resource bundle. The low 16 bits
identify the geometry and the high bits identify the gameplay variant. Creation
accepts durations from 5 through 30 minutes in whole minutes and UTF-8 comments
up to 400 characters. Creation is unavailable on bundles without arena definitions.
The server logs a startup warning if the card is enabled and the bundle has no
eligible arenas. A creation attempt then returns `EVENT_DISABLED` (14), which
the client displays as “This event is currently unavailable.” Earlier builds
sent `CANNOT_CREATE` (9), which the EU client translated as “wrong team number.”

The client labels the room comment **Description** in Create Room and
Modify Room. Both dialogs show the field when
`initialServerSettings.pre_moderation_config.prebattleDescriptionEnabled` is
true. The [account bootstrap](../../src/player_sessions/infrastructure/bootstrap.rs#L13)
sets it alongside Training Room visibility. The [settings window](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/TrainingSettingsWindow.py#L133)
also checks the host's comment permission when editing. The key and field
names are present in the installed EU build's compiled client scripts. The
description in the room list tracks the current settings, including edits
sent to room members.

The server sends `onPrebattleJoined`, then `updatePrebattle` with settings (7),
properties (10) and the one-host roster (1). The client creates its prebattle
object on the join event and waits for both settings and roster before switching
to the room. The client automatically sends ready command `402` for the selected
vehicle before opening the view. The server projects an owned vehicle's current
descriptor from the account snapshot, emits player-state update (4), and acknowledges
the command. Not-ready command `403` clears the roster's ready vehicle.
The initial settings include `bonusType = 2` (Training) and shared
`limits[0].maxCount = (20, 30)` for assigned and unassigned roster slots.
The client inherits these limits for both teams. For the Observer toggle it
subtracts five observer slots from the assigned limit, yielding 15 regular
players per team. The server caps the room at 30 members overall and checks
the destination roster's advertised limit on assignment.

On EU `v.2.4.1.1 #993`, the native client log showed that changing back from
Observer raised `KeyError: 'bonusType'` while calculating the regular-player
limit. It also logged `Max count limit` when dropping a player onto a team
that already had a member. Both checks are client-side, before it sends a
command. The [Training Room view](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/TrainingRoomBase.py#L209)
and [TeamLimits defaults](../references/wot-src/sources/res/scripts/common/prebattle_shared.py#L169)
explain these checks and the zero assigned-slot fallback.

The [Observer toggle](../references/wot-src/sources/res/scripts/client/gui/prb_control/entities/training/legacy/ctx.py#L73)
uses ready command `402` with the reserved inventory value `-5000`. The server
sends the stock `ussr:Observer` descriptor from the resource bundle in the same
roster slot. The client's return to Player sends `402` with the current garage
vehicle's inventory ID. The server resolves that owned loadout and updates the
roster before acknowledging either transition. Observer selection is room state
and creates no vehicle ownership. In-battle spectating remains pending.
The [client init flow](../references/wot-src/sources/res/scripts/client/gui/prb_control/entities/base/legacy/entity.py#L207)
and [training entry](../references/wot-src/sources/res/scripts/client/gui/prb_control/entities/training/legacy/entity.py#L369)
show these boundaries.

Invalid creation sends `onPrebattleJoinFailure`, which dismisses the creation
overlay. Repeat requests replay the existing room snapshot. The gateway owns
the room and the roster in memory. Each connected session belongs to at most
one training room, with up to 30 players per room.

The [client joins](../references/wot-src/sources/res/scripts/client/Account.py#L789)
with command `400`, using the selected room and cluster IDs (cluster `0` on
this server). A successful request gets the same initial state as the host,
with the guest role and the shared roster. Existing players receive the
player-added roster update. Guests can change their own readiness and observer selection.
The host controls team assignments, swaps and room settings. Battle launch
remains pending.

Both **Delete Room** and **Exit** go through
[the client's leave action](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/TrainingRoomBase.py#L131)
and send command `401`. The room advertises `destroyIfCreatorOut`, which the
client uses to label the host's button. A host's leave removes the room from
the directory, clears the guest memberships and sends `onPrebattleLeft` to
everyone. A guest leaving only removes that guest and refreshes the room's
roster. The same cleanup runs when a session disconnects or is replaced.

**Exclude from tactical unit** sends
[`CMD_PRB_KICK`](../references/wot-src/sources/res/scripts/client/Account.py#L878)
(`414`) with the guest's Account entity ID and two zeros. The server verifies
that the caller hosts the room and that the target is another member. It
removes the guest from the roster, updates the remaining players, and sends
the guest `onKickedFromPrebattle(10)` and the Training chat close action.
The client uses reason `10` (`PLAYERKICK`) to return to the lobby. The room's
usual join rules apply after removal.

The host moves players between Team 1, Team 2 and the unassigned list through
[`CMD_PRB_ASSIGN`](../references/wot-src/sources/res/scripts/client/Account.py#L809)
(`404`). The arguments are the target player's Account entity ID, the destination roster
slot and group `0`. Assigned slots are `1` and `2`. Unassigned slots retain the
previous team in `17` or `18`. The center button sends
[`CMD_PRB_SWAP_TEAM`](../references/wot-src/sources/res/scripts/client/Account.py#L819)
(`405`) with three zeros. It switches both teams and leaves unassigned players in their current slots.

Both commands send a full roster update (`1`) before acknowledging success.
The [client replaces its roster cache](../references/wot-src/sources/res/scripts/client_common/ClientPrebattle.py#L69)
and refreshes both team lists from this update. Earlier assignments sent an
incremental move (`5`), which depends on the previous slot being in sync.
A same-slot retry refreshes the lists with the current slot. Team moves preserve
the room ID, readiness and the selected tank or observer descriptor.

**Modify Room** sends one [account command](../references/wot-src/sources/res/scripts/client/Account.py#L834)
per changed field. The map (`406`), duration (`407`) and open flag (`408`) use
`doCmdInt3`. The comment (`409`) uses `doCmdStr`. Each command validates and
applies its field independently to the host's active room and publishes it to the roster. A changed field
sends `updatePrebattle(8, pickle((setting_name, value)))` before the command
acknowledgement. The client tracks all acknowledgements before dismissing
“Changing settings…”. Same-value retries succeed without another update.
The room ID, roster and readiness carry over when settings change.

The [voice-chat dropdown](../references/wot-src/sources-as3/gui_lobby/scripts/net/wg/gui/lobby/training/ArenaVoipSettings.as#L38)
sends command `410` with index `0` for General or `1` for Team. New rooms use
General. The server stores the selected mode in the room, publishes
`arenaVoipChannels` through setting update `8`, and acknowledges the request.
The room snapshot includes the selected mode. Voice audio transport remains
outside the lobby implementation.

## Training Room invitations

**Invite** sends [`accountPrebattle_sendPrebattleInvites`](../references/wot-src/sources/res/scripts/entity_defs/interfaces/AccountPrebattle.def#L54)
with a batch of account IDs and one optional UTF-8 message. The room host can
target up to 100 accounts per request with a message of up to 400 characters.
The server deduplicates targets and delivers to players who have synchronized
their garage on this server. Each room keeps a pending invitation per recipient
session. The server applies the client's eight-second cooldown to valid batches.

Delivery uses the client's legacy [`prebattleInvites` update](../references/wot-src/sources/res/scripts/client/Account.py#L1185),
keyed by `(room_id, periphery_id)`. The update includes the host's name and
account ID, message, room type and invitation state. It carries the recipient's
current garage revision as both `prevRev` and `rev`. The client formats the
notification and applies its invitation and ignore preferences. The configured
[`periphery_id`](../server.md#running) must match the server-list entry used by
the recipient. The installed EU build's custom server entry uses `250`.

Accept (`417`) joins the room, including a **By invitation** room, after
checking the recipient has an active invitation. Decline (`418`) consumes the
invitation and updates the notification. Deleting the room or losing the
host expires outstanding invitations. Disconnecting invalidates invitations
for the old recipient session.

## Training Room text chat

After the room settings and roster, the server sends BW_CHAT2
`INIT_UNIT_CHAT` (127) with prebattle type `2` and a zlib-compressed pickle of
an empty message list. The [client's unit chat handler](../references/wot-src/sources/res/scripts/client/messenger/proto/bw_chat2/chat_handlers.py#L243)
opens the built-in **Training** channel through the
[localized channel entity](../references/wot-src/sources/res/scripts/client/messenger/proto/bw_chat2/entities.py#L39).
Joining an open room follows the same path as creating one. Leaving closes
the channel with `DEINIT_UNIT_CHAT` (128). When the host leaves or disconnects,
the server closes it for each remaining player. Existing members receive
player-added and player-removed roster updates so the native channel can
refresh its member list. The action IDs were checked in the installed
EU build's compiled `messenger_common_chat2.pyc`. Its English `chat.mo`
renders the channel name as **Training**.

Clients send `BROADCAST_UNIT_MESSAGE` (129) with UTF-8 text in `strArg1`.
The server derives the room and sender from the authenticated session.
It sends `ON_UNIT_MESSAGE_BROADCAST` (130) to current members, including the
sender, and acknowledges the request separately. The
[message wrapper](../references/wot-src/sources/res/scripts/client/messenger/proto/bw_chat2/wrappers.py#L35)
reads the server timestamp, player ID, text and display name from the
fixed argument dictionary. All roster slots share the channel, including
unassigned players and observers. Messages are limited to 512 characters
and one send per 500 milliseconds.

## Starting a Training Room battle

The native [Start Battle handler](../references/wot-src/sources/res/scripts/client/gui/Scaleform/daapi/view/lobby/trainings/TrainingRoomBase.py#L503)
confirms Teams 1 and 2 in sequence with `doCmdInt3` command `411`. Command
`412` clears a confirmation. The server requires the host to confirm each
team, with at least one ready combat vehicle on each team and a ready vehicle
for every assigned player. Changes to assigned players, their readiness or
room settings clear the confirmations. The client checks the host's current
team state before sending the second request. The server acknowledges the
first request immediately and publishes both ready states in
`updatePrebattle(6)` only when the arena is about to launch.

Once both teams confirm, Player Sessions rechecks vehicle ownership and reads
the current equipped loadouts from Accounts. It submits the roster and room
settings to [Battles](../../src/battles/README.md), which assigns a Battle ID
and prepares the client payloads. Player Sessions then closes the room,
expires invitations and closes the Training text channel. Unassigned members stay in their garages. Each participant
receives `onArenaCreated` followed by `ResetEntities` on the encrypted channel
they already use. The client acknowledges with `EnableEntities`. The server
then creates its Avatar and cell player in the shared space, sends the
geometry mapping, creates the same Vehicle entities for each client, and
attaches the shared roster to each client's AvatarInfo. Leaving or exceeding
the round length resets the player again and reopens the garage after the next
`EnableEntities`. The arena also closes if either team empties.

The wire order and the replicated roster format were cross-checked against
[Toolkit battle bootstrap](../references/wg-toolkit-rs/wg-toolkit-wot/src/base/battle/mod.rs)
and the [native roster component](../references/wot-src/sources/res/scripts/client_common/AvatarVehiclesInfoBase.py).
The in-place reset handshake follows the client-side
[BigWorld reference](../references/BigWorld-Engine-14.4.1/programming/bigworld/lib/connection/server_connection.cpp#L2409).
The installed x64 client's reset handler at `0x142cfc1d0` reaches code that
queues the same interface element registered as `enableEntities` at
`0x1402953e1`. This confirms the acknowledgement in the compiled client.
The native UI handoff still requires a two-client walkthrough.

The round waits for client readiness or a bounded loading timeout, runs a
countdown, and uses the room's selected time limit. Maps need geometry,
bounds, two spawn locations and extracted elevation tiles. The server accepts `vehicle_moveWith` on
the player's Avatar and turret tracking on the player's Avatar or Vehicle.
The target entity must belong to the sending session. The server advances
tanks at 10 Hz and sends one full-ID pose snapshot per tick to both teams.
The snapshot starts with `TickSync` and uses the same channel sequencing
throughout countdown and combat. Turret property changes use the reliable
channel. Movement had also sent reliable copies without a tick timestamp,
which could deliver pose changes at the wrong sample time.

The server fits a hull pose to the terrain under a nominal track footprint
at each spawn and again as the tank drives. Each Vehicle creation and the
corresponding player's cell creation use the same terrain-supported world pose.
The extractor keeps `terrain2/heights` from each outdoor chunk and the map's
`space.settings`. The server interpolates the quantized PNG heights over the
terrain triangles. A missing spawn tile rejects launch, and movement stops at
the edge of available terrain.
See [Avatar movement](../references/wot-src/sources/res/scripts/client/Avatar.py#L1783)
and [client aim tracking](../references/wot-src/sources/res/scripts/client/VehicleGunRotator.py#L368)
for the client inputs. The detailed position frame was checked against
the Toolkit's client element notes for the executable identified below.

The `OwnVehicle` component initializes the local ammo and reload display from
the selected garage loadout. Training uses its own ammunition counters for
the round. A shot consumes one round, applies a three-second reload, and
casts a ray from the server-side gun position. The closest live opponent
intersecting the ray takes up to 75 hit points. Both peers receive the muzzle
event, tracer and health change. Eliminating the last opposing tank finishes
the round. Track dimensions are shared by the training simulation. Vehicle collision,
armour, gun-specific ballistics and spectator controls remain separate work.

The native [battle debug controller](../references/wot-src/sources/res/scripts/client/gui/battle_control/controllers/debug_ctrl.py#L45)
reads ping from `BigWorld.statPing()` and the red indicator from
`BigWorld.statLagDetected()`. In the installed x64 client, the connection
latency path at `0x14041be50` clamps the ping statistic to at least 1 ms
before the stat binding at `0x140ab5500` reads it. The server immediately
acknowledges accepted reliable arena packets so the client can measure
round-trip time.

The native client recorded the Avatar control handoff in the 21 November
2026 training replay, but the replay lacked the periodic Avatar poses. The
x64 packet receiver calls the unreliable sequence validator at
`0x142d4cab4` for external channel traffic. The validator at `0x142d35ef0`
requires a valid 28-bit sequence and rejects older datagrams. Without the
footer, the packet retains the constructor's `0x10000000` sentinel
(`0x142d45ea6`) and fails validation. The server now
adds the sequence footer to all on-channel packets, including standalone
acknowledgements and single-packet snapshots. Unreliable datagrams draw
from a separate allocator so reliability can maintain its own contiguous
sequence. This matches the external channel framing in the
[BigWorld UDP bundle reference](../references/BigWorld-Engine-14.4.1/programming/bigworld/lib/network/udp_bundle.cpp#L432).

The red dot is driven by the lag checks at `0x14041bfd0`. The
[Avatar script](../references/wot-src/sources/res/scripts/client/Avatar.py#L315)
registers its native AvatarFilter through `enableLagDetection(True)`.
The compiled `createCellPlayer` handler at `0x142cf7c20` adds the Avatar
to the locally controlled set. The detailed Avatar pose handler at
`0x142cf7070` returns early for members of that set. Arena entry sends
`controlEntity` (`0x41`) with the Avatar ID and control disabled, which
removes it from that set at `0x142cf7a3c`. The server then supplies
hull-relative Avatar poses and detailed Vehicle poses each tick, including
while parked. Driving and shooting still use Avatar cell methods. The
two-peer protocol test models the compiled client's pose gate. The red dot
still needs verification in the native HUD.

Each roster row starts as loading. `Avatar.setClientReady` publishes a
nested `AvatarVehiclesInfo.vehiclesInfo` update of `isAvatarReady`, followed by
the row's `__generation` commit. Both are required by the
[native roster component](../references/wot-src/sources/res/scripts/client_common/AvatarVehiclesInfoBase.py#L22).
Late arrivals receive current readiness after their roster is created.
Departure clears the ready flag for remaining players.

The bundle used for the two-peer protocol check was extracted from the installed
EU client reporting `v.2.4.1.1 #993`. The launcher executable SHA-256 was
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c` and the
`win64/WorldOfTanks.exe` SHA-256 was
`58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11`.
Publisher discovery reported EU production build `2.4.1.5514`, matching the
installed build documented in [client validation](../client-validation.md).

## Battle entry and return

The sequence below describes the historical Toolkit prototype. At the pinned
revision `f857926b91fa88bcb8e9ddab9a9c29687834682c`, the
[base app](https://github.com/theorzr/wg-toolkit-rs/blob/f857926b91fa88bcb8e9ddab9a9c29687834682c/wg-toolkit-wot/src/base/mod.rs#L477)
uses a timed queue. It gathers reconnecting participants into a shared battle
space and destroys the space when its last client leaves. Queue-info requests
report queue size. The pinned prototype has battle entry and return via the
queue, while Training Room discovery and creation have no corresponding handlers.

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
