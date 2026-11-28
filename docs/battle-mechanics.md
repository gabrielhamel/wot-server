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
| Draw distance and tracers | Local rendering, the server's vehicle draw radius, minimap knowledge and tracer visibility are separate. [Wargaming Player Support][eu-spotting] gives a 565 m vehicle draw circle. The archive lists 564 m and describes historical camera-dependent rules for tracers from hidden vehicles. | **Implemented in Training.** Vehicle entities and live poses follow per-player visibility and the draw radius. Tracer delivery uses that visibility and remembers recipients for tracer stops. Strategic camera containment remains open. See [vehicle replication](../src/battles/src/infrastructure/controls.rs) and [shot delivery](../src/battles/src/infrastructure/projectiles.rs). |
| View and spotting range | View range is an input to detection. A vehicle can spot an enemy itself up to 445 m, with proximity detection within 50 m even through cover. Extra view range helps detect concealed targets within the cap. See [Wargaming Player Support][eu-spotting]. | **Implemented in Training.** Pairwise detection uses fitted base view range, three-dimensional distance and scheduled checks. |
| Visibility checks and camouflage | Beyond proximity range, detection depends on line of sight and concealment. The archive describes view ports and target checkpoints, changes to concealment from movement, firing and vegetation, and how long a vehicle stays spotted. Its check intervals are explicitly flagged as outdated after 9.16. See [Wargaming's visibility system changes][eu-cadence]. | **Implemented in Training.** Each ray tests terrain, solid scenery and estimated foliage volumes. Fitted camouflage coefficients and hull movement determine the vehicle contribution. See [garage camouflage](camouflage.md) and [scene impacts](battle-replays.md#scene-impacts-and-gun-audio). |
| Radio range | Allies within signal range share spotted positions, which can appear on the minimap beyond the recipient's vehicle draw range. See [Wargaming Player Support][eu-spotting]. The combined-range formula and relay behavior in the archive need current-client verification. | **Implemented in Training.** Living allies share contacts throughout each connected radio group. Losing a link freezes the last reported allied positions and removes enemy contacts supplied only across the split. |

### Detection, concealment and delivery

Battles derives the static and turret-relative observer ports and six fixed
target checkpoints from collision bounds and mount positions. The side
checkpoints use hull width at gun-mount height. The observer alternates ports
on the arena clock. Proximity detection uses three-dimensional distance and
bypasses ray tests. Outside proximity range a single unblocked ray within
`view - (view - proximity) × concealment` qualifies for detection. View is
capped at the configured maximum. The loadout uses base ranges and base
stationary or moving concealment, multiplied by 0.5 for zero crew skill. Hull
translation or turning selects moving concealment.

A successful shot applies the fitted gun's `invisibilityFactorAtShot` for five
seconds from the latest shot. In the inspected EU build `2.4.2.0 #1007`, the
three SU-152 loadouts use multipliers of 0.143, 0.19 and 0.20. Wargaming's
[PC concealment video][eu-concealment-video] compares three SU-152 guns,
including the two 122 mm configurations with different muzzle brakes. The
PC firing duration is a configurable default. The [Blitz spotting
guide][blitz-spotting] describes five seconds for Blitz.
Along each sight ray, foliage near the observer
is transparent. Foliage near a firing target contributes the strongest nearby
bonus at 30%, and farther foliage keeps its full contribution. The vegetation
total is capped at 80% and total concealment at 100%. Foliage weights and
crown geometry currently use approximations described in the resource guide.

A living target starts one ten-second retention timer when its last direct
detection is lost. Live spotters can share a retained contact. Splitting or
rejoining a radio group affects access immediately and leaves its retention
clock running. Direct detection works with a group of one. In-range
contacts get a client Vehicle entity, and contacts outside the draw radius
can get minimap positions and health. Losing an allied link preserves its
last reported marker. Sixth Sense schedules one owner alert three seconds
into a continuous spotted period. The alert rearms after the vehicle goes
fully dark. Commander incapacitation is a later mechanic.

Under **Admin → Battles → Battle parameters → Spotting**, the default
distance bands end at 50, 150, 270 and 445 m. The supported ranges are
0–100, 1–300, 1–444 and 1–445 m, respectively, and the bands must increase.
The default check intervals are 1, 5, 10 and 20 simulation ticks (100 ms
each). The first three support 1–20 ticks and the far interval 1–50 ticks.
Port alternation defaults to 20 ticks and accepts 1–100. Retention defaults
to 100 ticks and accepts 0–600. Sixth Sense and the firing penalty default
to 30 and 50 ticks respectively and both accept 0–300. The draw radius
defaults to 565 m and accepts 0–1500. Nearby foliage distance defaults to
15 m and accepts 0–100. Foliage coefficients and the crown base fraction
accept values from 0 to 1. Their defaults are 0.5 leafy, 0.25 leafless,
0.8 cap, 0.3 retained after firing and 0.4 crown base. Check intervals and
shot duration are agreed implementation defaults. Measurements for the
current official PC battle server are open.

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

After a successful admin launch, the browser replaces its last battle setup
in local storage with the map, duration, simulation overrides and stationary
dummies (including teams and loadouts). Players and their tank configurations
are selected afresh for each launch. Reopening the panel restores the setup
and expands **Battle parameters**. The setup status and **Reset battle setup** button
appear below the map and duration, even when **Battle parameters** is collapsed.
Reset clears the entire current form. Reloading still restores the last successful launch.

Every adjustable mechanic and hardcoded gameplay tuning value owned by
Battles belongs in the admin Battle parameters panel. Define validated
defaults and ranges with the mechanic and pass the chosen values at launch.
Protocol identifiers, mathematical constants, resource definitions and hard
safety ceilings remain implementation details.

Battles owns the defaults and ranges in
[parameters.rs](../src/battles/src/domain/parameters.rs). The admin dashboard
can override round timing, parked position refresh, gravity and drive
response, hull contacts and rollover, ballistic integration, armor and device
damage, track repair, fire and tracer visibility. The validated values travel with the battle launch and are shared
by simulation and replication for that arena. Native Training Rooms use the same defaults.

The dashboard takes the field limits from the server. Use only technical
constraints for per-battle minimum and maximum values. These include numeric
representation, protocol fields, required invariants, and bounded server
work. Settings may produce unrealistic gameplay or a broken simulation as
long as the server handles them safely and continues serving other battles.

Integer intervals use their Rust storage limits. Spotting and parked pose intervals accept up to
4,294,967,295 ticks and fire accepts up to 255 ticks. Loading, countdown,
active play and results share the arena's 429,496,729-second clock budget.
Spotting distances must remain strictly ordered from proximity through
maximum range. Distance checks use the largest `f32` value that can be
squared without overflow.

Decimal settings accept fractional input and round to `f32` precision.
Fractions and cosines use their mathematical ranges. The track repair
multiplier uses the 120-second maximum base repair time when calculating
its upper limit. Projectile processing caps each flight at 640 segments
and each damage roll at 12 samples to bound synchronous work. Device hits
use the capacity of the strike buffer. The ballistic step accepts up to the
arena clock budget divided by the maximum segment count.

The 60 Hz physics step and single-precision collision geometry determine the
physics limits. Geometry uses a minimum fallback collider half extent of
1 mm and a scale of approximately 8.39 km as its upper bound. The server
derives its linear velocity limit from that distance per step and its angular
velocity limit from half a turn per step. Signed gravity is limited to the acceleration that
can change velocity by the linear speed limit in one step, approximately
30.2 million m/s² in either direction. Positive values pull down and
negative values pull up. Fallback mass bounds keep the mass-to-volume ratio
and squared momentum representable.

Suspension frequency, damping, and track response accept values beyond the
stable range of the integrator. Track forces use wider intermediate
arithmetic and cap the result using the per-step velocity limit.

The native movement bitmask identifies commands. Shell ballistics and equipped
vehicle performance come from the resource bundle. The tracer policy uses each recipient's known Vehicle entities.

### Parked vehicle positions

Training sends `TickSync`, the hull-relative Avatar pose and pending Mercury
ACKs every 100 ms. Each driver also receives their own hull pose every tick.
For other vehicles, the server compares position, yaw, pitch and roll with
the previous tick. A changed pose is sent immediately and remains
at the full tick rate for up to five ticks starting with that change. A client
entering the arena also triggers a full snapshot. Unchanged poses then use
refresh slots staggered by entity ID. Turret angle properties have their own
delivery path.

Under **Admin → Battles → Battle parameters → Network replication**,
**Other parked vehicle pose interval (100 ms ticks)** accepts 1–4,294,967,295 and defaults to 5.
It applies to other vehicles. Set it to 1 to include every pose on every tick. Native Training Rooms use the
default. See the [native replay evidence](battle-replays.md#widepark-parked-vehicle-cadence-from-2026-11-26).

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
tracked classes. Rapier resolves airborne motion and landings against terrain
and scenery. The track forces approximate traction, braking and track lock. The client speedometer admits up to 1.5 times the listed
speed in the supplied client reference. Training uses that ratio as a downhill
simulation bound. It still needs a measured EU downhill curve. See
[movement simulation](../src/battles/src/domain/movement.rs),
[hull physics](../src/battles/src/infrastructure/physics.rs),
[replay evidence](battle-replays.md#vehicle-movement-and-chassis-animation)
and [Wargaming's movement physics notes][eu-movement].

#### Rapier hull physics

Every arena creates a Rapier rigid-body world on the preparation worker.
Each 100 ms arena tick runs six 1/60 s physics steps. The server sends
the solved pose and chassis animation through the existing snapshot and
property updates.

The movement rules request longitudinal acceleration and yaw acceleration from
the equipped power-to-weight ratio, material resistance, speed limits, steering
and brake inputs. Three suspension probes on each track apply impulses where
they find support. A hull crossing a crest can lose contact one end at a time.
Gravity, external pushes, attitude and landing velocity come from the
[rigid-body world](../src/battles/src/infrastructure/physics.rs). The simplified
hull collider uses the loaded chassis and hull bounds and preserves the native
vehicle origin. Fixed heightfields reproduce the terrain tile triangles.
Scenery collision shapes are shared across arenas. The preparation worker
loads the spawn neighborhoods. Each physics step keeps the map placements
around the swept hulls in 32 m pages, across all elevations.
Fixed scenery meshes preserve bridges, underpasses, arena visibility and
vehicle-pass flags. Destroyed fragments affect subsequent physical contacts
in the same arena. Before each step the server sweeps the hull and tracks
along their approach to nearby obstacles. When a fragment gives way under
the impact, the tank pays its horizontal speed cost and the suspension can
reach the ground beneath it. Solid surfaces and surviving fragments still
support or deflect the hull. Armor intersections and HP decisions use the Battles rules.
Hulls collide with other vehicles and wrecks. A tank perched entirely on
another hull needs to regain contact with map geometry before its tracks
can propel it. The suspension treats support entirely on scenery as hard
ground.

During a turn, the tracks supply the sideways force needed to follow the
hull's heading. On a bank they also oppose gravity along the surface.
Neutral and braked tracks can hold a parked hull across, along or diagonally
to the slope. The available grip depends on surface material and the load on
each track contact. Once gravity exceeds that grip the hull slides downhill.
Track-lock steering keeps its own skid response.

| Rapier setting | Default | Range |
| --- | --- | --- |
| Suspension travel | 0.30 m | 0.05–0.8 m |
| Suspension frequency | 3 Hz | 1–5 Hz |
| Suspension damping ratio | 0.8 | 0.1–1.5 |
| Lateral grip response | 0.08 s | 0.05–2 s |

These are server tuning values. The [El Halluf playtest](battle-replays.md#el-halluf-training-capture-from-2026-11-27)
informed the handling changes. Retail grip limits and suspension response
still need calibration.

##### Validation on 2026-11-27

The extracted EU client resources reported `v.2.4.2.0 #1007`. The installed
`WorldOfTanks.exe` SHA-256 was
`0d7d61866dfe8f884a76f31a6c91308662b27e0ff09f14f08647234048c939f6`.
For the same test loadout, two seconds of forward input followed by a full
brake and a one-second stationary turn produced the following results.

| Ground | Acceleration distance | Braking distance | Turn |
| --- | ---: | ---: | ---: |
| Hard | 8.54 m | 0.95 m | 0.908 rad |
| Soft | 3.95 m | 0.29 m | 0.451 rad |

The Serene Coast regression drives one hull across the bridge and another
underneath. It checks the replicated positions on both levels and traces
each drive command to its position update. A separate fixture drives off a
finite deck, compares two sets of controls in free fall, and lands on a bank.
Another powers a parked tank through a destructible wall and checks the
native fracture delivery, the adjacent fragment and both tanks' HP.

In a 5 m/s powered turn on flat hard ground, sideways speed after one
second fell from 0.93 m/s to less than 0.01 m/s. The heading change was
0.78 rad in both runs. A separate fixture starts with a 2 m/s sideways
push. The tracks brought that below 0.1 m/s in 0.40 s, down from 0.65 s.
Across an 11.5° hard bank, a parked hull moved 0.009 m in four seconds,
compared with 1.41 m before the change. On a 37.2° hard bank, all three
tested headings slid downhill with the brake applied as well as in neutral.
At 26° the same hull held on hard ground and slid 1.07 m on soft ground
in two seconds. Driving across the 11.5° bank for 1.67 s produced
6.24 m of forward travel and 0.02 m of sideways drift. On the 37.2°
bank the hull still moved 5.40 m forward while sliding 1.56 m downhill
in the map plane and losing 1.18 m in height.

The El Halluf regression drives through the bush from the playtest without
publishing its fall. A separate downhill approach drives past tree 20 in
chunk `0x8081` twice, first with the trunk cleared, then with it standing.
Over the 0.30 s after the tree broke, the second run added less than 0.01°
of pitch or yaw and less than 2 mm of hull rise. On a separate flat course, an off-centre
4 m tree proxy previously added 13.86° of pitch. The same impact now adds
less than 0.02°. A 24 cm breakable prop also fractures as the track reaches it.

On an Apple M1 Max, a release build with 30 dispersed drivers on Serene
Coast measured 14.0 ms at p95 and 26.1 ms maximum over 300 timed arena
ticks. The test process peaked at 115.6 MiB RSS.
It updates the arena and queues snapshots for all 30 drivers. Gateway UDP
encoding, network transport and competition between active arenas need
separate measurements. To reproduce, set `WOT_RESOURCE_DIR` and run
`cargo test --release -p wot-battles measure_thirty -- --ignored --nocapture --test-threads=1`.
The [scenario fixtures](../src/battles/src/infrastructure/rapier_scenarios.rs)
use 20 warm-up ticks and check the queued snapshots for every recipient.

#### Scenery contact, pushing and recovery

The hull collider uses three-dimensional chassis and mounted hull bounds,
including rear overhang and offsets from the vehicle origin. Track dimensions
provide fallback extents when collision bounds are unavailable. Arena variants
use their collision visibility mask. Tracks can find support on a raised deck
while a separate vehicle travels on the terrain underneath.

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
from mass and contact speed, including the motion at the point where a turning
hull reaches an obstacle. Powered input adds sustained pressure at contact even while the hull is
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
in the vegetation catalog remain pass-through vegetation and provide
concealment. The supplied client's `AreaDestructibles.py` deactivates a bush
when its native ID arrives in a `fallenTrees` update. The server skips older
tree catalogs that could contain those IDs. Fallen trees no
longer block hulls in the server. The native `fallenTrees` record carries the
object index, the maximum pitch, the fall direction and the initial speed
step. The server estimates the pitch limit against terrain and standing
scenery. The client animates the fall and applies the settled state on entry.

The extractor reads Havok collision data for placed models through its
`tagfile`, `collision` and `mesh` modules. Tree placements come from SpeedTree
data. Server contact tests run in Rust. The native client runs its own
destruction effects.

Rapier resolves contact between live tanks, immovable wrecks and arena
boundaries. Installed mass determines the response to an impact. Forces from
a moving tank can reach a breakable through an unattended tank it pushes.

On steep surfaces, gravity can overcome material-dependent grip. The vehicle
then slides downhill even without a drive command. Side slopes, hard turns
and lateral contacts can tip a hull onto its side. A tipped vehicle cannot
drive or fire. A push from the appropriate side can right it on stable
ground. Training also uses a three-second settling period and a 15-second
automatic recovery countdown on stable ground. Movement restarts the wait.
This is Training server policy modeled on Wargaming's published recovery
rules, which introduced auto-flipping in Random Battles. [EU recovery notes][eu-recovery]

The resource-backed driving scenarios verify that a pushed tank breaks
one numbered wall fragment while an adjacent fragment remains intact.
Both tanks retain their health. The fracture and replicated hull pose retain
the initiating drive trace. The El Halluf scenario verifies a numbered tree
falls on contact. Serene Coast verifies hull support across a bridge and
passage on the terrain underneath. The Ensk regression checks cascaded
destruction and per-arena fracture state.

The protocol evidence comes
from the extracted EU entity definitions, the supplied
`AreaDestructibles.py`, `DestructiblesCache.py` and `Avatar.py` references, and
the Toolkit's matching captured battle in its `WGDE` decoder and
`wg-toolkit-wot/src/base/battle/areas.rs`.

| Remaining gap | What is missing |
| --- | --- |
| Scenery and ground contact fidelity | The convex hull and six suspension probes approximate the full track footprint and chassis shape. Native suspension tuning and grip at mixed-material contacts still need calibration. |
| Destruction coverage and fidelity | Unnumbered scenery lacks native synchronization. Trunk shapes, contact loads, pitch limits and foliage volumes are estimates. Fall collisions with tanks and other trees, authoritative debris motion, exact shell damage, shell continuation through broken cover and splash destruction remain open. |
| Rollover fidelity and feedback | Rigid bodies can tip and roll onto the roof. Rollover destruction rules and native recovery HUD messages remain open. Ramming damage is outside this work. |
| Environmental hazards | Airborne motion lands on the height map or a supporting surface. Landing and fall damage, water volumes and depth, drowning warnings and timers, and their battle outcomes remain open. |
| Damage and vehicle abilities | Damaged-engine power loss, reserve tracks, crew incapacitation, emergency and active repair interactions, wheeled mode switching, and vehicle-specific boosts still need implementation or further wire evidence. |
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
| Module and crew damage, ramming and repair | Module and crew damage have effects independent of vehicle HP. Ramming, fire, detonation and repair have separate conditions. | **Partial.** Model materials carry device hit probabilities. The server tracks hits to modeled devices and crew, disabled tracks and engines, gun disablement, passive module recovery, replicated engine fires and loaded ammunition-rack explosions. Track HUD and sound updates use native vehicle state and shot feedback. Engine and fuel tank state and automatic repair also update the driver’s module HUD. Full crew and other module penalties and HUD feedback, active repair and first aid, damage from collisions and falls, and exact fire tuning remain open. |
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
inside the viewport. These inputs belong to tracer delivery. Training uses per-viewer spotting
and the gun direction for its direct-view fallback. The strategic camera
viewport remains open.
The client receives the same launch velocity used by flight simulation and
the shell's effective range. The effect metadata controls how the native client
draws that tracer, including artillery visuals.

### Critical damage and recovery

Vehicle HP, module HP and crew state are independent. The implementation uses packaged client collision materials and an estimated engine compartment where exhaust hardpoints are available. One projectile can
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
| Engine and fuel | Engine damage can impair mobility. The archive describes an ignition roll on engine destruction and automatic ignition at zero fuel tank HP. Fire deals repeated damage to the vehicle and its internals and affects concealment. Firefighting, crew perks and extinguishers affect prevention or recovery. |
| Gun, turret, optics and radio | Damage or destruction can worsen accuracy, restrict elevation or traverse, prevent firing, or reduce view or signal range. |
| Ammunition rack | Damage slows reloading. Destruction can detonate a loaded rack and destroy the vehicle regardless of its remaining HP. The archive gives an exception for an empty rack. |
| Crew | Loss of a crew member impairs the corresponding role. Loss of the full crew can end participation with hull HP remaining. First aid restores eligible incapacitated crew. |
| Passive and active repair | Crew repair can restore a destroyed module to a functional but damaged state. Repair kits can restore modules fully, with scope and cooldown depending on the consumable. Verify module HP thresholds, timer inputs and cooldowns. |
| Ramming and falling | Vehicle collisions can impart motion and deal damage. The archive's ramming model depends on relative speed, vehicle mass, contact armor and modifiers such as Spall Liner and Controlled Impact. Falling and impact damage belong to the physics outcome as well. |

#### Engine fire

The archived wiki describes ignition on engine destruction. Wargaming's
support guide describes ignition regardless of remaining engine HP. Training
follows the support guide for engine hits and the archive for transmission hits.
A hit on authored transmission geometry damages the shared module without
rolling for ignition. The installed engine supplies its ignition probability. The
server applies the threshold to the rolled module damage. The client bundle
names the threshold `minFireStartingDamage` and loads it on the engine
descriptor. The exact production damage operand is unverified. Fuel tank
ignition uses the transition to zero fuel tank HP on authored geometry.
The original attacker keeps credit throughout the fire, including if they
leave the arena. Repeat hits during a fire leave its remaining duration and
attacker intact. [Archived fire mechanics](https://web.archive.org/web/20250303052442/https://wiki.wargaming.net/en/Battle_Mechanics#Fire_Damage) · [Wargaming module damage guide](https://wargaming.net/support/en/products/wot/article/15762/?redirect_lang=en)

When engine or fuel tank HP crosses the module's critical threshold or reaches
zero, the driver receives the corresponding module state in
`OwnVehicle.vehicleDamageInfoList`. Automatic recovery sends the repaired
state and refreshes `OwnVehicle.destroyedDevicesIsRepairing`. The timer list
also includes tracks with repairs in progress. A hit above the critical
threshold can ignite the engine without changing its module icon.

In the sampled WZ-113G FT bundle, the hull has exhaust hardpoints for an
estimated engine compartment. The vehicle collision meshes contain no fuel
tank triangles, so that vehicle can have an engine fire in Training and has
no modeled fuel tank strike.

The extracted client hull meshes rarely contain engine triangles. Training
prefers authored engine geometry where present. Otherwise it estimates a
volume inside the mounted hull's collision bounds, centered laterally and
vertically. The average longitudinal position of the exhaust hardpoints
anchors the volume, which is moved inward as needed to fit the bounds. A shot
must reach that volume through actual armor and pass the engine's module
damage roll. Re-extract the client bundle to get exhaust hardpoints. The
estimate may differ from the production hitbox, especially on vehicles
with exhaust routed away from the engine.

**Admin → Battles → Battle parameters → Fire** exposes the settings below.
Native Training Rooms use the same defaults.

| Setting | Default | Accepted range |
| --- | ---: | ---: |
| Estimated engine compartment width | 0.70 of hull width | 0–1 |
| Estimated engine compartment height | 0.75 of hull height | 0–1 |
| Estimated engine compartment length | 0.35 of hull length | 0–1 |
| Minimum module damage for engine ignition | 21 HP | 0–65,535 HP |
| Fire tick interval | 0.5 s | 0.1–10 s |
| Fire damage ticks | 12 | 0–30 |
| Maximum HP lost on the first tick | 4% of maximum vehicle HP | 0.01–25% |

A zero compartment dimension disables the estimate. Zero fire ticks disables
ignition. Damage declines linearly with the remaining tick count and rounds
to at least 1 HP while the target is alive. The cadence comes from production
replays. Damage, decay and total duration are server estimates.

The burning vehicle receives the `Fire` component on every entered client
and in the arena snapshot for late arrivals. The driver also receives
`Fire.fireInfo` with the ignited module, attacker and server time, which
drives the fire HUD and warnings. The attacker receives the ignition flag
and damage log entries. Damage ticks replicate health to all witnesses,
update `OwnVehicle.vehicleHealthInfo` for the driver and use fire attack
reason in the private attacker and victim logs. Component removal stops the
effect after burnout, destruction or round end. See the
[replay evidence](battle-replays.md#vehicle-fire).

Secondary module and crew damage from burning, concealment penalties,
firefighting perks, extinguishers, and the production treatment of some
front-mounted transmissions remain open.

#### Tracks and automatic repair

Each side has its own HP from the equipped chassis. A shell that penetrates
a modeled track plate applies the shell's rolled vehicle damage to the track's
own HP. A hit at the drive sprocket or idler uses that value in full. The
center divides it by the chassis's `bulkHealthFactor`. The server projects
the end zones inward from the track collision mesh using the named front and
rear driving wheel radii and the wheel-size coefficient in the client reader.
This projection approximates the production hitzones. When the bundle lacks
usable zone data, a penetrated track takes full damage. Repeated contacts on
one track along the resolved path count once. A shot that
crosses both track plates below the hull can damage both with zero hull damage. When an AP or APCR shot crosses the hull, the server
requires enough penetration for the entry and exit faces and the far track.
The server applies hull damage once. This is a simulation rule for the
client-supplied meshes, pending comparison with a controlled live shot.

A track that reaches the chassis's regeneration threshold is marked damaged.
At zero HP it breaks and disables powered movement and hull steering until
both tracks are functional. The turret and gun can still operate. Each side
repairs on its own timer and returns at its chassis regeneration HP. A damaging
hit on a broken track restarts that side's full repair time. The countdown
updates even when the shot deals zero hull HP damage. The
loadout projects the base chassis repair time, the crew's average trained
**Repairs** perk (scaled by perk efficiency), and passive general and
suspension repair-speed modifiers from equipment and consumables. The
**Automatic track repair time multiplier** under **Admin → Battles → Battle
parameters → Tracks** defaults to 1.0 and accepts 0.1–10.0. Training Rooms use
the default.

The driver's `OwnVehicle.vehicleDamageInfoList` carries the side and damage
or repair code before `OwnVehicle.destroyedDevicesIsRepairing` carries the
countdown. The client's `ModulesCtrl` registers the destroyed chassis side
before applying its timer, and the server initializes both properties on
vehicle entry. `Vehicle.publicStateModifiers` carries broken-track extras to both
clients for the damaged model. The shooter receives
`CHASSIS_DAMAGED_BY_PROJECTILE` in `Avatar.showShotResults` for direct
track strikes. The extra IDs come from the bundle's
`scripts/item_defs/vehicles/common/vehicle.xml` in its declared order. The
client reference routes these fields through `OwnVehicleBase`,
`Avatar.showVehicleDamageInfo`, and `TrackHealth`. The schema and serialized
updates are covered by the battle tests. Native audio and rendering still need
a two-client check.

Local validation used EU production build `2.4.2.5524`, labeled
`v.2.4.2.0 #1000` by the client, with executable SHA-256 `0d7d61866dfe8f884a76f31a6c91308662b27e0ff09f14f08647234048c939f6`.
The extracted T1 Cunningham chassis mesh has separate left- and right-track
materials with a projectile damage chance of 1.0. A two-participant battle
test with a controlled collision fixture decodes the shooter and driver
feedback and checks that the repairs trace back to the shot. In the same
bundle, the AMX 30 B's APCR lists 390 vehicle damage and 150 internal-module
damage. The HWK 40 has 240 HP per track and a center divisor of 3. At the
nominal roll the simulation applies 390 HP at either end or 130 HP at the
center. The track damage zones and divisor are based on
[controlled live tracking tests](https://www.reddit.com/r/WorldofTanks/comments/1p5a0ry/the_mechanics_of_tracking_tanks/),
`Chassis.drivingWheelsSizes` in the client reference, and the extracted chassis
definition.

The scope covers the primary left and right tracks. Reserve tracks, active
repair-kit use, the situational Field Support perk, and artillery splash
damage to tracks remain open. See the
[archived battle mechanics](https://web.archive.org/web/20250303052442/https://wiki.wargaming.net/en/Battle_Mechanics)
and [current crew guide](https://worldoftanks.eu/en/content/guide/game-mechanics-and-achievements/crew-training/).

### Hit interpretation and voice cues

A stopped tracer, ricochet, non-penetration or external-module hit can all
produce effects with zero hull damage. An apparent shell passing through a
vehicle can also result from prediction, missing tracer effects, a gap in
collision geometry or a cosmetic part with no collision. Compare server
contact and the effects seen by shooter and target before assigning a cause.

A track can take damage with zero hull HP loss, including when a shell
continues into the hull and ricochets. The driver's
`Avatar.showOwnVehicleHitDirection` sends zero HP damage with the track
critical or destroyed bit and the blocked flag cleared. The client's
`crits_mask_parser` maps that bit to the track icon in the critical-hit
direction indicator. The shooter receives the chassis damage and device
penetration flags in `Avatar.showShotResults` for the track-hit cue.
For a track-only hit, `Vehicle.showDamageFromShot` uses effect 5
(`CRITICAL_HIT`) at the chassis contact so the client feedback adaptor
selects the chassis critical marker. The hull result still controls projectile
flight and persistent armor marks. Hull HP hits keep their penetration or
damage effect. The decoded packet regression covers bare-track hits, hull
blocks and hull ricochets. Native rendering remains unverified.

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
- **Tracers and flight.** Connect the strategic camera viewport to the hidden-source tracer
  policy.
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
[eu-concealment-video]: https://www.youtube.com/watch?v=rgEE7L7jXvI
[blitz-spotting]: https://wargaming.net/support/en/products/wotb/article/15414/
[eu-cadence]: https://worldoftanks.eu/en/news/general-news/version-916-spotting-improvement/
[eu-penetration]: https://worldoftanks.eu/en/content/guide/newcomers-guide/how_to_survive/
[eu-he-support]: https://wargaming.net/support/en/products/wot/article/15053/
[eu-he]: https://worldoftanks.eu/en/news/general-news/1-13-HE-shells/
[eu-movement]: https://worldoftanks.eu/en/news/general-news/ver914-new-physics-final/
[eu-recovery]: https://worldoftanks.eu/en/content/docs/release_notes/release-notes-1-22/

[wg-archived-battle-mechanics]: https://web.archive.org/web/20250303052442/https://wiki.wargaming.net/en/Battle_Mechanics
