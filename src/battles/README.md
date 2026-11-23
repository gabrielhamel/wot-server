# Battles

The `wot-battles` library publishes its runtime and roster types through `wot_battles::contract`.

Battles owns the live arena from preparation through the departure of the last
participant. It allocates battle IDs, loads arena geometry, spawns vehicles,
advances rounds, resolves vehicle controls and hits, and replicates battle state
to connected players. Active battles and simulation state live in memory.

Player Sessions owns Training Room discovery, invitations, text chat, team
confirmation and the handoff between the garage and the arena. When the room
is ready, it resolves the equipped vehicles through Accounts and submits the
map, duration and assigned roster to Battles. The launch contract can also
accept a roster from a future matchmaker. Training is the implemented battle
mode.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Round timing and phase transitions, vehicle movement, aim, ammunition and hit resolution. |
| `application/` | Participant identity, launch inputs, roster eligibility and typed Avatar and vehicle commands. |
| `infrastructure/` | In-memory arena and participant lifecycle, bundled arena definitions, terrain and obstacle collision, and native battle payload encoders. The delivery port connects those encoders to the local Mercury adapter. |

Callers use [the published contract](src/contract.rs). Player Sessions assigns each
participant the identifier of the current session admission and keeps the
mapping to the live Mercury client. The Mercury adapter translates incoming
methods into typed commands and applies outputs from the arena delivery port.
The runtime holds participant IDs and arena entity IDs. The adapter owns
access to the live BaseApp, including entity creation and packet delivery.
The simulation task in [worker.rs](src/infrastructure/worker.rs) owns live
arena state and the 100 ms tick. A bounded queue carries activation, entry,
Avatar and vehicle commands, and disconnects. A bounded FIFO carries Mercury
deliveries and garage returns to Player Sessions. Position snapshots are sent
as unreliable UDP. Under output pressure the simulation task drops position
snapshots and waits for space to enqueue ordered deliveries. The gateway
disconnects a participant whose command cannot be queued and retries cleanup
as capacity becomes available.

Player Sessions submits the roster and reserved entity IDs to the dedicated
[preparation task](src/infrastructure/preparation.rs). Its bounded request
and result queues are independent of simulation delivery. It validates the
launch, loads terrain and collision geometry, and builds the client payloads
while the gateway handles requests and the simulation runs existing arenas.
Player Sessions revalidates the room and account snapshots before acknowledging
the launch and passing the prepared arena to the simulation task. Both tasks
share the server process. Each queued operation retains its originating span.
The simulation starts the arena clock and increments its active count on
activation.
Once a participant leaves,
Player Sessions resets the connection for garage entry. The Battle ID is
allocated independently of the Training Room ID. At the end of a round,
Battles publishes the outcome and continues pose and chassis replication while
vehicles coast to a stop before the scheduled garage return. Each Avatar receives
its player’s customization display preference at entry.

The server advances hull speed and yaw every 100 ms. It reads the equipped
engine, vehicle mass and chassis resistance from the loadout, and samples the
terrain at four points beneath the chassis. The height map supplies pitch and
roll, while the surface map supplies hard, medium and soft ground. The driver
receives independent track speeds in the Avatar's auxiliary physics property.
Battle peers receive the Vehicle's engine mode and, where the chassis
defines them, its wheel scroll and steering properties. See the
[replay and resource evidence](../../docs/battle-replays.md#vehicle-movement-and-chassis-animation).

| Term | Meaning |
| --- | --- |
| Battle | A live match on one arena with an assigned roster and round clock. |
| Arena | The shared space where the battle runs, identified by its geometry and gameplay variant. |
| Participant | A connected player assigned to a team with a combat or observer vehicle. |
| Participant ID | Gateway-local admission generation used to route battle inputs and deliveries to one specific session. A new login gets a new ID. |
| Round | The progression through loading, countdown, active battle and results. |
| Loadout | The equipped vehicle descriptor, health, crew size, ammunition, locked outfit and base performance at launch. |
| Scene impact | The first projectile contact with terrain or a placed obstacle, reported with its effect material. |
