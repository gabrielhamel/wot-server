use super::super::*;
use super::{TRAINING, now};

pub(in super::super) const ACCEPT_INVITE: i16 = 417;
pub(in super::super) const DECLINE_INVITE: i16 = 418;

const MAX_RECIPIENTS: usize = 100;
const MAX_COMMENT_CHARS: usize = 400;
const INVITE_COOLDOWN: Duration = Duration::from_secs(8);

pub(super) struct Invitation {
    payload: Value,
    queued_by: Option<tracing::Span>,
}

#[derive(Clone, Copy)]
pub(super) enum State {
    Active = 1,
    Accepted = 2,
    Declined = 3,
    Expired = 4,
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.send_training_invites", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.recipients.count = recipients.len(), input.string.bytes = comment.len(), output.room.id = tracing::field::Empty, output.recipients.count = tracing::field::Empty, output.invites.queued_count = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty))]
    pub(in super::super) fn send_training_invites(
        &mut self,
        client: ClientKey,
        recipients: &[i64],
        comment: &[u8],
    ) {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get_mut(&client) else {
            return;
        };
        let Some(room) = session
            .training_room
            .and_then(|id| self.training_rooms.get_mut(&id))
            .filter(|room| room.host == client)
        else {
            span.record("output.outcome", "host_required");
            return;
        };
        span.record("output.room.id", room.id);
        let comment = std::str::from_utf8(comment).ok().filter(|text| {
            text.len() <= MAX_COMMENT_CHARS * 4
                && text.chars().count() <= MAX_COMMENT_CHARS
                && !text.chars().any(|c| c.is_control() && c != '\n')
        });
        let Some(comment) = comment.filter(|_| (1..=MAX_RECIPIENTS).contains(&recipients.len()))
        else {
            span.record("output.outcome", "invalid_request");
            return;
        };
        let received_at = Instant::now();
        if received_at < session.training_invite_next {
            span.record("output.outcome", "rate_limited");
            return;
        }
        session.training_invite_next = received_at + INVITE_COOLDOWN;
        let host_id = session.account.snapshot.id();
        let invite = crate::pickle!({
            "createTime": (now()),
            "type": (TRAINING),
            "comment": (comment),
            "creator": (session.account.snapshot.display_name()),
            "creatorDBID": (host_id),
            "state": (State::Active as i32),
            "count": 1,
            "extraData": {"arenaTypeID": (room.settings.arena_type_id)},
        });
        let mut sent = 0;
        let mut queued = 0;
        let mut failed = false;
        for &account_id in recipients {
            if account_id <= 0
                || room
                    .members
                    .values()
                    .any(|member| member.account_id == account_id)
                || room.invites.contains_key(&account_id)
            {
                continue;
            }
            if room.invites.len() >= MAX_RECIPIENTS {
                break;
            }
            let recipient = self.sessions.values().find(|session| {
                session.account.snapshot.id() == account_id && session.garage_synced
            });
            let delivery = recipient.and_then(|session| {
                session.entity.map(|entity| {
                    publish(
                        &mut self.base,
                        session,
                        entity,
                        room.id,
                        self.config.periphery_id,
                        State::Active,
                        invite.clone(),
                    )
                })
            });
            let queued_by = match delivery {
                Some(Ok(())) => {
                    sent += 1;
                    None
                }
                result => {
                    if let Some(Err(error)) = result {
                        failed = true;
                        error!(%error, "Training invitation delivery failed");
                    }
                    queued += 1;
                    Some(span.clone())
                }
            };
            room.invites.insert(
                account_id,
                Invitation {
                    payload: invite.clone(),
                    queued_by,
                },
            );
        }
        span.record("output.invites.queued_count", queued);
        span.record("output.recipients.count", sent);
        span.record(
            "output.outcome",
            match (sent, failed) {
                (_, true) => "delivery_failed",
                (0, false) if queued > 0 => "queued",
                (0, false) => "no_recipients",
                _ => "sent",
            },
        );
        if failed {
            span.record("otel.status_code", "ERROR");
        }
    }

    #[tracing::instrument(name = "player_sessions.deliver_pending_training_invites", skip_all, fields(
        ddd.bounded_context.name = "player_sessions", input.entity_id = entity.id().0,
        output.invites.delivered_count = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ))]
    pub(in super::super) fn deliver_pending_training_invites(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
    ) {
        let Some(session) = self.sessions.get(&client) else {
            return;
        };
        let span = tracing::Span::current();
        let mut delivered = 0;
        for room in self.training_rooms.values_mut() {
            let Some(invite) = room.invites.get_mut(&session.account.snapshot.id()) else {
                continue;
            };
            if let Some(queued_by) = &invite.queued_by {
                span.follows_from(queued_by);
            }
            match publish(
                &mut self.base,
                session,
                entity,
                room.id,
                self.config.periphery_id,
                State::Active,
                invite.payload.clone(),
            ) {
                Ok(()) => {
                    invite.queued_by = None;
                    delivered += 1;
                }
                Err(error) => {
                    span.record("otel.status_code", "ERROR");
                    error!(%error, "Pending Training invitation delivery failed");
                }
            }
        }
        span.record("output.invites.delivered_count", delivered);
    }

    #[tracing::instrument(name = "player_sessions.decline_training_invite", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = args[0], input.cluster.id = args[1], output.outcome = tracing::field::Empty))]
    pub(in super::super) fn decline_training_invite(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        args: [i64; 3],
    ) {
        let span = tracing::Span::current();
        let [id, cluster, 0] = args else {
            span.record("output.outcome", "invalid_request");
            return;
        };
        let Some(room) = u32::try_from(id)
            .ok()
            .filter(|_| cluster == i64::from(self.config.periphery_id))
            .and_then(|id| self.training_rooms.get_mut(&id))
        else {
            span.record("output.outcome", "invitation_unavailable");
            return;
        };
        if let Some(session) = self.sessions.get(&client)
            && room
                .invites
                .remove(&session.account.snapshot.id())
                .is_some()
        {
            settle(
                &mut self.base,
                session,
                entity,
                room.id,
                self.config.periphery_id,
                State::Declined,
            );
            span.record("output.outcome", "declined");
        } else {
            span.record("output.outcome", "invitation_unavailable");
        }
    }
}

pub(super) fn settle(
    base: &mut base::App,
    session: &Session,
    entity: EntityKey<Account>,
    room_id: u32,
    periphery_id: u32,
    state: State,
) {
    let patch = crate::pickle!({"state": (state as i32)});
    if let Err(error) = publish(base, session, entity, room_id, periphery_id, state, patch) {
        error!(%error, "Training invitation status delivery failed");
    }
}

#[tracing::instrument(name = "player_sessions.publish_training_invite", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = room_id, input.cluster.id = periphery_id, input.invite.state = state as i32, input.revision = session.account.snapshot.revision(), otel.status_code = tracing::field::Empty), err)]
fn publish(
    base: &mut base::App,
    session: &Session,
    entity: EntityKey<Account>,
    room_id: u32,
    periphery_id: u32,
    state: State,
    patch: Value,
) -> Result<()> {
    let revision = session.account.snapshot.revision();
    let key = crate::pickle!(((room_id), (periphery_id)));
    let diff = crate::pickle!({
        "prevRev": (revision),
        "rev": (revision),
        "prebattleInvites": {(key): (patch)},
    });
    base.entity(entity)?.update(Pickle(diff)).inspect_err(|_| {
        tracing::Span::current().record("otel.status_code", "ERROR");
    })?;
    Ok(())
}
