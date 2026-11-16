use std::time::{Duration, Instant};
use wot_server::social::{Conversations, RequestBudget};

#[test]
fn private_rooms_authorize_both_participants_bound_capacity_and_reclaim_disconnects() {
    let mut rooms = Conversations::default();
    assert!(rooms.open(1, 1).is_none());
    let room = rooms.open(1, 2).unwrap();
    assert_eq!(rooms.open(2, 1).unwrap().id, room.id);
    assert!(rooms.between(1, 2).unwrap().is_subscribed(2));
    assert!(rooms.joined(room.id, 3).is_none());
    assert!(rooms.close(room.id, 3).is_none());
    assert!(rooms.joined(room.id, 2).is_some());
    for peer in 3..=17 {
        assert!(rooms.open(1, peer).is_some());
    }
    assert!(rooms.open(1, 18).is_none());
    assert_eq!(rooms.disconnect(1).len(), 16);
    assert!(rooms.joined(room.id, 2).is_none());
    assert!(rooms.open(2, 18).is_some());
}

#[test]
fn request_flood_is_bounded_and_recovers_after_window() {
    let mut budget = RequestBudget::default();
    let now = Instant::now();
    for _ in 0..32 {
        assert!(budget.admit(now));
    }
    for _ in 0..1000 {
        assert!(!budget.admit(now));
    }
    assert!(budget.admit(now + Duration::from_secs(1)));
}

#[test]
fn private_windows_open_and_close_independently_and_both_closed_reclaim_the_pair() {
    let mut rooms = Conversations::default();
    let first = rooms.open(1, 2).unwrap();
    assert!(first.is_subscribed(1));
    assert!(!first.is_subscribed(2));
    assert!(rooms.joined(first.id, 2).is_none());
    assert!(rooms.subscribe(first.id, 3).is_none());
    rooms.subscribe(first.id, 2).unwrap();
    rooms.close(first.id, 1).unwrap();
    assert!(rooms.joined(first.id, 1).is_none());
    assert!(rooms.joined(first.id, 2).is_some());
    let reopened = rooms.open(1, 2).unwrap();
    assert_eq!(reopened.id, first.id);
    assert!(reopened.is_subscribed(2));
    rooms.close(first.id, 1).unwrap();
    rooms.close(first.id, 2).unwrap();
    assert!(rooms.between(1, 2).is_none());
    assert_ne!(rooms.open(2, 1).unwrap().id, first.id);
}

#[test]
fn disconnect_of_unnotified_recipient_also_reclaims_the_conversation() {
    let mut rooms = Conversations::default();
    let room = rooms.open(1, 2).unwrap();
    assert!(!room.is_subscribed(2));
    assert_eq!(rooms.disconnect(2).len(), 1);
    assert!(rooms.between(1, 2).is_none());
}
