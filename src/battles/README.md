# Battles

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
| `application/` | Launch inputs and roster eligibility for a two-team battle. |
| `infrastructure/` | In-memory arena and participant lifecycle, bundled arena definitions, terrain and obstacle collision, and the native Avatar, Vehicle and roster adapters over Platform's shared Mercury transport. |

Callers use [the published contract](mod.rs). Preparation validates the roster
and builds the client payloads before Player Sessions acknowledges the launch.
Activation registers the participants for routing. Once a participant leaves,
Player Sessions resets the connection for garage entry. The Battle ID is
allocated independently of the Training Room ID. At the end of a round,
Battles publishes the outcome and continues pose and chassis replication while
vehicles coast to a stop before the scheduled garage return.

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
| Round | The progression through loading, countdown, active battle and results. |
| Loadout | The equipped vehicle descriptor, health, crew size, ammunition and base performance at launch. |
| Scene impact | The first projectile contact with terrain or a placed obstacle, reported with its effect material. |
