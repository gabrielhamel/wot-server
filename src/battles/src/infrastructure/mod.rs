//! Live arena orchestration and replication through its delivery port.

use super::{
    application::{
        AvatarCommand, BattleEntityIds, BattleLaunch, BattleMode, ParticipantId, VehicleLoadout,
    },
    domain::{
        parameters::BattleParameters,
        round::{Round, RoundPhase},
        vehicle::Tank,
    },
};
use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tracing::error;
use wot_platform::{
    client_formats::pickle::{self, Value},
    mercury::{
        app::{
            client::element::arena::{CreateArenaEntity, LeaveArenaEntity, VehiclePosition},
            dispatch::ScriptDispatch,
            entity::EntityId,
        },
        script::Value as ScriptValue,
    },
};
use wot_resources::ResourceBundle;

use avatar::AvatarUpdate;
use delivery::ArenaDelivery;
use maps::ArenaMap;
use projectiles::PendingImpact;
use wire::{ArenaEntry, ArenaWire, DamageStickers, ShotImpact};

mod avatar;
mod controls;
mod delivery;
mod explosions;
mod maps;
pub(crate) mod mercury;
mod motion;
mod obstacles;
mod preparation;
#[cfg(test)]
mod projectile_tests;
mod projectiles;
mod scenery;
mod terrain;
mod vehicle_damage;
mod wire;
mod worker;

pub use delivery::BattleDelivery;
pub use preparation::BattlePrepared;
pub use worker::{BattleOutput, BattleQueueError, BattleStart, BattleWorker};

const ARENA_PERIOD: u8 = 3;
const AFTERBATTLE_PERIOD: Duration = Duration::from_secs(60);
const ELIMINATION: u8 = 1;
const TIME_LIMIT: u8 = 3;
const TECHNICAL: u8 = 5;
pub const TICK: Duration = Duration::from_millis(100);

struct Entrant {
    season: wot_shared_kernel::ArenaSeason,
    parameters: BattleParameters,
    account_id: i64,
    name: String,
    team: u8,
    is_host: bool,
    observer: bool,
    stationary_dummy: bool,
    customization_display_type: u8,
    vehicle: VehicleLoadout,
    avatar_id: u32,
    vehicle_id: u32,
    info_id: u32,
    position: Vec3,
    direction: Vec3,
}

impl Entrant {
    fn tank(&self) -> Tank {
        Tank::new(
            self.parameters,
            self.position,
            self.direction,
            self.vehicle.health,
            self.vehicle.ammunition.clone(),
            self.vehicle.performance.clone(),
        )
    }

    fn initial_gun_angles(&self) -> u16 {
        self.tank().packed_gun_angles()
    }

    fn sample() -> Self {
        Self {
            season: Default::default(),
            parameters: BattleParameters::default(),
            account_id: 1,
            name: "Arena Player".into(),
            team: 1,
            is_host: true,
            observer: false,
            stationary_dummy: false,
            customization_display_type: 2,
            vehicle: VehicleLoadout {
                descriptor: vec![1],
                outfits: Default::default(),
                health: 100,
                crew_size: 3,
                ammunition: vec![(10, 5)],
                performance: None,
            },
            avatar_id: 1,
            vehicle_id: 2,
            info_id: 3,
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
        }
    }
}

struct Battle {
    parameters: BattleParameters,
    map: ArenaMap,
    ground: Arc<terrain::Terrain>,
    obstacles: obstacles::Scenery,
    areas: scenery::SceneryAreas,
    arena_type: i32,
    players: HashMap<ParticipantId, Participant>,
    started_at: Instant,
    last_snapshot_tick: Option<u32>,
    full_snapshot_pending: bool,
    round: Round,
    launch: Option<tracing::Span>,
    shot_sequence: i32,
    projectiles: Vec<PendingImpact>,
    result: Option<(i8, u8)>,
    finish_span: Option<tracing::Span>,
    roster_count: usize,
}

struct Participant {
    vehicle: CreateArenaEntity,
    pose_cadence: PoseCadence,
    avatar_id: u32,
    roster_row: usize,
    team: u8,
    entry: Option<ArenaEntry>,
    entered: bool,
    ready: bool,
    tank: Tank,
    observer: bool,
    customization_display_type: u8,
    last_engine_mode: [u8; 2],
    last_wheels: [Vec<u8>; 2],
    last_gun_angles: u16,
    last_gun_marker: Option<crate::domain::vehicle::GunRay>,
    server_marker: bool,
    pending_marker: Option<tracing::Span>,
    pending_drive: Option<tracing::Span>,
    pending_aim: Option<tracing::Span>,
    pending_reload: Option<tracing::Span>,
    pending_stun: Option<(Instant, tracing::Span)>,
    active_fire: Option<vehicle_damage::ActiveFire>,
    device_repair_parent: Option<tracing::Span>,
    fire_roots: HashSet<usize>,
    device_hud_repair_parents: [Option<tracing::Span>; 4],
    damage_stickers: DamageStickers,
}

#[derive(Default)]
struct PoseCadence {
    previous: Option<(Vec3, Vec3)>,
    changed_at: u32,
}

impl PoseCadence {
    fn includes(&mut self, pose: (Vec3, Vec3), entity_id: u32, tick: u32, interval: u32) -> bool {
        if interval <= 1 {
            return true;
        }
        if self.previous != Some(pose) {
            self.previous = Some(pose);
            self.changed_at = tick;
        }
        tick.saturating_sub(self.changed_at) < interval || tick % interval == entity_id % interval
    }
}

impl Battle {
    #[tracing::instrument(name = "battles.roster_ready", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id, input.roster.row = row,
        input.ready = ready, output.recipients.count = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ))]
    fn publish_roster_ready(
        &self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        battle_id: u32,
        row: usize,
        ready: bool,
    ) {
        let span = tracing::Span::current();
        let mut delivered = 0;
        for (&peer, player) in self.players.iter().filter(|(_, player)| player.entered) {
            match wire.roster_ready(delivery, peer, self.roster_count, row, ready) {
                Ok(()) => delivered += 1,
                Err(error) => {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, entity_id = player.avatar_id, "Arena readiness delivery failed");
                }
            }
        }
        span.record("output.recipients.count", delivered);
    }

    fn ticks(&self, now: Instant) -> u32 {
        (now.saturating_duration_since(self.started_at).as_millis() / TICK.as_millis()) as u32
    }

    fn finish_if_team_left(&mut self, now: Instant) -> bool {
        if self.round.phase == RoundPhase::Finished {
            return false;
        }
        let team_present = |team| {
            self.players
                .values()
                .any(|player| player.team == team && !player.observer && player.tank.health > 0)
        };
        let result = match (team_present(1), team_present(2)) {
            (true, true) => return false,
            (true, false) => (1, ELIMINATION),
            (false, true) => (2, ELIMINATION),
            (false, false) => (0, TECHNICAL),
        };
        self.result = Some(result);
        self.round.finish(now);
        true
    }

    #[tracing::instrument(name = "battles.period", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.players.count = self.players.len(), output.phase = match self.round.phase {
            RoundPhase::Loading => "loading",
            RoundPhase::Countdown => "countdown",
            RoundPhase::Battle => "battle",
            RoundPhase::Finished => "finished",
        }, output.battle.winner_team = tracing::field::Empty,
        output.battle.finish_reason = tracing::field::Empty,
        output.delivered.count = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    ))]
    fn publish_period(&mut self, delivery: &mut impl ArenaDelivery, battle_id: u32, now: Instant) {
        let span = tracing::Span::current();
        if self.round.phase == RoundPhase::Finished {
            let (winner, reason) = self.result.unwrap_or((0, TIME_LIMIT));
            span.record("output.battle.winner_team", winner);
            span.record("output.battle.finish_reason", reason);
            self.finish_span = Some(span.clone());
        }
        let mut delivered = 0;
        for (&client, player) in &self.players {
            if !player.entered {
                continue;
            }
            match self.send_period(delivery, client, now) {
                Ok(()) => delivered += 1,
                Err(error) => {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Could not send battle period");
                    delivery.disconnect(client);
                }
            }
        }
        span.record("output.delivered.count", delivered);
    }

    fn send_period(
        &self,
        delivery: &mut impl ArenaDelivery,
        participant: ParticipantId,
        now: Instant,
    ) -> Result<()> {
        let (winner, reason) = self.result.unwrap_or((0, TIME_LIMIT));
        let (period, extra) = match self.round.phase {
            RoundPhase::Loading => return Ok(()),
            RoundPhase::Countdown => (2, Value::List(Vec::new())),
            RoundPhase::Battle => (3, Value::List(Vec::new())),
            RoundPhase::Finished => (
                4,
                wot_platform::pickle!(((i32::from(winner)), (i32::from(reason)))),
            ),
        };
        let length = if self.round.phase == RoundPhase::Finished {
            AFTERBATTLE_PERIOD
        } else {
            self.round.phase_length
        };
        let end = f64::from(self.ticks(now)) * TICK.as_secs_f64()
            + self
                .round
                .deadline
                .saturating_duration_since(now)
                .as_secs_f64()
            + length.saturating_sub(self.round.phase_length).as_secs_f64();
        let length = if self.round.phase == RoundPhase::Finished {
            Value::F64(length.as_secs_f64())
        } else {
            Value::I64(length.as_secs() as i64)
        };
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&pickle::to_bytes(&wot_platform::pickle!((
            (period),
            (end),
            (length),
            (extra)
        ))))?;
        if matches!(self.round.phase, RoundPhase::Finished) {
            delivery.send_avatar(participant, AvatarUpdate::RoundFinished { winner, reason })?;
        }
        delivery.send_avatar(
            participant,
            AvatarUpdate::Arena {
                kind: ARENA_PERIOD,
                data: encoder.finish()?,
            },
        )?;
        Ok(())
    }
}

/// Prepared wire state that becomes active after the launch is acknowledged.
pub struct PreparedBattle {
    id: u32,
    battle: Battle,
}

impl PreparedBattle {
    /// Allocate native chunk controllers from the gateway's arena entity ID sequence.
    pub fn assign_scenery_entities(&mut self, allocate: impl FnMut() -> u32) {
        self.battle.areas.assign(&self.battle.obstacles, allocate);
    }

    pub fn contains(&self, client: ParticipantId) -> bool {
        self.battle.players.contains_key(&client)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn participant_count(&self) -> usize {
        self.battle.players.len()
    }
}

pub enum AvatarAction {
    Stay,
    Leave,
}

pub enum ParticipantDeparture {
    Entered,
    Loading,
}

pub struct BattleRuntime {
    maps: HashMap<i32, Option<ArenaMap>>,
    resources: ResourceBundle,
    wire: Option<Arc<ArenaWire>>,
    terrain: HashMap<String, Weak<terrain::Terrain>>,
    obstacles: HashMap<i32, Weak<obstacles::Obstacles>>,
    next_id: u32,
}

impl BattleRuntime {
    fn load(
        dispatch: Arc<ScriptDispatch>,
        resources: &ResourceBundle,
        enabled: bool,
    ) -> Result<Self> {
        let maps = maps::load(resources).context("Load arena maps")?;
        let wire = if enabled && maps.values().any(Option::is_some) {
            Some(Arc::new(
                ArenaWire::bind(dispatch, resources).context("Validate arena layout")?,
            ))
        } else {
            None
        };
        Ok(Self {
            maps,
            terrain: HashMap::new(),
            obstacles: HashMap::new(),
            resources: resources.clone(),
            wire,
            next_id: 0,
        })
    }

    #[tracing::instrument(name = "battles.prepare", skip_all, fields(
        ddd.bounded_context.name = "battles", input.arena_type_id = launch.arena_type,
        input.round_length.seconds = launch.duration.as_secs(), input.players.count = launch.participants.len(),
        input.battle.load_timeout.seconds = launch.parameters.load_timeout_seconds,
        input.battle.countdown.seconds = launch.parameters.countdown_seconds,
        input.battle.results.seconds = launch.parameters.results_seconds,
        input.outfits.count = tracing::field::Empty,
        output.battle.id = tracing::field::Empty, output.players.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    fn prepare(
        &mut self,
        launch: BattleLaunch,
        entity_ids: Vec<BattleEntityIds>,
    ) -> Result<Option<PreparedBattle>> {
        let span = tracing::Span::current();
        let arena_type = launch.arena_type;
        let (Some(Some(map)), Some(wire)) = (self.maps.get(&arena_type), &self.wire) else {
            span.record("output.outcome", "arena_unavailable");
            return Ok(None);
        };
        let mut clients = HashSet::new();
        if entity_ids.len() != launch.participants.len()
            || !launch.valid_roster()
            || launch
                .participants
                .iter()
                .any(|player| !clients.insert(player.id))
        {
            span.record("output.outcome", "invalid_roster");
            return Ok(None);
        }
        let battle_id = self
            .next_id
            .checked_add(1)
            .context("Battle IDs exhausted")?;
        self.next_id = battle_id;
        span.record("output.battle.id", battle_id);
        self.terrain.retain(|_, ground| ground.strong_count() > 0);
        self.obstacles
            .retain(|_, obstacles| obstacles.strong_count() > 0);
        let ground = match self.terrain.get(&map.geometry).and_then(Weak::upgrade) {
            Some(ground) => ground,
            None => {
                let ground = Arc::new(terrain::Terrain::load(&self.resources, &map.geometry)?);
                self.terrain
                    .insert(map.geometry.clone(), Arc::downgrade(&ground));
                ground
            }
        };
        let obstacles = match self.obstacles.get(&arena_type).and_then(Weak::upgrade) {
            Some(obstacles) => obstacles,
            None => {
                let obstacles = Arc::new(obstacles::Obstacles::load(
                    &self.resources,
                    &map.geometry,
                    arena_type,
                )?);
                self.obstacles
                    .insert(arena_type, Arc::downgrade(&obstacles));
                obstacles
            }
        };
        span.record(
            "input.outfits.count",
            launch
                .participants
                .iter()
                .filter(|participant| {
                    !participant
                        .vehicle
                        .outfits
                        .for_season(map.season)
                        .is_empty()
                })
                .count(),
        );
        let mut entrants = Vec::with_capacity(launch.participants.len());
        let mut counts = [0usize; 2];
        for (player, ids) in launch.participants.into_iter().zip(entity_ids) {
            let (side, [from, to]) = if player.team == 1 {
                (0, map.spawns)
            } else {
                let [first, second] = map.spawns;
                (1, [second, first])
            };
            let forward = (to - from).normalize_or_zero();
            let across = Vec2::new(forward.y, -forward.x);
            let position =
                (from + across * (counts[side] as f32 * 5.0)).clamp(map.bounds[0], map.bounds[1]);
            let (position, direction) = ground
                .vehicle_pose(position, forward.x.atan2(forward.y))
                .context("Arena spawn has no terrain height")?;
            counts[side] += 1;
            entrants.push((
                player.id,
                Entrant {
                    season: map.season,
                    parameters: launch.parameters,
                    account_id: player.account_id,
                    name: player.name,
                    team: player.team,
                    is_host: player.is_host,
                    observer: player.observer,
                    stationary_dummy: player.stationary_dummy,
                    customization_display_type: player.customization_display_type,
                    vehicle: player.vehicle,
                    avatar_id: ids.avatar,
                    vehicle_id: ids.vehicle,
                    info_id: ids.info,
                    position,
                    direction,
                },
            ));
        }
        let roster = entrants
            .iter()
            .map(|(_, entrant)| entrant)
            .collect::<Vec<_>>();
        let players = entrants
            .iter()
            .enumerate()
            .map(|(roster_row, (client, player))| {
                Ok((
                    *client,
                    Participant {
                        vehicle: wire.vehicle(player)?,
                        pose_cadence: PoseCadence::default(),
                        avatar_id: player.avatar_id,
                        roster_row,
                        team: player.team,
                        entry: (!player.stationary_dummy)
                            .then(|| {
                                wire.entry(battle_id, arena_type, launch.mode, player, &roster)
                            })
                            .transpose()?,
                        entered: false,
                        ready: player.stationary_dummy,
                        tank: player.tank(),
                        observer: player.observer,
                        customization_display_type: player.customization_display_type,
                        last_engine_mode: [0; 2],
                        last_wheels: Default::default(),
                        last_gun_angles: player.initial_gun_angles(),
                        last_gun_marker: None,
                        server_marker: false,
                        pending_marker: None,
                        pending_drive: None,
                        pending_aim: None,
                        pending_reload: None,
                        pending_stun: None,
                        active_fire: None,
                        device_repair_parent: None,
                        fire_roots: HashSet::new(),
                        device_hud_repair_parents: Default::default(),
                        damage_stickers: DamageStickers::default(),
                    },
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        span.record("output.players.count", players.len());
        span.record("output.outcome", "prepared");
        let now = Instant::now();
        Ok(Some(PreparedBattle {
            id: battle_id,
            battle: Battle {
                parameters: launch.parameters,
                map: map.clone(),
                ground,
                obstacles: obstacles::Scenery::new(obstacles),
                areas: scenery::SceneryAreas::default(),
                arena_type,
                players,
                started_at: now,
                last_snapshot_tick: None,
                full_snapshot_pending: false,
                round: Round::new(now, launch.duration, &launch.parameters),
                launch: Some(tracing::Span::current()),
                shot_sequence: 0,
                projectiles: Vec::new(),
                result: None,
                finish_span: None,
                roster_count: roster.len(),
            },
        }))
    }
}

struct BattleSimulation {
    active: BTreeMap<u32, Battle>,
    engagements: HashMap<ParticipantId, u32>,
    wire: Option<Arc<ArenaWire>>,
}

impl BattleSimulation {
    #[tracing::instrument(name = "battles.activate", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = prepared.id,
        input.players.count = prepared.participant_count(), output.outcome = tracing::field::Empty,
    ))]
    fn start(&mut self, prepared: PreparedBattle) {
        let PreparedBattle { id, mut battle } = prepared;
        let now = Instant::now();
        battle.started_at = now;
        battle.round.deadline = now + battle.round.load_timeout;
        for (&client, player) in &battle.players {
            if player.entry.is_some() {
                self.engagements.insert(client, id);
            }
        }
        self.active.insert(id, battle);
        tracing::Span::current().record("output.outcome", "activated");
    }

    fn enter_with_delivery(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
    ) -> Result<()> {
        let Some(&battle_id) = self.engagements.get(&client) else {
            return Ok(());
        };
        let Some(battle) = self.active.get_mut(&battle_id) else {
            return Ok(());
        };
        let Some(player) = battle.players.get(&client).filter(|player| !player.entered) else {
            return Ok(());
        };
        let Some(entry) = &player.entry else {
            return Ok(());
        };
        let span = tracing::info_span!(
            parent: battle.launch.as_ref().unwrap_or(&tracing::Span::none()),
            "battles.enter",
            "ddd.bounded_context.name" = "battles",
            "input.battle.id" = battle_id,
            "input.arena_type_id" = battle.arena_type,
            "input.team" = player.team,
            "input.customization.display_type" = player.customization_display_type,
            "input.participant.id" = client.0,
            "input.entity_id" = player.avatar_id,
            "output.players.count" = battle.players.len(),
            "output.avatar_control" = tracing::field::Empty,
            "output.scenery.snapshot_chunks.count" = tracing::field::Empty,
            "output.fire.snapshot_count" = tracing::field::Empty,
            "output.outcome" = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
        );
        let mut fire_roots = HashSet::new();
        let mut fires = 0;
        let result = span.in_scope(|| -> Result<_> {
            delivery.create_avatar(client, player.avatar_id, entry.base_properties.clone())?;
            entry.publish(
                delivery,
                client,
                &battle.map,
                battle.players.values().map(|other| other.vehicle.clone()),
                battle.ticks(Instant::now()),
            )?;
            span.record("output.avatar_control", "server");
            if let Some(wire) = &self.wire {
                let scenery_chunks = battle.areas.snapshot(delivery, wire, client)?;
                span.record("output.scenery.snapshot_chunks.count", scenery_chunks);
                for (&other_peer, other) in &battle.players {
                    if other.tank.health != other.tank.max_health {
                        wire.vehicle_properties(
                            delivery,
                            client,
                            other.vehicle.entity_id,
                            [("health", ScriptValue::Int16(other.tank.health))],
                        )?;
                    }
                    if let Some(fire) = &other.active_fire {
                        let own = other_peer == client;
                        wire.publish_fire_start(
                            delivery,
                            client,
                            other.roster_row,
                            own,
                            fire,
                            (!own).then_some(other.vehicle.entity_id),
                        )?;
                        fires += 1;
                        if !own {
                            fire_roots.insert(other.roster_row);
                        }
                    }
                    if !other.damage_stickers.is_empty() {
                        wire.publish_damage_stickers(
                            delivery,
                            client,
                            other.vehicle.entity_id,
                            &other.damage_stickers,
                        )?;
                    }
                }
                for other in battle.players.values().filter(|player| player.ready) {
                    wire.roster_ready(
                        delivery,
                        client,
                        battle.roster_count,
                        other.roster_row,
                        true,
                    )?;
                }
            }
            battle.send_period(delivery, client, Instant::now())?;
            delivery.enable_arena_cell(client, player.vehicle.entity_id)?;
            Ok(())
        });
        span.in_scope(|| match result {
            Ok(()) => {
                if let Some(player) = battle.players.get_mut(&client) {
                    player.entered = true;
                    player.fire_roots = fire_roots;
                    span.record("output.fire.snapshot_count", fires);
                    battle.full_snapshot_pending = true;
                }
                if battle
                    .players
                    .values()
                    .all(|player| player.entered || player.entry.is_none())
                {
                    battle.launch = None;
                }
                span.record("output.outcome", "entered");
                Ok(())
            }
            Err(error) => {
                span.record("output.outcome", "delivery_failed");
                span.record("otel.status_code", "ERROR");
                delivery.disconnect(client);
                Err(error)
            }
        })
    }

    fn avatar_command_with_delivery(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        participant: ParticipantId,
        entity_id: u32,
        command: AvatarCommand,
    ) -> AvatarAction {
        let span = tracing::Span::current();
        let Some(&battle_id) = self.engagements.get(&participant) else {
            span.record("output.outcome", "arena_unavailable");
            return AvatarAction::Stay;
        };
        let Some(battle) = self.active.get_mut(&battle_id) else {
            span.record("output.outcome", "arena_unavailable");
            return AvatarAction::Stay;
        };
        let Some(player) = battle
            .players
            .get_mut(&participant)
            .filter(|player| player.entered && player.avatar_id == entity_id)
        else {
            span.record("output.outcome", "entity_unavailable");
            return AvatarAction::Stay;
        };
        match command {
            AvatarCommand::Ready => {
                let was_ready = player.ready;
                player.ready = true;
                let row = player.roster_row;
                if !was_ready && let Some(wire) = &self.wire {
                    battle.publish_roster_ready(delivery, wire, battle_id, row, true);
                }
                span.record(
                    "output.outcome",
                    if was_ready { "already_ready" } else { "ready" },
                );
                if battle.players.values().all(|participant| participant.ready) {
                    let now = Instant::now();
                    if battle.round.advance(now, true) {
                        battle.publish_period(delivery, battle_id, now);
                    }
                }
                AvatarAction::Stay
            }
            AvatarCommand::Leave => {
                span.record("output.outcome", "left_arena");
                AvatarAction::Leave
            }
        }
    }

    fn tick_with_delivery(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        now: Instant,
    ) -> Vec<(ParticipantId, tracing::Span)> {
        let mut ending = Vec::new();
        for (&battle_id, battle) in &mut self.active {
            if battle.round.ended(now) {
                ending.extend(
                    battle
                        .players
                        .iter()
                        .filter(|(_, player)| player.entry.is_some())
                        .map(|(&client, _)| {
                            (
                                client,
                                battle
                                    .finish_span
                                    .clone()
                                    .unwrap_or_else(tracing::Span::none),
                            )
                        }),
                );
                continue;
            }
            if let Some(wire) = &self.wire {
                battle.advance_projectiles(delivery, wire, battle_id, now);
                battle.expire_stuns(delivery, wire, now);
                if battle.round.phase == RoundPhase::Battle && now < battle.round.deadline {
                    battle.advance_damage(delivery, wire, battle_id, now);
                }
            }
            if battle.round.advance(now, false) {
                battle.publish_period(delivery, battle_id, now);
            }
            if battle.round.phase == RoundPhase::Finished
                && let Some(wire) = &self.wire
            {
                battle.end_round_fires(delivery, wire, battle_id);
            }
            let tick = battle.ticks(now);
            if battle.last_snapshot_tick == Some(tick) {
                continue;
            }
            battle.last_snapshot_tick = Some(tick);
            let mut movement = Vec::new();
            if matches!(
                battle.round.phase,
                RoundPhase::Battle | RoundPhase::Finished
            ) && let Some(wire) = &self.wire
            {
                movement = battle.advance_vehicles(delivery, wire, now);
            }
            let movement: Vec<_> = movement
                .into_iter()
                .map(|(entity_id, command)| {
                    tracing::info_span!(parent: &command, "battles.replication",
                        "ddd.bounded_context.name" = "battles", "input.entity_id" = entity_id,
                        "input.replication.kind" = "position", "input.arena.tick" = i64::from(tick),
                        "output.recipients.count" = tracing::field::Empty,
                        "output.delivered.count" = tracing::field::Empty,
                        "otel.status_code" = tracing::field::Empty)
                })
                .collect();
            let interval = battle.parameters.parked_vehicle_pose_interval_ticks;
            let full = std::mem::take(&mut battle.full_snapshot_pending);
            let mut recipients = Vec::new();
            let mut vehicles = Vec::new();
            for (&id, player) in &mut battle.players {
                let entity_id = player.vehicle.entity_id;
                let pose = (player.vehicle.position, player.vehicle.direction);
                let include = player
                    .pose_cadence
                    .includes(pose, entity_id, tick, interval)
                    || full;
                let position = VehiclePosition {
                    reference: 0,
                    entity_id,
                    position: pose.0,
                    direction: pose.1,
                };
                if player.entered {
                    let own_vehicle = (!include && !player.observer).then(|| position.clone());
                    recipients.push((id, own_vehicle));
                } else if player.entry.is_some()
                    && now >= battle.started_at + battle.round.load_timeout
                {
                    delivery.disconnect(id);
                }
                if include {
                    vehicles.push(position);
                }
            }
            if !recipients.is_empty() {
                delivery.publish_positions(delivery::BattlePositions {
                    battle_id,
                    tick: tick as u8,
                    queued_at: now,
                    recipients,
                    vehicles,
                    spans: movement,
                });
            }
        }
        ending
    }

    #[tracing::instrument(name = "battles.remove_participant", skip_all, fields(ddd.bounded_context.name = "battles", input.participant.id = client.0, output.battle.id = tracing::field::Empty, output.players.count = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    fn remove_participant_with_delivery(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
    ) -> Option<ParticipantDeparture> {
        let id = self.engagements.remove(&client)?;
        let battle = self.active.get_mut(&id)?;
        let removed = battle.players.remove(&client)?;
        let span = tracing::Span::current();
        span.record("output.battle.id", id);
        span.record("output.players.count", battle.players.len());
        if battle.players.values().all(|player| player.entry.is_none()) {
            self.active.remove(&id);
        } else {
            if removed.ready
                && let Some(wire) = &self.wire
            {
                battle.publish_roster_ready(delivery, wire, id, removed.roster_row, false);
            }
            for (&peer, player) in &battle.players {
                if player.entered
                    && let Err(error) = delivery.send_arena_element(
                        peer,
                        LeaveArenaEntity {
                            entity_id: removed.vehicle.entity_id,
                        },
                    )
                {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Could not remove departed battle vehicle");
                }
            }
            if battle
                .players
                .values()
                .all(|p| p.entered || p.entry.is_none())
            {
                battle.launch = None;
            }
            let now = Instant::now();
            if battle.finish_if_team_left(now) {
                battle.publish_period(delivery, id, now);
            }
        }
        Some(if removed.entered {
            ParticipantDeparture::Entered
        } else {
            ParticipantDeparture::Loading
        })
    }
}

#[cfg(test)]
mod pose_cadence_tests {
    use super::*;

    #[test]
    fn parked_poses_are_staggered_and_motion_resumes_immediately() {
        let mut cadence = PoseCadence::default();
        let parked = (Vec3::ZERO, Vec3::ZERO);
        let turning = (Vec3::ZERO, Vec3::Z);
        let entity = 12;

        assert!(cadence.includes(parked, entity, 1, 5));
        assert!((2..=5).all(|tick| cadence.includes(parked, entity, tick, 5)));
        assert!(!cadence.includes(parked, entity, 6, 5));
        assert!(cadence.includes(parked, entity, 7, 5));
        assert!(cadence.includes(turning, entity, 8, 5));
        assert!((9..=12).all(|tick| cadence.includes(turning, entity, tick, 5)));
        assert!(!cadence.includes(turning, entity, 13, 5));
        assert!(cadence.includes(turning, entity, 14, 1));
    }
}
