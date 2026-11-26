# Battles

The `wot-battles` library publishes its runtime and roster types through `wot_battles::contract`.

Battles owns the live arena from preparation through the departure of the last
participant. It allocates battle IDs, loads arena geometry, spawns vehicles,
advances rounds, resolves vehicle controls and hits, and replicates battle state
to connected players. Active battles and simulation state live in memory.

[Training Rooms](../training-rooms/README.md) owns room discovery,
invitations, membership and team confirmation. Player Sessions delivers room
updates and handles the handoff between the garage and the arena. When the room
is ready, Player Sessions resolves equipped vehicles through Accounts and
submits the map, duration and assigned roster to Battles. A dashboard launch submits
the same Training battle mode and can include stationary dummies. Battles
gives dummies a vehicle and a roster row, pre-marks them as ready and keeps
their hulls fixed in vehicle contact resolution. The last player's departure
releases the arena and its dummies.

The [Battle mechanics guide](../../docs/battle-mechanics.md) maps the official
game's rules to the current Training implementation and identifies open work.

| Layer | Responsibilities |
| --- | --- |
| `domain/` | Round timing and phase transitions, vehicle movement and world contact, aim, ammunition and hit resolution. |
| `application/` | Participant identity, launch inputs, roster eligibility and typed Avatar and vehicle commands. |
| `infrastructure/` | In-memory arena and participant lifecycle, bundled arena definitions, terrain and obstacle collision, and native battle payload encoders. The delivery port connects those encoders to the local Mercury adapter. |

Callers use [the published contract](src/contract.rs). Player Sessions assigns each
connected participant the identifier of the current session admission and keeps the
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
share the server process. The gateway assigns native scenery controller IDs
from the same sequence as other arena entities before activation. Each queued
operation retains its originating span.
The simulation starts the arena clock and increments its active count on
activation. The launch carries the arena's [battle parameters](src/domain/parameters.rs).
The Battles contract publishes defaults and bounds so Player Sessions can
validate dashboard overrides before enqueueing preparation.
Once a participant leaves,
Player Sessions resets the connection for garage entry. The Battle ID is
allocated independently of the Training Room ID. At the end of a round,
Battles publishes the outcome and continues pose and chassis replication while
vehicles coast to a stop before the scheduled garage return. Each Avatar receives
its player’s customization display preference at entry. Battles selects each
vehicle’s encoded garage outfit using the arena definition’s camouflage season.

The server advances hull speed and yaw every 100 ms. It reads engine power,
vehicle mass, chassis resistance and supported garage modifiers from the
loadout, and samples the terrain at four points beneath the chassis. The
height map supplies pitch and
roll, while the surface map supplies hard, medium and soft ground. The driver
receives independent track speeds in the Avatar's auxiliary physics property.
Battle peers receive the Vehicle's engine mode and, where the chassis
defines them, its wheel scroll and steering properties. See the
[replay and resource evidence](../../docs/battle-replays.md#vehicle-movement-and-chassis-animation)
and [movement coverage](../../docs/battle-mechanics.md#training-movement-coverage).

Projectile launch and contact are separate. The simulation queues contact
until the tracer has travelled the distance to the selected intersection. The
solver uses the equipped part meshes and plate materials to resolve armor,
screens, modern direct-fire HE, artillery splash and modeled device damage. At contact it applies HP loss if applicable,
sends the outcome and local hit ray, and replicates penetration marks to arena
viewers. See the [replay and resource evidence](../../docs/battle-replays.md#persistent-vehicle-impact-marks).

| Term | Meaning |
| --- | --- |
| Stationary dummy | A vehicle and roster entry with no client admission or Avatar. It can collide and take damage. |
| Battle | A live match on one arena with an assigned roster and round clock. |
| Arena | The shared space where the battle runs, identified by its geometry and gameplay variant. |
| Participant | A connected player assigned to a team with a combat or observer vehicle. |
| Participant ID | Gateway-local admission generation used to route battle inputs and deliveries to one specific session. A new login gets a new ID. |
| Round | The progression through loading, countdown, active battle and results. |
| Battle parameters | The validated round timing and simulation settings carried at launch and used for one arena. |
| Loadout | The equipped vehicle descriptor, health, crew size, ammunition, locked outfit and base performance at launch. |
| Armor plate | A projectile collision surface with a material that specifies thickness, damage factor and applicable angle rules. |
| Damage sticker | A persistent penetration mark replicated on the struck vehicle when the shell defines one. |
| Scene impact | The first projectile contact with terrain or a placed obstacle, reported with its effect material and any native references to fragments broken on impact. |
| Scenery controller | A native `AreaDestructibles` entity that publishes broken objects and tree falls for one map chunk. Its state belongs to one arena. |
