use crate::domain::{RoomMember, TrainingRoom, TrainingRoomSettings};
use std::{collections::BTreeMap, hash::Hash};

pub struct TrainingRooms<K, D, I> {
    next_after: u32,
    rooms: BTreeMap<u32, TrainingRoom<K, D, I>>,
}

pub enum RoomDeparture<K, D, I> {
    MemberLeft(RoomMember<D>),
    RoomClosed(TrainingRoom<K, D, I>),
}

impl<K, D, I> Default for TrainingRooms<K, D, I> {
    fn default() -> Self {
        Self {
            next_after: 0,
            rooms: BTreeMap::new(),
        }
    }
}

impl<K: Copy + Eq + Hash, D, I> TrainingRooms<K, D, I> {
    pub fn create(
        &mut self,
        settings: TrainingRoomSettings,
        host: K,
        member: RoomMember<D>,
    ) -> Option<u32> {
        let id = self.next_after.checked_add(1)?;
        self.rooms
            .insert(id, TrainingRoom::new(id, settings, host, member));
        self.next_after = id;
        Some(id)
    }

    pub fn get(&self, id: &u32) -> Option<&TrainingRoom<K, D, I>> {
        self.rooms.get(id)
    }

    pub fn get_mut(&mut self, id: &u32) -> Option<&mut TrainingRoom<K, D, I>> {
        self.rooms.get_mut(id)
    }

    #[tracing::instrument(name = "training_rooms.leave", skip_all, fields(
        ddd.bounded_context.name = "training_rooms", input.room.id = id,
        output.outcome = tracing::field::Empty, output.players.count = tracing::field::Empty
    ))]
    pub fn leave(&mut self, id: u32, key: &K) -> Option<RoomDeparture<K, D, I>> {
        let span = tracing::Span::current();
        let Some((room, member)) = self.rooms.get_mut(&id).and_then(|room| {
            let member = room.leave(key)?;
            Some((room, member))
        }) else {
            span.record("output.outcome", "already_left");
            return None;
        };
        if room.host() == *key {
            span.record("output.outcome", "deleted");
            span.record("output.players.count", 0);
            self.rooms.remove(&id).map(RoomDeparture::RoomClosed)
        } else {
            span.record("output.outcome", "left");
            span.record("output.players.count", room.members().len());
            Some(RoomDeparture::MemberLeft(member))
        }
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &TrainingRoom<K, D, I>> {
        self.rooms.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut TrainingRoom<K, D, I>> {
        self.rooms.values_mut()
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }
}
