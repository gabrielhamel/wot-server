//! Live arena orchestration and its BaseApp transport adapter.

use super::{
    application::{BattleLaunch, BattleMode, VehicleLoadout},
    domain::{
        round::{LOAD_TIMEOUT, Round, RoundPhase},
        vehicle::Tank,
    },
};
use crate::platform::{
    client_formats::pickle::{self, Value},
    mercury::app::{
        base::{self, ClientKey},
        client::element::arena::{
            AvatarPosition, CreateArenaEntity, LeaveArenaEntity, VehiclePosition,
        },
        entity::{Bytes, EntityId, EntityKey, MethodDecodeError},
    },
};
use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::error;
use wot_resources::ResourceBundle;

use avatar::{Avatar, AvatarBase};
use maps::ArenaMap;
use wire::{ArenaEntry, ArenaWire, ShotImpact};

mod avatar;
mod controls;
mod maps;
mod terrain;
mod wire;

const ARENA_PERIOD: u8 = 3;
const TIME_LIMIT: u8 = 3;
pub(crate) const TICK: Duration = Duration::from_millis(100);

struct Entrant {
    account_id: i64,
    name: String,
    team: u8,
    is_host: bool,
    observer: bool,
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
            self.position,
            self.direction.z,
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
            account_id: 1,
            name: "Arena Player".into(),
            team: 1,
            is_host: true,
            observer: false,
            vehicle: VehicleLoadout {
                descriptor: vec![1],
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
    map: ArenaMap,
    ground: Arc<terrain::Terrain>,
    arena_type: i32,
    players: HashMap<ClientKey, Participant>,
    started_at: Instant,
    last_snapshot_tick: Option<u32>,
    round: Round,
    launch: Option<tracing::Span>,
    shot_sequence: i32,
    result: Option<(i8, u8)>,
    roster_count: usize,
}

struct Participant {
    vehicle: CreateArenaEntity,
    avatar_id: u32,
    roster_row: usize,
    team: u8,
    entry: ArenaEntry,
    avatar: Option<EntityKey<Avatar>>,
    ready: bool,
    tank: Tank,
    observer: bool,
    last_gun_angles: u16,
    pending_drive: Option<tracing::Span>,
    pending_aim: Option<tracing::Span>,
    pending_reload: Option<tracing::Span>,
}

impl Battle {
    #[tracing::instrument(name = "battles.roster_ready", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id, input.roster.row = row,
        input.ready = ready, output.recipients.count = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ))]
    fn publish_roster_ready(
        &self,
        base: &mut base::App,
        wire: &ArenaWire,
        battle_id: u32,
        row: usize,
        ready: bool,
    ) {
        let span = tracing::Span::current();
        let mut delivered = 0;
        for (&peer, player) in self
            .players
            .iter()
            .filter(|(_, player)| player.avatar.is_some())
        {
            match wire.roster_ready(base, peer, self.roster_count, row, ready) {
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

    fn has_both_teams(&self) -> bool {
        [1, 2]
            .into_iter()
            .all(|team| self.players.values().any(|player| player.team == team))
    }

    #[tracing::instrument(name = "battles.period", skip_all, fields(
        ddd.bounded_context.name = "battles", input.battle.id = battle_id,
        input.players.count = self.players.len(), output.phase = match self.round.phase {
            RoundPhase::Loading => "loading",
            RoundPhase::Countdown => "countdown",
            RoundPhase::Battle => "battle",
            RoundPhase::Finished => "finished",
        }, otel.status_code = tracing::field::Empty,
    ))]
    fn publish_period(&self, base: &mut base::App, battle_id: u32, now: Instant) {
        for (&client, player) in &self.players {
            if let Some(avatar) = player.avatar
                && let Err(error) = self.send_period(base, avatar, now)
            {
                tracing::Span::current().record("otel.status_code", "ERROR");
                error!(%error, "Could not send battle period");
                base.disconnect_client(client);
            }
        }
    }

    fn send_period(
        &self,
        base: &mut base::App,
        avatar: EntityKey<Avatar>,
        now: Instant,
    ) -> Result<()> {
        let (winner, reason) = self.result.unwrap_or((0, TIME_LIMIT));
        let (period, extra) = match self.round.phase {
            RoundPhase::Loading => return Ok(()),
            RoundPhase::Countdown => (2, Value::List(Vec::new())),
            RoundPhase::Battle => (3, Value::List(Vec::new())),
            RoundPhase::Finished => (
                4,
                crate::pickle!(((i32::from(winner)), (i32::from(reason)))),
            ),
        };
        let end = f64::from(self.ticks(now)) * TICK.as_secs_f64()
            + self
                .round
                .deadline
                .saturating_duration_since(now)
                .as_secs_f64();
        let length = self.round.phase_length.as_secs() as i64;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&pickle::to_bytes(&crate::pickle!((
            (period),
            (end),
            (length),
            (extra)
        ))))?;
        let mut avatar = base.entity(avatar)?;
        if matches!(self.round.phase, RoundPhase::Finished) {
            avatar.onRoundFinished(winner, reason)?;
        }
        avatar.updateArena(ARENA_PERIOD, Bytes(encoder.finish()?))?;
        Ok(())
    }
}

/// Prepared wire state that becomes active after the launch is acknowledged.
pub(crate) struct PreparedBattle {
    id: u32,
    battle: Battle,
}

impl PreparedBattle {
    pub(crate) fn contains(&self, client: ClientKey) -> bool {
        self.battle.players.contains_key(&client)
    }

    pub(crate) fn participant_count(&self) -> usize {
        self.battle.players.len()
    }
}

pub(crate) enum AvatarAction {
    Stay,
    Leave,
}

pub(crate) enum ParticipantDeparture {
    Entered,
    Loading,
}

pub(crate) struct BattleRuntime {
    maps: HashMap<i32, Option<ArenaMap>>,
    active: BTreeMap<u32, Battle>,
    engagements: HashMap<ClientKey, u32>,
    resources: ResourceBundle,
    wire: Option<ArenaWire>,
    next_id: u32,
}

impl BattleRuntime {
    pub(crate) fn bind(
        base: &mut base::App,
        resources: &ResourceBundle,
        enabled: bool,
    ) -> Result<Self> {
        let maps = maps::load(resources).context("Load arena maps")?;
        let wire = if enabled && maps.values().any(Option::is_some) {
            Some(ArenaWire::bind(base).context("Validate arena layout")?)
        } else {
            None
        };
        Ok(Self {
            maps,
            active: BTreeMap::new(),
            engagements: HashMap::new(),
            resources: resources.clone(),
            wire,
            next_id: 0,
        })
    }

    pub(crate) fn has_maps(&self) -> bool {
        !self.maps.is_empty()
    }

    pub(crate) fn has_map(&self, arena_type: i32) -> bool {
        self.maps.contains_key(&arena_type)
    }

    pub(crate) fn contains(&self, client: ClientKey) -> bool {
        self.engagements.contains_key(&client)
    }

    #[tracing::instrument(name = "battles.prepare", skip_all, fields(
        ddd.bounded_context.name = "battles", input.arena_type_id = launch.arena_type,
        input.round_length.seconds = launch.duration.as_secs(), input.players.count = launch.participants.len(),
        output.battle.id = tracing::field::Empty, output.players.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    pub(crate) fn prepare(
        &mut self,
        base: &mut base::App,
        launch: BattleLaunch,
    ) -> Result<Option<PreparedBattle>> {
        let span = tracing::Span::current();
        let arena_type = launch.arena_type;
        let (Some(Some(map)), Some(wire)) = (self.maps.get(&arena_type), &self.wire) else {
            span.record("output.outcome", "arena_unavailable");
            return Ok(None);
        };
        let mut clients = HashSet::new();
        if !launch.valid_roster()
            || launch.participants.iter().any(|player| {
                self.engagements.contains_key(&player.client) || !clients.insert(player.client)
            })
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
        let ground = if let Some(active) = self
            .active
            .values()
            .find(|active| active.map.geometry == map.geometry)
        {
            Arc::clone(&active.ground)
        } else {
            Arc::new(terrain::Terrain::load(&self.resources, &map.geometry)?)
        };
        let mut entrants = Vec::with_capacity(launch.participants.len());
        let mut counts = [0usize; 2];
        for player in launch.participants {
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
                player.client,
                Entrant {
                    account_id: player.account_id,
                    name: player.name,
                    team: player.team,
                    is_host: player.is_host,
                    observer: player.observer,
                    vehicle: player.vehicle,
                    avatar_id: base.arena_entity_id(),
                    vehicle_id: base.arena_entity_id(),
                    info_id: base.arena_entity_id(),
                    position,
                    direction,
                },
            ));
        }
        let roster = entrants
            .iter()
            .map(|(_, entrant)| entrant)
            .collect::<Vec<_>>();
        let dispatch = base.dispatch();
        let players = entrants
            .iter()
            .enumerate()
            .map(|(roster_row, (client, player))| {
                Ok((
                    *client,
                    Participant {
                        vehicle: wire.vehicle(dispatch, player)?,
                        avatar_id: player.avatar_id,
                        roster_row,
                        team: player.team,
                        entry: wire.entry(
                            dispatch,
                            battle_id,
                            arena_type,
                            launch.mode,
                            player,
                            &roster,
                        )?,
                        avatar: None,
                        ready: false,
                        tank: player.tank(),
                        observer: player.observer,
                        last_gun_angles: player.initial_gun_angles(),
                        pending_drive: None,
                        pending_aim: None,
                        pending_reload: None,
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
                map: map.clone(),
                ground,
                arena_type,
                players,
                started_at: now,
                last_snapshot_tick: None,
                round: Round::new(now, launch.duration),
                launch: Some(tracing::Span::current()),
                shot_sequence: 0,
                result: None,
                roster_count: roster.len(),
            },
        }))
    }

    pub(crate) fn start(&mut self, prepared: PreparedBattle) {
        let PreparedBattle { id, battle } = prepared;
        for &client in battle.players.keys() {
            self.engagements.insert(client, id);
        }
        self.active.insert(id, battle);
    }

    pub(crate) fn enter(&mut self, base: &mut base::App, client: ClientKey) -> Result<()> {
        let Some(&battle_id) = self.engagements.get(&client) else {
            return Ok(());
        };
        let Some(battle) = self.active.get_mut(&battle_id) else {
            return Ok(());
        };
        let Some(player) = battle
            .players
            .get(&client)
            .filter(|player| player.avatar.is_none())
        else {
            return Ok(());
        };
        let span = tracing::info_span!(
            parent: battle.launch.as_ref().unwrap_or(&tracing::Span::none()),
            "battles.enter",
            "ddd.bounded_context.name" = "battles",
            "input.battle.id" = battle_id,
            "input.arena_type_id" = battle.arena_type,
            "input.team" = player.team,
            "input.entity_id" = player.avatar_id,
            "output.players.count" = battle.players.len(),
            "output.avatar_control" = tracing::field::Empty,
            "output.outcome" = tracing::field::Empty,
            "otel.status_code" = tracing::field::Empty,
        );
        let result = span.in_scope(|| -> Result<_> {
            let avatar = base.create_base_player_with_id::<Avatar>(
                client,
                player.avatar_id,
                player.entry.base_properties.clone(),
            )?;
            player.entry.publish(
                base,
                client,
                &battle.map,
                battle.players.values().map(|other| other.vehicle.clone()),
                battle.ticks(Instant::now()),
            )?;
            span.record("output.avatar_control", "server");
            if let Some(wire) = &self.wire {
                for other in battle.players.values().filter(|player| player.ready) {
                    wire.roster_ready(base, client, battle.roster_count, other.roster_row, true)?;
                }
            }
            battle.send_period(base, avatar, Instant::now())?;
            base.enable_arena_cell(client, player.vehicle.entity_id);
            Ok(avatar)
        });
        span.in_scope(|| match result {
            Ok(avatar) => {
                if let Some(player) = battle.players.get_mut(&client) {
                    player.avatar = Some(avatar);
                }
                if battle
                    .players
                    .values()
                    .all(|player| player.avatar.is_some())
                {
                    battle.launch = None;
                }
                span.record("output.outcome", "entered");
                Ok(())
            }
            Err(error) => {
                span.record("output.outcome", "delivery_failed");
                span.record("otel.status_code", "ERROR");
                base.disconnect_client(client);
                Err(error)
            }
        })
    }

    pub(crate) fn avatar_method(
        &mut self,
        base: &mut base::App,
        event: &base::BaseMethodEvent,
    ) -> AvatarAction {
        let span = tracing::Span::current();
        let Some(battle_id) = self.engagements.get(&event.client).copied() else {
            span.record("output.outcome", "arena_unavailable");
            return AvatarAction::Stay;
        };
        let Some(battle) = self.active.get_mut(&battle_id) else {
            span.record("output.outcome", "arena_unavailable");
            return AvatarAction::Stay;
        };
        let Some(player) = battle.players.get_mut(&event.client).filter(|player| {
            player
                .avatar
                .is_some_and(|avatar| avatar.any() == event.entity)
        }) else {
            span.record("output.outcome", "entity_unavailable");
            return AvatarAction::Stay;
        };
        match base.decode::<AvatarBase>(event.entity, &event.call) {
            Ok((_, AvatarBase::SetClientReady {})) => {
                let was_ready = player.ready;
                player.ready = true;
                let row = player.roster_row;
                if !was_ready && let Some(wire) = &self.wire {
                    battle.publish_roster_ready(base, wire, battle_id, row, true);
                }
                span.record(
                    "output.outcome",
                    if was_ready { "already_ready" } else { "ready" },
                );
                if battle.players.values().all(|participant| participant.ready) {
                    let now = Instant::now();
                    if battle.round.advance(now, true) {
                        battle.publish_period(base, battle_id, now);
                    }
                }
                AvatarAction::Stay
            }
            Ok((_, AvatarBase::LeaveArena {})) => {
                span.record("output.outcome", "left_arena");
                AvatarAction::Leave
            }
            Err(error) => {
                let outcome = if matches!(error, MethodDecodeError::Unimplemented) {
                    "unimplemented"
                } else {
                    "decode_failed"
                };
                span.record("output.outcome", outcome);
                span.record("otel.status_code", "ERROR");
                error!(outcome, "Avatar base method could not be dispatched");
                AvatarAction::Stay
            }
        }
    }

    pub(crate) fn tick(&mut self, base: &mut base::App, now: Instant) -> Vec<ClientKey> {
        let mut ending = Vec::new();
        for (&battle_id, battle) in &mut self.active {
            if !battle.has_both_teams() || battle.round.ended(now) {
                ending.extend(battle.players.keys().copied());
                continue;
            }
            if battle.round.advance(now, false) {
                battle.publish_period(base, battle_id, now);
            }
            let tick = battle.ticks(now);
            if battle.last_snapshot_tick == Some(tick) {
                continue;
            }
            battle.last_snapshot_tick = Some(tick);
            let mut movement = Vec::new();
            if matches!(battle.round.phase, RoundPhase::Battle)
                && let Some(wire) = &self.wire
            {
                movement = battle.advance_vehicles(base, wire, now);
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
            let mut recipients = 0_i64;
            let mut delivered = 0_i64;
            for (&client, player) in &battle.players {
                if player.avatar.is_some() {
                    let avatar = AvatarPosition {
                        reference: 0,
                        // The Avatar rides its Vehicle, so its own pose is relative to the hull.
                        position: Vec3::ZERO,
                        direction: Vec3::ZERO,
                    };
                    let vehicles = battle.players.values().map(|other| VehiclePosition {
                        reference: 0,
                        entity_id: other.vehicle.entity_id,
                        position: other.vehicle.position,
                        direction: other.vehicle.direction,
                    });
                    recipients += 1;
                    match base.arena_tick(client, tick as u8, avatar, vehicles) {
                        Ok(()) => delivered += 1,
                        Err(error) => {
                            error!(%error, battle_id, "Could not send battle tick");
                            base.disconnect_client(client);
                        }
                    }
                } else if now >= battle.started_at + LOAD_TIMEOUT {
                    base.disconnect_client(client);
                }
            }
            for span in movement {
                span.record("output.recipients.count", recipients);
                span.record("output.delivered.count", delivered);
                if delivered != recipients {
                    span.record("otel.status_code", "ERROR");
                    span.in_scope(|| error!("Arena pose snapshot delivery failed"));
                }
            }
        }
        ending
    }

    #[tracing::instrument(name = "battles.remove_participant", skip_all, fields(ddd.bounded_context.name = "battles", input.client.address = %client, output.battle.id = tracing::field::Empty, output.players.count = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    pub(crate) fn remove_participant(
        &mut self,
        base: &mut base::App,
        client: ClientKey,
    ) -> Option<ParticipantDeparture> {
        let id = self.engagements.remove(&client)?;
        let battle = self.active.get_mut(&id)?;
        let removed = battle.players.remove(&client)?;
        let span = tracing::Span::current();
        span.record("output.battle.id", id);
        span.record("output.players.count", battle.players.len());
        if battle.players.is_empty() {
            self.active.remove(&id);
        } else {
            if removed.ready
                && let Some(wire) = &self.wire
            {
                battle.publish_roster_ready(base, wire, id, removed.roster_row, false);
            }
            for (&peer, player) in &battle.players {
                if player.avatar.is_some()
                    && let Err(error) = base.send_arena_element(
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
            if battle.players.values().all(|p| p.avatar.is_some()) {
                battle.launch = None;
            }
        }
        Some(if removed.avatar.is_some() {
            ParticipantDeparture::Entered
        } else {
            ParticipantDeparture::Loading
        })
    }
}
