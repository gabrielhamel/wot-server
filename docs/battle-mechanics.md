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

| Mechanic | Game mechanic | Server coverage |
| --- | --- | --- |
| Base capture | Occupying an objective can accrue capture progress and decide a round. The objective and victory conditions depend on the mode. | **Partial.** Training supports team elimination, departure-based outcomes and a time-limit draw. Capture zones, progress, resets and capture wins are open. See [round state](../src/battles/src/domain/round.rs) and [outcome delivery](../src/battles/src/infrastructure/mod.rs). |
| Battle communication | Players can send contextual requests and ping vehicles or map locations. See [Wargaming's communication guide][eu-communication]. | **Open.** The Training channel handles room text chat in the garage. In-arena text, quick commands and contextual pings are separate workflows. See [Player Sessions](../src/player-sessions/README.md). |
| Matchmaking | Random Battles assemble teams using tier, vehicle role, class limits, and queue and map policies. Wargaming has revised those policies since the snapshot. See the [2.0 matchmaker notes][eu-matchmaker] and [update history][eu-history]. | **Open.** Player Sessions hands an assigned Training roster to Battles. See the [launch contract](../src/battles/src/contract.rs). |
| Tank stats | Equipped modules define base characteristics. Crew, equipment, consumables and directives can modify applicable stats. The archive's crew formulas and examples need separate verification against the current crew system. See the [EU crew rework][eu-crew]. | **Partial.** The launch profile includes base mobility, gun and ammunition data derived from the equipped modules. Full battle modifier calculation and application remain open. See [profile extraction](../src/accounts/src/infrastructure/client/battle_performance.rs), [crew](crew.md), [equipment](equipment.md), [consumables](consumables.md) and [directives](directives.md). |
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
| Draw distance and tracers | Local rendering, the server's vehicle draw radius, minimap knowledge and tracer visibility are separate. [Wargaming Player Support][eu-spotting] gives a 565 m vehicle draw circle. The archive lists 564 m and describes historical camera-dependent rules for tracers from hidden vehicles. | **Open.** The arena sends vehicle state and shot effects to entered peers without a per-viewer spotting or draw gate. See [vehicle replication](../src/battles/src/infrastructure/controls.rs) and [shot delivery](../src/battles/src/infrastructure/projectiles.rs). |
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
| Ballistic trajectory | Flight time and projectile drop affect target lead and the ability to hit over terrain. Shell types and gun definitions can differ in range and velocity. | **Partial.** The shot follows a straight ray at a shared 450 m/s. The server chooses contact at launch, delays the impact by flight time and uses the configured shell range. Gravity, per-shell velocity and collision checks against moving targets during flight are open. See [projectile resolution](../src/battles/src/infrastructure/projectiles.rs) and [range expiry](battle-replays.md#range-expiry). |
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
approach speed. The server deducts 25% of the share of impact energy required
for the break, after accounting for engine pressure. This caps the speed loss
at about 13.4% per fragment. The active EU client's
`scripts.pkg/scripts/client/Vehicle.pyc` confirms the impact damage
coefficient in `_isDestructibleMayBeBroken`. The method returns a fracture
prediction. Toolkit's `base/physics/rubble.rs` uses a separate speed-loss
fit from a Type 64 capture, with a coefficient applied to fragment damage and
normalized vehicle mass.

The server's previous 35% energy deduction was introduced in commit
`5c09940`. A synthetic test using the extracted stock TKS 20 mass of 2,800 kg
and the 10 health of Widepark panel 45 in chunk `0x817e` reproduced a stall
after two breaks at an entry speed of 8.5 m/s. With the 25% deduction the TKS
breaks three at that speed. At 9.8 m/s it breaks all five test panels while
losing speed. Sustained pressure and slowdown through an actual row of panels
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
| Penetration | The game compares shell penetration with the armor encountered along its path. Plate angle, normalization, ricochet and caliber overmatch affect the result. The archive also describes penetration rolls, distance loss and subsequent impacts through layered armor or after a ricochet. | **Open.** Vehicle contact uses a bounding sphere for each eligible enemy. A damaging contact is reported as a penetration. There is no armor mesh or penetration solver. See the [vehicle intersection](../src/battles/src/domain/vehicle.rs) and [shot results](../src/battles/src/infrastructure/projectiles.rs). |
| Shell and armor damage | Penetrating direct-fire hits remove vehicle HP. Nominal damage is the basis for a damage roll, and modules and crew follow separate damage rules. Certain contacts can produce impact feedback without HP loss. | **Partial.** Every eligible vehicle contact applies the configured nominal short-range HP damage, bounded by remaining health. Health loss and destruction are replicated. Armor-dependent outcomes, damage rolls and friendly vehicle hits are open. See [direct-hit evidence](battle-replays.md#direct-hit-in-replay-1344). |
| HE after the rework | Direct-fire HE uses a point-of-impact model with different results for penetrations and non-penetrations. Screens, tracks and some destructible objects can affect the remaining penetration. The wiki's earlier HE model is historical. Validate SPG splash and stun separately. See [Wargaming's HE rework][eu-he]. | **Open.** Shell kind affects feedback, while HE uses the same vehicle HP path as other shells. Scene hits stop the projectile without blast damage or penetration through objects. |
| Module and crew damage, ramming and repair | Separate damage to modules or crew can impair the tank. Tracks, engine fires, ammunition rack explosions, ramming, recovery and repair have their own conditions and effects. The archive's probabilities and repair values are research leads. | **Open.** Training tracks vehicle HP and alive state. Module and crew injury, track disablement, fire, ammunition rack explosions, battle repairs and active repair consumables are open. Garage consumable mounting is covered in [Garage consumables](consumables.md). |
| Zero-damage hits and ghost shells | A shell can make contact without damaging the hull. An apparent hit with no server contact can look similar to the player and requires separate evidence. | **Partial.** The server distinguishes scene impacts, range expiry and vehicle contacts. It has no track, spaced-armor or ricochet outcomes. Compare the tracer, collision and hit result on both clients when investigating an apparent hit. See [range expiry](battle-replays.md#range-expiry). |
| Crew voice messages | Client announcements convey battle events such as auto-aim, hits, kills, module damage, repairs and crew losses. The archive lists historical voice cues. | **Partial.** The server publishes round state, hit results, damage events, hit direction and health changes. Native cue selection depends on the client and the supported event. See [hit direction](battle-replays.md#direction-indicator-in-the-2026-11-22-training-room-capture) and [impact marks](battle-replays.md#persistent-vehicle-impact-marks). |

### Contact and shell outcome

1. **Find the surface the shell strikes.** Use the projectile's direction
   at impact and the collision surface normal. Hull armor, spaced screens
   and external modules have distinct consequences. A contact with a
   screen or track may end before the shell reaches armor that controls
   vehicle HP.
2. **Resolve angle and remaining penetration.** The archive gives ricochet
   thresholds of 70° for AP/APCR and 85° for HEAT, measured from the normal.
   AP/APCR normalization turns the effective angle toward the normal.
   The archived two-caliber rule increases normalization; the three-caliber
   rule bypasses AP/APCR ricochet. A shell that bypasses ricochet can still
   lack the penetration needed to cross the plate. The line-of-sight
   thickness of a flat plate is its nominal thickness divided by the cosine
   of the applicable angle. For example, a 100 mm plate at 60° presents
   200 mm before normalization.
3. **Account for shell properties and subsequent surfaces.** The wiki
   reports a penetration roll of up to ±25% and distance-dependent loss
   for AP/APCR, with no loss for HE, HESH and HEAT. A shell that passes a
   screen carries only its remaining penetration to the next surface.
   The archive describes linear loss beginning after 100 m, continued
   travel up to ten shell calibers after penetration, and a redirected
   path after ricochet. Validate range curves, threshold
   equality, per-shell exceptions and continuation against current EU.
4. **Apply the appropriate damage rule.** A penetrating AP, APCR or HEAT
   hit uses the hull damage roll. The archive reports up to ±25% from
   nominal hull damage. For direct-fire HE, Wargaming documents the
   post-1.13 point-of-impact rule. Hull penetrations and non-penetrations
   yield different damage, and a screen, wheel or track subtracts more
   penetration than an equally thick destructible obstacle. The release
   describes a multiplier of three for screens and external modules and
   one for destructible obstacles. [Wargaming's HE rework][eu-he]

The pre-1.13 direct-fire HE model could deal damage on a near miss. Its
historical equation took half the nominal shell damage, scaled it linearly
by remaining splash radius, and subtracted 1.1 times plate thickness times
the spall coefficient. It selected an unobstructed path to armor based on
the resulting damage. The historical section also describes a cone for
internal criticals after a direct exterior explosion. SPG splash, stun and
shell variants need their own rules and evidence.

### Critical damage and recovery

Vehicle HP, module HP and crew state are independent. One projectile can
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
[eu-he]: https://worldoftanks.eu/en/news/general-news/1-13-HE-shells/
[eu-movement]: https://worldoftanks.eu/en/news/general-news/ver914-new-physics-final/
[eu-recovery]: https://worldoftanks.eu/en/content/docs/release_notes/release-notes-1-22/
