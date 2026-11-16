# Installed client validation

## Local installation and server entry

On 2026-11-11 the installed EU client reported launcher build `2.4.1.5514` and
in-game version `v.2.4.1.1 #993`. It runs through the official macOS Wargaming Game
Center's CrossOver/Wine environment. Discover the current local game directory
with `python3 tools/game-client.py path`; see [game client setup](game-client.md).

At the time of validation, the older-named Development reference directory
(`...2.4.1.5498...`) was a symlink to that installation. Its name did not establish the binary version;
the offline mapper's hash-pinned supplied-client test is a separate historical
fixture and must not be described as passing against this newer executable.

The server list is BigWorld Packed XML, not encrypted XML. The configuration was
backed up before modification to:

```
.local/client-backup-20261111-184256/scripts_config.xml
.local/client-backup-20261111-184256/manifest.json
```

The manifest records original/configured SHA-256 hashes and the resolved game path.
All five official EU entries were preserved. An additional entry has:

| Property | Value |
| --- | --- |
| Name | `WG Server Local` |
| Short name | `WGSERVER` |
| URL / token URL | `127.0.0.1:20016` |
| Public key | `loginapp_wg_server.pubkey` |
| Periphery ID | `250` |

ID 205 belongs to EU5 in this installation, so the earlier prototype's 205 example
would collide. The separate public key lives in the game's `res/` directory.
The private PKCS#8 key is `.local/login.pem` (owner-only permissions); it is never
copied into the client or committed. Configuration, keys, traces and screenshots
under `.local/` are deliberately ignored by Git.

From the repository root, start the configured server with:

```sh
./target/release/wot-server
```

No arguments or environment variables are needed. Startup now listens on all
IPv4 interfaces and chooses each client's reachable BaseApp address automatically.
The existing private/public key identity is reused: the saved `.local/login.pem`
is selected directly by `private_key` in `target/release/server.json`, outside
the disposable build directory. See the
[configuration guide](server.md) before moving the executable.

Select the local server entry above. If the launcher presents its official
account, choose **Switch Account** and use **demo@example.test / demo**. Official launcher tokens
are intentionally rejected; they are not local database credentials. The official
launcher may automatically try its current account when the game starts.

To restore the original server list, close the game and copy the preserved file:

```sh
cp .local/client-backup-20261111-184256/scripts_config.xml "$WOT_GAME_DIR/res/scripts_config.xml"
```

Game Center repair/update can replace the modified server list. Recheck the
installed build and preserve the latest configuration before applying it again.

## Confirmed live result

The real client authenticated the PostgreSQL account `DemoPlayer`, received its
Account entity and loaded the garage. At 19:02:44 Europe/Paris the client logged
`HANGAR READY` followed by `VEHICLE LOADING END`. The visible garage showed:

- `DemoPlayer` and the local server's short name in the connection indicator.
- Both owned tanks in the carousel: Leichttraktor (`L.Tr.`) and T1 Cunningham (`T1`).
- The Leichttraktor's rendered 3D model, crew and ammunition. Selecting the
  T1 Cunningham in the carousel also loaded its own 3D model and two crew members.
- 100 million credits, one million gold and one million free XP, matching PostgreSQL.

Local evidence is `.local/garage-leichttraktor.png`, `.local/garage-t1.png` and
`.local/server-live-third.log`; the account/ownership rows were checked directly in
PostgreSQL. No battle, purchase or sale was attempted.

## Compatibility fixes found by this test

### Account synchronization must end with a revision acknowledgement

After a full resource transfer, `AccountSyncData.__onSyncComplete` sets its new
revision and immediately requests synchronization again. An unconditional full
stream makes that loop repeat every client frame and leaves loading stuck.

Access now sends `onCmdResponseExt(RES_SUCCESS, {prevRev, rev})` when the requested
revision equals the current snapshot. A different revision gets a full stream,
followed by the acknowledgement when the client asks again. The encrypted UDP
integration test exercises this second request and checks that it starts no new
resource stream. A later settings revision still yields a fresh snapshot.

Evidence: `docs/references/wot-src/sources/res/scripts/client/account_helpers/AccountSyncData.py`
(`__onSyncComplete`, `__onSyncResponse`, `__sendSyncRequest`) and `SyncController.py`;
confirmed by the live client's repeated resource requests before the fix and garage
completion afterward. The supplied decompilation's line numbers differ from the
installed bytecode, so function names are the stable source pointers.

### Disabled modes still supply startup/replay configuration

The client indexes in-battle configuration keys while initializing BattleReplay,
even outside a battle. Missing `comp7_ranks_config` and `fun_random_config` produced
live KeyErrors. The adapter supplies disabled/empty Comp7, Comp7 Light, Fun Random,
Battle Royale, Epic and vehicle post-progression settings. Epic settings also need
an `epicMetaGame` member: omitting it aborts ServerSettings initialization and
prevents later schema registration.

Source support: core `constants.INBATTLE_CONFIGS`,
`constants_utils.getUsedInReplaysConfigKeys`, `BattleReplay.__onAccountBecomePlayer`,
`helpers/server_settings.py`, and the `comp7`, `comp7_light` and `fun_random`
extensions under `docs/references/wot-src/sources/res/`. The UDP test inspects actual Account
creation data for these externally required keys and Epic's nested member.

## Remaining limitations

Garage completion does not establish support for every lobby feature. The client
still logs handled errors in ancillary achievements, boosters, Epic progression,
Battle Pass settings, Battle Royale widgets, quest weights and notifications.
Some missing/default feature data has a different type from what these controllers
expect. These did not block the observed garage, balances or owned-tank display;
they remain follow-up compatibility work. Battles and progression are outside
this milestone. Two human clients and production throughput are not live-validated.

The computer-control tool exposed Game Center but not the Wine game window. With
explicit user permission, macOS screen capture and native input APIs were used
for the live UI test. The final result is a real client/server connection, not an
image mockup or a synthetic protocol peer.

## RSA identity mismatch recovery (2026-11-11)

A later login failed before database authentication with `rsa::Error::Decryption`.
The configured `target/release/login.pem` had a different public key from the client;
the saved `.local/login.pem` still matched the public key in the client entry
above.
The local configuration now references the saved identity directly, outside build
outputs. The cause of replacement of the build-directory key was not established.

Toolkit `wg-toolkit/src/net/filter/rsa.rs` used `unwrap()` on inbound OAEP/SHA-1
decryption at the original baseline. The server-owned RSA filter returns
`InvalidData`. LoginApp propagates it as `Event::IoError`, which the gateway logs
before continuing.
A UDP regression sends a wrong-key request followed by a valid encrypted request
to the same LoginApp, confirming rejection and recovery without a process panic.

After restoring the saved identity, a synthetic UDP peer using the installed
client's public key logged into the running server as the existing account 33,
received Account/showGUI and downloaded the garage cache with two owned vehicles.
This recovery check did not launch or visually inspect the game client.

## Local login with a public BaseApp address (2026-11-11)

The installed client's `game.log` recorded successful LoginApp handoff through
`127.0.0.1:20016`, followed by all 60 attempts to the configured public BaseApp
address timing out (20:22–20:27 Europe/Paris). This happened with
`player1@demo.demo`; it was a transport failure after authentication.

Endpoint selection now sends loopback peers to `127.0.0.1` and the actual bound
BaseApp port when the listener includes loopback. The configured public override
remains in use for other peers, preserving remote connectivity. Regression tests
cover both paths and concrete non-loopback listener behavior. This is deliberately
a loopback exception: LAN clients using a public override still need a reachable
public/NAT route.

After rebuilding and starting the corrected release binary with the existing public
override, the real Mac client connected as account 33 (Player 1) at 20:30:03
Europe/Paris. Its log recorded `HANGAR READY` at 20:30:11.652 and
`VEHICLE LOADING END` at 20:30:11.773. A separate synthetic probe attempted the
same account afterward and was refused while this real session was active.
No fresh remote game login was performed during this fix.
