use std::{collections::HashMap, hash::Hash};

mod settings;
pub use settings::{TrainingRoomSettings, VoiceChatMode};

const MAX_TEAM_PLAYERS: usize = 15;
const MAX_TEAM_OBSERVERS: usize = 5;
pub const MAX_ROOM_PLAYERS: usize = 2 * MAX_TEAM_PLAYERS;
pub const MAX_ASSIGNED_PLAYERS: usize = MAX_TEAM_PLAYERS + MAX_TEAM_OBSERVERS;
const MAX_INVITE_RECIPIENTS: usize = 100;
const MAX_INVITE_COMMENT_CHARS: usize = 400;

pub fn valid_invitation_comment(comment: &str, recipients: usize) -> bool {
    (1..=MAX_INVITE_RECIPIENTS).contains(&recipients)
        && comment.len() <= MAX_INVITE_COMMENT_CHARS * 4
        && comment.chars().count() <= MAX_INVITE_COMMENT_CHARS
        && !comment.chars().any(|c| c.is_control() && c != '\n')
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RosterSlot {
    TeamOne,
    TeamTwo,
    UnassignedFromTeamOne,
    UnassignedFromTeamTwo,
}

impl RosterSlot {
    pub fn assigned_team(self) -> Option<u8> {
        match self {
            Self::TeamOne => Some(1),
            Self::TeamTwo => Some(2),
            Self::UnassignedFromTeamOne | Self::UnassignedFromTeamTwo => None,
        }
    }

    fn swap_team(self) -> Self {
        match self {
            Self::TeamOne => Self::TeamTwo,
            Self::TeamTwo => Self::TeamOne,
            slot => slot,
        }
    }

    fn capacity(self) -> usize {
        if self.assigned_team().is_some() {
            MAX_ASSIGNED_PLAYERS
        } else {
            MAX_ROOM_PLAYERS
        }
    }
}

pub struct RoomMember<D> {
    pub delivery: D,
    pub account_id: i64,
    pub name: String,
    pub joined_at: f64,
    in_battle: bool,
    ready_vehicle: Option<Vec<u8>>,
    ready_inventory: Option<i64>,
    roster: RosterSlot,
}

impl<D> RoomMember<D> {
    pub fn new(delivery: D, account_id: i64, name: String, joined_at: f64) -> Self {
        Self {
            delivery,
            account_id,
            name,
            joined_at,
            in_battle: false,
            ready_vehicle: None,
            ready_inventory: None,
            roster: RosterSlot::UnassignedFromTeamOne,
        }
    }

    pub fn in_battle(&self) -> bool {
        self.in_battle
    }

    pub fn ready_inventory(&self) -> Option<i64> {
        self.ready_inventory
    }

    pub fn ready_vehicle(&self) -> Option<&[u8]> {
        self.ready_vehicle.as_deref()
    }

    pub fn return_to_room(&mut self, delivery: D) {
        self.delivery = delivery;
        self.in_battle = false;
        self.ready_vehicle = None;
    }

    pub fn begin_round(&mut self, entering_battle: bool) {
        self.ready_inventory = None;
        if entering_battle {
            self.in_battle = true;
        } else {
            self.ready_vehicle = None;
        }
    }

    pub fn roster(&self) -> RosterSlot {
        self.roster
    }
}

pub enum JoinRejection {
    InvitationUnavailable,
    InvitationOnly,
    Full,
}

pub struct TrainingRoom<K, D, I> {
    id: u32,
    created_at: f64,
    host: K,
    settings: TrainingRoomSettings,
    members: HashMap<K, RoomMember<D>>,
    invites: HashMap<i64, I>,
    teams_ready: (bool, bool),
}

impl<K: Copy + Eq + Hash, D, I> TrainingRoom<K, D, I> {
    pub(crate) fn new(
        id: u32,
        settings: TrainingRoomSettings,
        host: K,
        member: RoomMember<D>,
    ) -> Self {
        Self {
            id,
            created_at: member.joined_at,
            host,
            settings,
            members: HashMap::from([(host, member)]),
            invites: HashMap::new(),
            teams_ready: (false, false),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn created_at(&self) -> f64 {
        self.created_at
    }

    pub fn host(&self) -> K {
        self.host
    }

    pub fn settings(&self) -> &TrainingRoomSettings {
        &self.settings
    }

    pub fn change_settings(&mut self, settings: TrainingRoomSettings) -> bool {
        if self.settings == settings {
            return false;
        }
        self.settings = settings;
        self.clear_team_readiness();
        true
    }

    pub fn members(&self) -> &HashMap<K, RoomMember<D>> {
        &self.members
    }

    pub fn members_mut(&mut self) -> impl Iterator<Item = (&K, &mut RoomMember<D>)> {
        self.members.iter_mut()
    }

    pub fn member_mut(&mut self, key: &K) -> Option<&mut RoomMember<D>> {
        self.members.get_mut(key)
    }

    pub fn into_members(self) -> HashMap<K, RoomMember<D>> {
        self.members
    }

    pub fn invitations(&self) -> &HashMap<i64, I> {
        &self.invites
    }

    pub fn invitation_mut(&mut self, account: i64) -> Option<&mut I> {
        self.invites.get_mut(&account)
    }

    pub fn can_invite(&self, account: i64) -> bool {
        self.invites.len() < MAX_INVITE_RECIPIENTS
            && account > 0
            && !self.invites.contains_key(&account)
            && !self
                .members
                .values()
                .any(|member| member.account_id == account)
    }

    pub fn invite(&mut self, account: i64, invitation: I) {
        if self.can_invite(account) {
            self.invites.insert(account, invitation);
        }
    }

    pub fn remove_invitation(&mut self, account: i64) -> Option<I> {
        self.invites.remove(&account)
    }

    #[tracing::instrument(name = "training_rooms.join", skip_all, fields(
        ddd.bounded_context.name = "training_rooms", input.room.id = self.id,
        user.id = member.account_id, input.invitation = by_invitation,
        output.outcome = tracing::field::Empty, output.players.count = tracing::field::Empty
    ))]
    pub fn join(
        &mut self,
        key: K,
        member: RoomMember<D>,
        by_invitation: bool,
    ) -> Result<bool, JoinRejection> {
        let rejection = if by_invitation && !self.invites.contains_key(&member.account_id) {
            Some((
                JoinRejection::InvitationUnavailable,
                "invitation_unavailable",
            ))
        } else if !by_invitation && !self.settings.open {
            Some((JoinRejection::InvitationOnly, "invitation_only"))
        } else if self.members.len() >= MAX_ROOM_PLAYERS {
            Some((JoinRejection::Full, "room_full"))
        } else {
            None
        };
        let span = tracing::Span::current();
        if let Some((reason, outcome)) = rejection {
            span.record("output.outcome", outcome);
            return Err(reason);
        }
        let invited = self.invites.remove(&member.account_id).is_some();
        self.members.insert(key, member);
        span.record("output.outcome", "joined");
        span.record("output.players.count", self.members.len());
        Ok(invited)
    }

    pub(crate) fn leave(&mut self, key: &K) -> Option<RoomMember<D>> {
        let member = self.members.remove(key)?;
        if member.roster.assigned_team().is_some() {
            self.clear_team_readiness();
        }
        Some(member)
    }

    fn can_assign(&self, key: &K, destination: RosterSlot) -> bool {
        self.members.get(key).is_some_and(|member| {
            member.roster == destination
                || self
                    .members
                    .values()
                    .filter(|member| member.roster == destination)
                    .count()
                    < destination.capacity()
        })
    }

    #[tracing::instrument(name = "training_rooms.assign_roster", skip_all, fields(
        ddd.bounded_context.name = "training_rooms", input.room.id = self.id,
        input.team = destination.assigned_team(), output.outcome = tracing::field::Empty
    ))]
    pub fn assign(&mut self, key: &K, destination: RosterSlot) -> bool {
        let span = tracing::Span::current();
        if !self.can_assign(key, destination) {
            span.record("output.outcome", "roster_full");
            return false;
        }
        if let Some(member) = self.members.get_mut(key) {
            if member.roster == destination {
                span.record("output.outcome", "unchanged");
            } else {
                member.roster = destination;
                self.clear_team_readiness();
                span.record("output.outcome", "assigned");
            }
            return true;
        }
        false
    }

    pub fn swap_teams(&mut self) -> bool {
        let mut changed = false;
        for member in self.members.values_mut() {
            let destination = member.roster.swap_team();
            changed |= destination != member.roster;
            member.roster = destination;
        }
        if changed {
            self.clear_team_readiness();
        }
        changed
    }

    pub fn set_member_ready(
        &mut self,
        key: &K,
        inventory: Option<i64>,
        descriptor: Option<Vec<u8>>,
    ) {
        if let Some(member) = self.members.get_mut(key) {
            let changed = member.ready_inventory != inventory || member.ready_vehicle != descriptor;
            member.ready_inventory = inventory;
            member.ready_vehicle = descriptor;
            if changed && member.roster.assigned_team().is_some() {
                self.clear_team_readiness();
            }
        }
    }

    #[tracing::instrument(name = "training_rooms.set_team_ready", skip_all, fields(
        ddd.bounded_context.name = "training_rooms", input.room.id = self.id,
        input.team = team, input.ready = ready, output.outcome = tracing::field::Empty
    ))]
    pub fn set_team_ready(&mut self, team: u8, ready: bool, observer_inventory: i64) -> bool {
        if ready {
            let mut assigned = self
                .members
                .values()
                .filter(|member| member.roster.assigned_team() == Some(team));
            let mut has_vehicle = false;
            if !assigned.all(|member| {
                has_vehicle |= member
                    .ready_inventory
                    .is_some_and(|id| id != observer_inventory);
                member.ready_inventory.is_some()
            }) || !has_vehicle
            {
                tracing::Span::current().record("output.outcome", "team_not_ready");
                return false;
            }
        }
        match team {
            1 => self.teams_ready.0 = ready,
            2 => self.teams_ready.1 = ready,
            _ => return false,
        }
        tracing::Span::current().record(
            "output.outcome",
            if ready { "team_ready" } else { "team_unlocked" },
        );
        true
    }

    pub fn clear_team_readiness(&mut self) {
        self.teams_ready = (false, false);
    }

    pub fn teams_ready(&self) -> bool {
        self.teams_ready == (true, true)
    }
}
