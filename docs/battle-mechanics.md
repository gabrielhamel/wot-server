# Battle mechanics

This guide maps the World of Tanks PC mechanics described in the Wargaming
global wiki article *Battle Mechanics*, revision 350583, last edited
2024-10-13 and captured on 2025-03-03. The article combines game rules,
player measurements and rules from earlier releases. The historical models
below preserve the useful relationships and identify what still needs
verification on the latest EU production client.

The server coverage below describes the current Training implementation.
**Partial** means the server implements part of the mechanic or uses an
approximation. **Open** means the arena lacks the gameplay rule. A garage
feature, resource field or client effect can exist independently of the
server rule that drives it. See the [Battles context](../src/battles/README.md)
for runtime ownership and [Battle replay research](battle-replays.md) for
observed client behavior and wire evidence.

## Battle flow and progression

An [admin launch](admin.md#launch-a-training-battle) uses the Training arena
with an assigned roster. Stationary dummies start ready, appear in the arena
roster and participate in collision, damage and team elimination. Dummies
have fixed hull positions and stay in the arena until the round ends or all
connected participants leave. The native roster marks them as bots with an
empty `avatarSessionID`, matching `PlayerInfoVO.isBot` in the client reference.


| Mechanic | Game mechanic | Server coverage |
| --- | --- | --- |
| Base capture | Occupying an objective can accrue capture progress and decide a round. The objective and victory conditions depend on the mode. | **Partial.** Training supports team elimination, departure-based outcomes and a time-limit draw. Capture zones, progress, resets and capture wins are open. See [round state](../src/battles/src/domain/round.rs) and [outcome delivery](../src/battles/src/infrastructure/mod.rs). |
| Battle communication | Players can send contextual requests and ping vehicles or map locations. See [Wargaming's communication guide][eu-communication]. | **Open.** The Training channel handles room text chat in the garage. In-arena text, quick commands and contextual pings are separate workflows. See [Player Sessions](../src/player-sessions/README.md). |
| Matchmaking | Random Battles assemble teams using tier, vehicle role, class limits, and queue and map policies. Wargaming has revised those policies since the snapshot. See the [2.0 matchmaker notes][eu-matchmaker] and [update history][eu-history]. | **Open.** Player Sessions hands an assigned Training roster to Battles. See the [launch contract](../src/battles/src/contract.rs). |
| Tank stats | Equipped modules define base characteristics. Crew, equipment, consumables and directives can modify applicable stats. The archive's crew formulas and examples need separate verification against the current crew system. See the [EU crew rework][eu-crew]. | **Partial.** The launch profile includes base mobility, gun and ammunition data derived from the equipped modules. Supported passive modifiers apply during projection, including `miscAttrs/healthFactor` from equipment and matching slots. Full battle modifier calculation and application remain open. See [profile extraction](../src/accounts/src/infrastructure/client/battle_performance.rs), [crew](crew.md), [equipment](equipment.md), [consumables](consumables.md) and [directives](directives.md). |
| Experience and credits | Eligible battle modes award credits and progression based on the result and contributions such as damage and assistance. Combat XP, Free XP and Crew XP have distinct destinations and rules. | **Partial.** Accounts has a rule to route trusted Combat and Crew XP awards. Training publishes a round outcome and returns players to the garage. Battle award calculation, persistent settlement, repair charges and resupply deductions are open. See [Battle XP routing](crew.md#battle-xp-routing). |

### Effective vehicle stats

The archive distinguishes displayed stats from effective stats in battle.
The historical calculation picks the crew role responsible for the stat and
averages effective skill when several crew members fill that role. The commander
adds a bonus to the other crew members. For a role at effective skill percentage
`s`, the wiki's examples use the multiplier `(0.5 + 0.00375 × s) / 0.875`.
Multiply a stat that improves by increasing, such as view range. Divide a stat
that improves by decreasing, such as reload duration. Treat the multiplier and
the commander's historical 10% contribution as research leads.

Direct modifiers such as optics or a rammer change a specific stat. The archived
equation places the equipment contribution after crew scaling. Ventilation,
food and crew perks can act through crew efficiency. Geometry and mass are
separate from role skills. The Loader affects the magazine reload in the
archived model, while the mechanism sets the interval between shots in the
magazine. Crew averaging, modifier order, equipment activation conditions and
the client UI's rounding all matter when comparing effective stats.

### Battle economy

The article's historical reward table separates participation, combat,
objectives and settlement. Training is excluded from those reward rules.

| Event family | Inputs described by the article |
| --- | --- |
| Participation and spotting | Credits for entering an eligible battle, scaled by tier. First detections generate rewards, with a larger award for spotting an SPG. |
| Direct and assisted damage | Damage from hits, splash, fire, ramming or ammunition rack explosions contributes. The archive distinguishes self-spotted, unspotted and ally-spotted targets. A portion of the reward for assisted damage is shared among eligible spotters. Combat XP also depends on the target's tier and type. |
| Criticals and kills | The archive awards XP for disabling a module or incapacitating crew, with special treatment for repeated damage after repair. It lists an XP bonus for the killing shot and for close-range damage. A vehicle disabled by full crew loss is treated specially when crediting its remaining HP. |
| Objectives and team result | Successful capture, capture defense, survival, team damage dealt, victory and special awards can affect settlement. The archive lists separate XP and credit treatments and historical rules for organized modes. |
| Modifiers and penalties | Vehicle and premium-account modifiers, first-victory bonuses, early departure and historical team-damage penalties affect the payout. Vehicle repairs and restocking are separate expenses. |

Combat XP accrues to the vehicle and is used for its progression. Free XP is
an additional cross-vehicle balance that can also be used for crew training.
The article gives a historical base award of 5% of earned XP and describes
gold conversion from eligible Elite vehicle XP. Crew XP goes to the
participating crew, subject to training and award modifiers. Current reward
rates, exceptions and mode-specific settlement need independent evidence.

## Visibility and replication

| Mechanic | Game mechanic | Server coverage |
| --- | --- | --- |
| Draw distance and tracers | Local rendering, the server's vehicle draw radius, minimap knowledge and tracer visibility are separate. [Wargaming Player Support][eu-spotting] gives a 565 m vehicle draw circle. The archive lists 564 m and describes historical camera-dependent rules for tracers from hidden vehicles. | **Partial.** Training supplies known sources for every viewer. Tracer delivery enforces draw range and remembers recipients for tracer stops. The policy also accepts hidden sources, a direct-view direction, or strategic viewport containment. Camera and spotting providers for the hidden-source path are open. See [vehicle replication](../src/battles/src/infrastructure/controls.rs) and [shot delivery](../src/battles/src/infrastructure/projectiles.rs). |
| View and spotting range | View range is an input to detection. A vehicle can spot an enemy itself up to 445 m, with proximity detection within 50 m even through cover. Extra view range helps detect concealed targets within the cap. See [Wargaming Player Support][eu-spotting]. | **Open.** There is no detection state or per-viewer enemy discovery. |
| Visibility checks and camouflage | Beyond proximity range, detection depends on line of sight and concealment. The archive describes view ports and target checkpoints, changes to concealment from movement, firing and vegetation, and how long a vehicle stays spotted. Its check intervals are explicitly flagged as outdated after 9.16. See [Wargaming's visibility system changes][eu-cadence]. | **Open.** Terrain and obstacle queries are used for projectile contact. Exterior customization is carried with the arena loadout, with no corresponding spotting or concealment calculation. See [garage camouflage](camouflage.md) and [scene impacts](battle-replays.md#scene-impacts-and-gun-audio). |
| Radio range | Allies within signal range share spotted positions, which can appear on the minimap beyond the recipient's vehicle draw range. See [Wargaming Player Support][eu-spotting]. The combined-range formula and relay behavior in the archive need current-client verification. | **Open.** The arena has no signal-range or shared-spotting model. |

### Detection, concealment and delivery

1. **Calculate the observer's view range.** The archive combines the fitted
   turret and applicable crew, perks and equipment. For an observer view range
   `V` and target concealment fraction `C`, the archived relation before caps is
   `V - (V - 50 m) × C`. Apply the 50 m proximity rule and 445 m self-spotting
   cap separately from the draw radius.
2. **Resolve concealment and line of sight.** The archive combines vehicle
   concealment for stationary and moving states, the crew skill contribution
   and a vehicle/gun firing penalty, then adds applicable exterior, stationary
   equipment and foliage bonuses. Foliage can stack up to a cap. A shot
   temporarily reduces the contribution of nearby foliage, with the wiki
   describing a 15 m boundary and separate handling of foliage outside it.
   In its model, firing retains only the strongest nearby foliage contribution
   at 30% of its original value, with more distant foliage unchanged.
   Opaque terrain and buildings block sight beyond proximity range.
   Wargaming support distinguishes hull movement from turret and gun traverse
   when explaining concealment. [Wargaming Player Support][eu-spotting]
3. **Test the view ports.** The archived model casts sight rays from a high
   fixed port or a gun-mount port to six checkpoints on the target. They
   cover the turret top, gun mount, hull front and rear, and both turret
   sides. One clear ray is sufficient inside the calculated spotting range.
   The archive treats other vehicles and wrecks as transparent for spotting.
4. **Track spotted state.** The archive gives a 10-second baseline linger
   after contact is lost, with crew and equipment effects on the duration.
   Its old distance-band intervals of 0.1, 0.5, 1 and 2 seconds are historical.
   Verify cadence, port scheduling, terrain and foliage occlusion, and
   spotting retention independently. [Wargaming's visibility changes][eu-cadence]
5. **Deliver the applicable information to each viewer.** Separate self-spots,
   allied reports, minimap markers and 3D vehicles. The archive models sharing
   using the sum of two allies' signal ranges and excludes relaying secondhand
   contacts. It describes friendly and spotted-enemy tracers within the draw
   limit, a restricted forward cone for some unspotted tracers, and separate
   rules for the SPG strategic viewport. These sharing and tracer filters need
   controlled verification before implementation.

The archived concealment calculation scales the stationary or moving base
value by `0.5 + 0.00375 × effective concealment skill` and by the shot
multiplier, then adds exterior, active camouflage equipment and foliage.
The result is capped at 100%. Verify the modifier order, foliage weights
and caps, equipment activation and firing penalties against the current
client. Historical equipment configurations and individual vehicle examples
illustrate the calculation.

## Movement, aiming and projectiles

| Mechanic | Game mechanic | Server coverage |
| --- | --- | --- |
| Movement and physics | Engine power relative to mass, suspension, surface resistance and slope govern acceleration and traverse. Forward and reverse limits constrain powered speed. The article also covers physical interactions such as falling and drowning. | **Partial.** Training resolves mounted mobility modifiers, simulates terrain-aware driving and replicates vehicle pose and chassis motion. See [Training movement coverage](#training-movement-coverage). |
| Aiming | Player aiming includes arcade and sniper views, target lead, auto-aim, penetration feedback and a strategic view for SPGs. Gun traverse, elevation and flight time affect where a shot can land. | **Partial.** The server accepts aim points and angles, advances the gun within traversal and pitch limits, and publishes an optional server marker. Target-tracking auto-aim, armor-derived feedback and the SPG trajectory workflow are open. See [turret and reticle evidence](battle-replays.md#turret-and-reticle-replication). |
| Accuracy and dispersion | The aiming circle represents dispersion. Movement, traverse and firing can widen it, and it contracts as the gun settles. The server's shot distribution determines the shot direction within that model. | **Partial.** The marker uses the gun's base dispersion value. The shot follows the center ray, with no dynamic bloom, settling model or sampled dispersion. See the [vehicle model](../src/battles/src/domain/vehicle.rs). |
| Ballistic trajectory | Flight time and projectile drop affect target lead and the ability to hit over terrain. Shell types and gun definitions can differ in range and velocity. | **Partial.** Direct-fire shots follow the gun ray at the shell’s configured velocity. SPG shots use the shell’s configured gravity and bounded curved segments. The tracer receives the matching velocity, gravity and effective range. The solver selects contacts from current poses and schedules the impact by travel time. Collision against a target moving during flight is open. See [projectile resolution](../src/battles/src/infrastructure/projectiles.rs) and [range expiry](battle-replays.md#range-expiry). |
| Damage per shot and per minute | Nominal damage per shot describes one hit. Sustained damage also depends on the gun's firing cycle, including time between shots within a magazine and the time to replenish it. | **Partial.** Training tracks rounds and supports single-shot, magazine and autoreloading cycles, plus shell selection and manual magazine reload. Live damage and reload modifiers can change actual sustained output in the official game. See [ammunition state](../src/battles/src/domain/ammunition.rs). |

### Driving and gun control

- The archive's traverse model scales the nominal yaw rate by ratios for
  installed versus stock engine power, hard-ground versus current resistance,
  and stock versus current mass. It also distinguishes turning about the
  vehicle center from steering around a track. Compare those relationships
  against speed and turn-rate measurements on each surface. The physics
  section describes traction loss, sideslip, handbrake turns, steep grades,
  airborne motion, rollover, vehicle pushing and water-depth hazards.
- Arcade view aims from a raised third-person camera. Sniper view aims from
  near the gun with magnification. That separation can change what the
  camera sees around an obstacle and what the gun can reach. Auto-aim
  maintains a lock on a visible vehicle and follows an aim point on it,
  with manual lead still needed for motion. The gun's ballistic path can
  clear an obstacle that blocks the camera line of sight. The archive
  describes automatic elevation correction while aiming at a highlighted
  target and a projected dispersion ellipse for strategic SPG aiming.
  The article's historical penetration indicator compares the selected
  shell's penetration range with the plate at the aim point. An aim-point
  preview can differ from the actual hit because of dispersion, incidence
  angle and intervening surfaces.
- Dispersion is specified at 100 m and the corresponding footprint grows
  with range. Firing, hull motion, hull turn and turret traverse cause
  temporary bloom. Gun damage and gunner injury also affect accuracy. The
  archive describes aim time as the interval for the circle to shrink to
  one-third of its size, and gives a historical center-weighted shot model
  with a separate roll for shots outside the circle. Measure the current
  distribution and settling curve before using these details.
- The client reticle, server marker, rendered tracer and authoritative
  collision describe different stages of a shot. The server marker reflects
  the latest aim state received by the client. Latency and prediction can
  make the views diverge. Compare the command, gun pose, tracer and impact
  together when validating the gun path.
- For a fixed magazine of `N` rounds, the steady-state cycle is the full
  magazine reload plus `N - 1` intra-magazine intervals. Nominal sustained
  DPM is damage per shell multiplied by rounds per minute. Burst damage
  depends on how many rounds are available for the current engagement. The
  archive also describes a first-shot comparison that credits an already
  loaded gun at the start of the time window.

### Per-battle simulation parameters

Battles owns the defaults and ranges in
[parameters.rs](../src/battles/src/domain/parameters.rs). The admin dashboard
can override round timing, gravity and drive response, hull contacts and
rollover, ballistic integration, armor and device damage, fire and tracer
visibility. The validated values travel with the battle launch and are shared
by simulation and replication for that arena. The existing values are the
defaults used by native Training Rooms.

The native movement bitmask identifies commands. Shell ballistics and equipped
vehicle performance come from the resource bundle. The tracer rule for hidden
shooters is available to the simulator, while current Training replication
treats shooters as known.

### Training movement coverage

Accounts resolves equipped engine and module mass, chassis and terrain
parameters, optional-device mass and applicable equipment category bonuses
when preparing the Training roster. The profile includes the resource-defined
power and speed effects of turbochargers and experimental mobility equipment,
rotation mechanisms, grousers, passive fuel and applicable turbocharger and
ventilation directives. Ventilation, food and Brothers in Arms contribute to
the seated driver's terrain-resistance calculation. Clutch Braking,
Off-Road Driving and Engineer contribute with the current driver's trained
perk level and perk efficiency. See [loadout resolution](../src/accounts/src/infrastructure/client/battle_mobility.rs)
and [module extraction](../src/accounts/src/infrastructure/client/module_defs.rs).

Battles combines the fitted engine and mass ratios with the chassis traverse
rate and terrain resistance. It applies the chassis grade limit to uphill
traction and supports centered rotation, single-track
pivots, wheel steering, downhill overspeed, track-lock steering on eligible
tracked classes, and airborne motion and landing against the terrain height
map. The drive model uses approximate traction, braking, track-lock and
takeoff responses. The client speedometer admits up to 1.5 times the listed
speed in the supplied client reference. Training uses that ratio as a downhill
simulation bound. It still needs a measured EU downhill curve. See
[movement simulation](../src/battles/src/domain/movement.rs),
[terrain pose](../src/battles/src/domain/vehicle.rs),
[replay evidence](battle-replays.md#vehicle-movement-and-chassis-animation)
and [Wargaming's movement physics notes][eu-movement].

#### Scenery contact, pushing and recovery

Training sweeps an oriented hull footprint against wall-like triangles in the
arena collision bundle. It stops the component of motion into the obstruction
and allows glancing movement along the surface. Arena variants use their
configured collision visibility mask. Within the chassis's local step range,
upward scenery triangles can support the hull and affect pitch and roll.
Bridges and ramps can carry vehicles above the terrain while a vehicle below
a deck can continue on the underlying surface. Bundles without collision
placements still support terrain and tank contact.

Hull sweeps use the loaded Havok chassis and mounted hull bounds on the X/Z
plane, including rear overhang and any offset from the vehicle origin. Track
dimensions remain the fallback when the bundle lacks those bounds. The tank
keeps its own movement and replication origin. The AMX 30 B in the recorded EU
resources has a track half-length of about 2.04 m and its chassis reaches about
3.33 m behind the origin.

The extractor joins placed Havok models to the destruction definitions and
uses the compiled map's `WGDE` section to identify each object by chunk and
index. Placements that share that identity share fragment health within an
arena. When a fragment breaks, its transitive dependencies break in the same
simulation step and the whole set stops blocking hulls or projectiles.
Independent parts remain in place. Havok bodies can allow shells or vehicles
to pass independently. In Widepark, the vehicle-only bodies for the street
lamps trigger a fall, and the center locomotive's vehicle-only walls block
tanks and share damage with the corresponding shootable modules. Authored
vehicle-only walls on carriages and locomotives keep blocking when their
visual fragments break. This is Training policy pending a native check
after destruction. The [Widepark capture](battle-replays.md#widepark-training-capture-from-2026-11-23)
locates the stock Leopard 1 stop beside a flat carriage.
Numbered placements scale base fragment
health by the square of the model scale. Drive contact estimates scenery damage
from mass and contact speed. At each angular sample, the solver compares
horizontal penetration of the same surface before and after rotation.
This detects a corner entering a fence panel even when the overall hull
projection toward the contact normal decreases. Steering alone can damage
such a wall, and a turn that clears an existing contact can continue.
Powered input adds sustained pressure at contact even while the hull is
stationary. Breaking scenery consumes a bounded share of the assumed impact
energy, so a light vehicle can lose more speed than a heavier one at the same
approach speed. The server deducts 30% of the share of impact energy required
for the break, after accounting for engine pressure. This caps the speed loss
at about 16.3% per fragment. The active EU client's
`scripts.pkg/scripts/client/Vehicle.pyc` confirms the impact damage
coefficient in `_isDestructibleMayBeBroken`. The method returns a fracture
prediction. Toolkit's `base/physics/rubble.rs` uses a separate speed-loss
fit from a Type 64 capture, with a coefficient applied to fragment damage and
normalized vehicle mass.

The server's previous 35% energy deduction was introduced in commit
`5c09940`. A synthetic test using the extracted stock TKS 20 mass of 2,800 kg
and the 10 health of Widepark panel 45 in chunk `0x817e` reproduced a stall
after two breaks at an entry speed of 8.5 m/s. With the current 30% deduction
the TKS breaks three at that speed, four at 9.8 m/s and all five at
10.3 m/s while losing speed. Sustained pressure and slowdown through an actual row of panels
remain open for native calibration. Shell impacts use the configured shell damage.
Contact with scenery or another tank leaves tank HP unchanged.

The server publishes broken modules, fragile props and fallen columns through
the native `AreaDestructibles` entity for the affected chunk. Live updates
trigger the client's destruction and debris effects. For shell impacts,
`Avatar.explodeProjectile` also carries each break's native reference when its
object index fits the method's eight-bit field. Larger indices are carried by
the area property and use the client's delayed effect fallback. Arena entry
includes a snapshot of already broken objects. The client applies that
snapshot without replaying the initial effects.

Numbered trees use the same chunk controllers. Extraction joins each
`WGDE` entry with the tree's `SpTr` placement and destruction definition.
Drive contact can break a standing trunk with impact energy or sustained
engine force. The Rust solver uses a scaled trunk proxy for vehicle contact.
Shells pass through standing trunks, as in the Toolkit model. Bushes listed
in the vegetation catalog remain pass-through vegetation. Fallen trees no
longer block hulls in the server. The native `fallenTrees` record carries the
object index, the maximum pitch, the fall direction and the initial speed
step. The server estimates the pitch limit against terrain and standing
scenery. The client animates the fall and applies the settled state on entry.

The extractor reads Havok collision data for placed models through its
`tagfile`, `collision` and `mesh` modules. Tree placements come from SpeedTree
data. Server contact tests run in Rust. The native client runs its own
destruction effects.

Tank contact sweeps the full chassis and mounted hull bounds in three
dimensions, using each tank's current pitch, roll, and yaw. Contact starts
at the bounds without extra tank-to-tank clearance. Translation during the
sweep is horizontal. The server alternates pair order across bounded solver
passes. Installed vehicle and equipment mass determines how much each tank
moves. An unattended tank can be pushed, while a wreck acts as an immovable
hull. The pushed hull can damage scenery with its resulting motion and engine
pressure transmitted along the contact normal. The solver checks the proposed
displacement against the remaining scenery and arena bounds in the same tick.
If a pileup still overlaps after the solver budget, the affected group holds
its previous horizontal pose for that tick. Other groups can continue moving.

On steep surfaces, gravity can overcome material-dependent grip. The vehicle
then slides downhill even without a drive command. Side slopes, hard turns
and lateral contacts can tip a hull onto its side. A tipped vehicle cannot
drive or fire. A push from the appropriate side can right it on stable
ground. Training also uses a three-second settling period and a 15-second
automatic recovery countdown on stable ground. Movement restarts the wait.
This is Training server policy modeled on Wargaming's published recovery
rules, which introduced auto-flipping in Random Battles. [EU recovery notes][eu-recovery]

The Lakeville bundle regression starts two tanks facing each other and runs
a drive command through the arena tick. It pushes the parked tank through a
numbered wall and verifies that both tanks advance and retain their health.
Both viewers are queued the updated positions and fracture under the
initiating drive trace. The same regression checks a separate steering
fracture and resulting tree fall. The Serene Coast
bundle regression samples the `hd_env_UNI_036_Bridge` deck near
`(-143.73, 102.71)` in world X/Z and checks that the ground below stays a
separate driving surface. Synthetic regressions cover ramp traversal,
partial structure damage, independent rounds, a ten-tank pileup, and AMX 30 B
bounds meeting on a slope. The powered push test covers repeated pressure
against a breakable wall and a solid wall that holds both tanks.
The fresh bundle from the recorded executable produced 806 collision models
with destructible triangles, including 25 with cascade dependencies. The
Ensk regression traces a ray into a real numbered fragment, confirms its
dependants disappear from arena collision state, and verifies a fresh arena
keeps the geometry intact. The wire regression decodes live fracture arrays
with the installed EU client's `AreaDestructibles.def`. A fresh extraction
from that executable found 269,309 numbered trees across 70 layouts,
including 4,739 in Malinovka. The Malinovka regression checks a standing
trunk's hull contact and shell passthrough, native ID, loss of collision on
breakage, and fresh-arena reset. The wire regression checks the five-byte
tree fall record, including the captured pitch bytes documented in the
Toolkit's `wg-toolkit-wot/src/base/battle/areas.rs`. The Lakeville
regression checks that the scenery update is queued for both peers
under the command that caused it, and that arena entry queues the broken
state. The exported trace also records a closed delivery queue as an error.

The EU executable launched on macOS through Wargaming.net Game Center, which
reported build `2.4.1.5514` as current. The Wine game window was unavailable to
the automation. Live observation of tree falls, debris, model changes and entry snapshots
in a native Training battle remains pending. A Training player confirmed
placed object destruction and tree falls, then reported passing through
Widepark street lamps and the center train and backing partly into a wall
with the AMX 30 B. The refreshed bundle regression checks the numbered
Widepark lamp and locomotive, and the AMX 30 B contact envelope and reverse
sweep have dedicated tests. Those three fixes still need a native retest.
The protocol evidence comes
from the extracted EU entity definitions, the supplied
`AreaDestructibles.py`, `DestructiblesCache.py` and `Avatar.py` references, and
the Toolkit's matching captured battle in its `WGDE` decoder and
`wg-toolkit-wot/src/base/battle/areas.rs`.

| Remaining gap | What is missing |
| --- | --- |
| Scenery and ground contact fidelity | The solver uses chassis and hull envelopes for tank contact, an estimated height for the scenery sweep and upward triangles for support. Dedicated movement materials, exact suspension contact, continuous angular sweeps, airborne collision and fully coupled 3D pileups remain open. |
| Destruction coverage and fidelity | Unnumbered scenery lacks native synchronization. Trunk shapes, contact loads and pitch limits are estimates. Fall collisions with tanks and other trees, authoritative debris motion, and foliage concealment from standing and fallen vegetation remain open. Exact shell damage rules, shell continuation through broken cover and splash destruction remain open. |
| Rollover fidelity and feedback | The current model tips onto a side. Full rotational dynamics, tumbles onto the roof, rollover destruction rules and native recovery HUD messages remain open. Ramming damage is outside this work. |
| Environmental hazards | Airborne motion lands on the height map or a supporting surface. Landing and fall damage, water volumes and depth, drowning warnings and timers, and their battle outcomes remain open. |
| Damage and vehicle abilities | Damaged-engine power loss, broken tracks and reserve tracks, crew incapacitation, emergency and repair interactions, wheeled mode switching, active consumables and vehicle-specific boosts require live battle state and further wire evidence. |
| Full modifier fidelity | Situational perks, crew directive effects beyond the implemented equipment directives, progression slot overrides and field modifications, special equipment/vehicle overrides, and exact crew and perk stacking need further work. |
| Native verification | Total speed and traverse curves, track-lock eligibility, slope grip, pushes, tipping and recovery, drop response and loadout modifiers need controlled latest-EU comparisons. |

The movement inputs were checked against the extracted EU bundle reporting
`v.2.4.1.1 #993` and the supplied reference client source. The installed
executable reported SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`.
The launcher registry did not identify the installed executable. A separate
WGUS production lookup on 2026-11-23 reported launcher build `2.4.1.5514`.
The modeled track-lock and downhill responses are simulation choices pending
native comparison.

The bundle-backed Accounts regression resolves an E 50 Training loadout.
It checks the matching-slot Turbocharger's forward and reverse speed changes,
the added power and turret traverse from fuel, and the effects of the seated
driver's full and partial perks. The Battles regressions cover the chassis
grade limit, single-track pivots, downhill overspeed, track-lock eligibility,
and landing against the terrain height. The encrypted E2E trace connects the
mobility profile to vehicle preparation and drive steps to replication.

## Penetration, damage and feedback

| Mechanic | Game mechanic | Server coverage |
| --- | --- | --- |
| Penetration | Plate angle, normalization, caliber, range and the shell's remaining penetration determine the outcome. | **Partial.** The gun ray intersects the equipped chassis, hull, turret and gun collision meshes. The solver resolves AP-family normalization and overmatch, AP-family and HEAT ricochets, screen penetration and HEAT jet loss. AP-family shells retain 75% of remaining penetration at the first ricochet. Eligible shells can continue on the reflected ray for up to ten calibers and can hit the same tank again or another nearby enemy tank. A shell can carry unused penetration past a screen-only contact, and a hull penetration traces internal devices for ten calibers or until the next hull surface. Contacts with allies or wrecks and server-only internal geometry remain open. |
| Shell and armor damage | A penetrating hit applies rolled damage and the plate's damage factor. Some contacts produce an impact with zero vehicle HP loss. | **Partial.** The server rolls penetration and hull damage from the equipped shell definitions and caps HP loss at the target's current health. The client receives the actual part and a short component-local ray through the contact. The normal roll distribution is an approximation. |
| Modern direct-fire HE | Armor penetration deals hull damage. An exterior detonation against main armor causes spall. Screens can stop the shell or reduce penetration before it reaches main armor. [Wargaming's HE rework][eu-he] | **Partial.** Modern shells can penetrate permitted screens and destructible fragments. A penetrated screen subtracts three times its effective thickness. A shell stopped at a screen or passing through a screen with no armor behind it causes zero HP damage. Spall and fragments use the shell damage curves and configured fragment absorption. Exact spall transfer and fragment geometry still need production evidence. Ground impacts can break trees. Ground splash affects vehicle HP only for artillery. [Wargaming HE support][eu-he-support] |
| Module and crew damage, ramming and repair | Module and crew damage have effects independent of vehicle HP. Ramming, fire, detonation and repair have separate conditions. | **Partial.** Model materials carry device hit probabilities. The server tracks hits to modeled devices and crew, disabled tracks and engines, gun disablement, passive module recovery, fire ticks and loaded ammunition-rack explosions. Full crew and module penalties, active repair and first aid, native per-device indicators, damage from collisions and falls, and exact fire tuning remain open. |
| Zero-damage hits and ghost shells | Surface contact can occur without a loss of HP. A visual shot can also miss its intended target. | **Partial.** Armor blocks, screen stops, screen penetrations, ricochets, scene impacts and range expiry have distinct results. The struck player receives a blocked directional indicator for armor blocks, screen stops and ricochets with zero HP loss. Those contacts leave a persistent surface mark when the shell defines one. The server sends health and damage-log updates only for actual HP loss. |
| Crew voice messages | The client chooses announcements based on battle events such as penetration, ricochet and destruction. | **Partial.** The server sends the appropriate shot results and impact effects. Native cue selection and sticker placement still need a controlled client session. |

### Contact and shell outcome

The extractor writes material-tagged triangles and bounds from the packaged
Havok collision models. Accounts joins the triangle material IDs to the
equipped module's armor entries and the common material defaults. Battles
transforms the ray into each part's local coordinates using hull tilt, turret
yaw and gun pitch. The local hit segment is quantized against the component
bounds for the client's `showDamageFromShot` decoder. A fresh extraction is
required to supply these collision meshes. See [resource files](resources.md).

Penetration and damage interpolate between the shell definition's near and far
values, using the default **50 m** and **500 m** anchors in the client common
helpers. The formula extends past the far anchor and clamps at zero. Effective shell reach is the shortest of the nominal range and the zero crossings for penetration, armor damage, device damage, and modern HE impact damage curves. A shell
rolls damage and penetration independently at launch. The armor solver applies
the struck material's hit-angle, normalization, overmatch and ricochet flags.
The angle is measured from the plate normal. A 100 mm plate at 60° presents
200 mm of line-of-sight armor before normalization. AP-family shots get
increased normalization above two times the plate's thickness and can bypass
ricochet above three times the thickness where that material permits it.
HEAT loses penetration across air gaps after the first penetrated layer and
does not ricochet as a jet.

A ricochet ends the incoming tracer and can start a reflected tracer. The
reflected projectile can travel up to ten times its caliber (1 m for a 100 mm
round), bounded by the range left at the bounce. It can penetrate a different
part of the same vehicle, such as the hull after striking the turret, or a
nearby enemy vehicle if its remaining penetration defeats the next plate. The
reflected leg checks terrain and scenery and can register a second ricochet,
which ends the projectile's flight. See the
[archived Wargaming Battle Mechanics guide][wg-archived-battle-mechanics].

For modern direct-fire HE, a successful main-armor penetration applies rolled
hull damage. When the shell reaches main armor and fails the penetration roll,
spall damage currently scales the configured base by the ratio of nominal
penetration to nominal penetration plus plate thickness. This is a gameplay
approximation. The previous area-splash rule is historical for direct-fire HE.
Artillery has a separate ground splash path. It checks terrain and scenery
cover along sample rays toward exposed main armor, then applies radius falloff
and armor absorption. Shells with a stun profile can stun targets within their
own radius. A direct hit can stun without dealing splash damage twice. Training
uses the vehicle's stun property and reduces gun traverse while stunned.
The server approximates blast exposure and stun falloff. It still needs a
native comparison for timers and the full stun effects on vehicle stats. A modern HE shell can
continue after breaking a destructible fragment when its shell definition
permits it. The resolver honors an explicit shell power reduction and currently
has no material-specific default.

The 2026-11-23 production WGUS lookup reported EU SD build `2.4.1.5514`.
The registered EU client matched that build in Game Center metadata and
reported `v.2.4.1.1 #993` in `version.xml`. The collision
data was freshly extracted from that installation with executable SHA-256
`fc80a8fcd804dba1ce54482d87950f4cbe31b3742eefd0c3de03dc28e941af7c`.
The pinned source reference locates the field definitions in
`items/vehicles.py` and the corresponding penetration preview in
`AvatarInputHandler/gun_marker_ctrl.py` and
`gui/impl/lobby/vehicle_hub/sub_presenters/armor/penetration_utils.py`.
`helpers_common.py` and `VehicleEffects.py` describe local hit-segment
encoding and decoding. The preview supplies evidence for the mechanics it
describes, while server-specific damage and RNG behavior need live comparison.
[Wargaming's penetration guide][eu-penetration] · [Wargaming's HE rework][eu-he]

### Projectile tracers

Training uses the published 565 m draw range for known sources. The archived
section 6.2 describes the policy for unspotted enemies. In arcade and sniper
views, it accepts origins within
445 m and 25° of the viewing direction. In strategic view it accepts origins
inside the viewport. These inputs belong to tracer delivery. Training currently
provides known sources, and no spotting or camera detection was added.
The client receives the same launch velocity used by flight simulation and
the shell's effective range. The effect metadata controls how the native client
draws that tracer, including artillery visuals.

### Critical damage and recovery

Vehicle HP, module HP and crew state are independent. The implementation uses packaged client collision materials. It cannot trace production server-only device or crew geometry that is absent from those meshes. One projectile can
affect more than one of them along its path. The archive describes a
module-specific chance to take damage after a hit and a separate crew chance
for direct hits and explosions. Treat the historical probabilities and perk
adjustments as test hypotheses.

The archived hit-to-damage probabilities are 100% for suspension, 45% for
engine, fuel, optics, radio and turret ring, 33% for the gun, and 27% for
the ammunition rack. It lists crew incapacitation at 33% for AP/APCR/HEAT
contact and 10% for an internal blast. The article also describes a
Deadeye adjustment. These figures describe the archive's model.

| Subsystem | State changes described in the article |
| --- | --- |
| Tracks and suspension | A broken track immobilizes a vehicle. Hit location and vehicle-specific mechanisms such as reserve tracks affect disablement and recovery. The article marks its track damage multiplier as unofficial. |
| Engine and fuel | Engine damage can impair mobility and engine destruction can trigger fire. Destruction of the fuel tank starts a fire in the archived model. Fire deals repeated damage to the vehicle and its internals and affects concealment. Firefighting, crew perks and extinguishers affect prevention or recovery. |
| Gun, turret, optics and radio | Damage or destruction can worsen accuracy, restrict elevation or traverse, prevent firing, or reduce view or signal range. |
| Ammunition rack | Damage slows reloading. Destruction can detonate a loaded rack and destroy the vehicle regardless of its remaining HP. The archive gives an exception for an empty rack. |
| Crew | Loss of a crew member impairs the corresponding role. Loss of the full crew can end participation with hull HP remaining. First aid restores eligible incapacitated crew. |
| Passive and active repair | Crew repair can restore a destroyed module to a functional but damaged state. Repair kits can restore modules fully, with scope and cooldown depending on the consumable. Verify module HP thresholds, timer inputs and cooldowns. |
| Ramming and falling | Vehicle collisions can impart motion and deal damage. The archive's ramming model depends on relative speed, vehicle mass, contact armor and modifiers such as Spall Liner and Controlled Impact. Falling and impact damage belong to the physics outcome as well. |

### Hit interpretation and voice cues

A stopped tracer, ricochet, non-penetration or external-module hit can all
produce effects with zero hull damage. An apparent shell passing through a
vehicle can also result from prediction, missing tracer effects, a gap in
collision geometry or a cosmetic part with no collision. Compare server
contact and the effects seen by shooter and target before assigning a cause.

The article's voice inventory maps cues to round start, auto-aim acquisition
and loss, hit outcome, enemy destruction, damage and recovery by module,
crew loss by role, vehicle or crew defeat, and friendly destruction. It
describes cases where several hit outcomes apply but the client plays one
cue. It also lists historical unused cues for reload, spotting, nearby
explosions, allied and enemy deaths, and round results. Map these to semantic
events and verify cue selection in the client.

## Remaining projectile and damage work

- **HE and artillery.** Establish the production spall and fragmentation
  formulas, splash exposure and falloff, and stun stacking and penalties.
- **Modules and crew.** Fill gaps in the internal hit geometry, implement the
  remaining module and crew penalties, and handle full crew incapacitation.
  Calibrate fire damage and passive module repair timing.
- **Active consumables.** Implement repair kits, first-aid kits, and fire
  extinguishers with their cooldowns.
- **Tracers and flight.** Connect the hidden-source tracer policy to future
  spotting and camera inputs. Spotting belongs to a separate workstream.
  Check for collisions against vehicles that move during shell flight.
- **Native client validation.** Verify stun timers, module and crew indicators,
  ammo-rack effects, and artillery tracers in a live client session.

## Evidence for unresolved rules

- Use the [current resource bundle](resources.md) to identify vehicle, shell
  and arena inputs. Resource values establish what the client ships. Runtime
  modifier order, server timers and server decision rules need separate evidence.
- Use [battle replays](battle-replays.md) for observed sequences. Use a
  controlled bidirectional capture to verify client commands and delivery,
  with both the initiating and observing clients recorded for per-viewer rules.
- Record mode, client identity, input setup and observed outcome. Keep the
  archive's exact formulas, timing tables, random distributions and reward
  coefficients historical until evidence supports their use on current EU.

[eu-communication]: https://worldoftanks.eu/uk/news/general-news/1-10-battle-communication/
[eu-matchmaker]: https://worldoftanks.eu/en/news/general-news/update-2-0-matchmaker/
[eu-history]: https://worldoftanks.eu/en/content/docs/release_notes/
[eu-crew]: https://worldoftanks.eu/en/news/general-news/crew-perks-expansion-2026/
[eu-spotting]: https://wargaming.net/support/en/products/wot/article/10222/
[eu-cadence]: https://worldoftanks.eu/en/news/general-news/version-916-spotting-improvement/
[eu-penetration]: https://worldoftanks.eu/en/content/guide/newcomers-guide/how_to_survive/
[eu-he-support]: https://wargaming.net/support/en/products/wot/article/15053/
[eu-he]: https://worldoftanks.eu/en/news/general-news/1-13-HE-shells/
[eu-movement]: https://worldoftanks.eu/en/news/general-news/ver914-new-physics-final/
[eu-recovery]: https://worldoftanks.eu/en/content/docs/release_notes/release-notes-1-22/

[wg-archived-battle-mechanics]: https://web.archive.org/web/20250303052442/https://wiki.wargaming.net/en/Battle_Mechanics
