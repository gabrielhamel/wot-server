use super::super::*;
use super::{TRAINING, now};
use crate::infrastructure::schema::MessengerArgs;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;
use wot_platform::mercury::app::entity::Bytes;

const RESPONSE_SUCCESS: i16 = 0;
const RESPONSE_FAILURE: i16 = 1;
const INIT_UNIT_CHAT: i16 = 127;
const DEINIT_UNIT_CHAT: i16 = 128;
pub(in super::super) const CHAT_BROADCAST: i16 = 129;
const ON_UNIT_MESSAGE_BROADCAST: i16 = 130;

const NOT_READY: i32 = 2;
const IN_COOLDOWN: i32 = 3;
const WRONG_ARGS: i32 = 8;
const MAX_MESSAGE_CHARS: usize = 512;
const MESSAGE_COOLDOWN: Duration = Duration::from_millis(500);

#[tracing::instrument(name = "player_sessions.training_chat_membership", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.room.id = room_id, input.chat.joining = joining), err)]
pub(super) fn update_membership(
    base: &mut base::App,
    entity: EntityKey<Account>,
    room_id: u32,
    joining: bool,
) -> Result<()> {
    let (action, args) = if joining {
        // The fixed argument dictionary always carries strArg1. The client decodes it as history.
        let mut history = ZlibEncoder::new(Vec::new(), Compression::fast());
        history.write_all(&pickle::to_bytes(&Value::List(Vec::new())))?;
        (
            INIT_UNIT_CHAT,
            MessengerArgs {
                int32Arg1: TRAINING,
                strArg1: Bytes(history.finish()?),
                ..Default::default()
            },
        )
    } else {
        (DEINIT_UNIT_CHAT, MessengerArgs::default())
    };
    base.entity(entity)?
        .messenger_onActionByServer_chat2(action, 0, args)?;
    Ok(())
}

impl Gateway {
    #[tracing::instrument(name = "player_sessions.broadcast_training_chat", skip_all, fields(ddd.bounded_context.name = "player_sessions", input.request_id = request_id, input.text.bytes = message.len(), output.room.id = tracing::field::Empty, output.recipients.count = tracing::field::Empty, output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty), err)]
    pub(in super::super) fn broadcast_training_chat(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: u16,
        message: Vec<u8>,
    ) -> Result<()> {
        let span = tracing::Span::current();
        let Some(session) = self.sessions.get_mut(&client) else {
            return Ok(());
        };
        let room = session
            .training_room
            .and_then(|id| self.training_rooms.get(&id));
        let Some((room, sender)) =
            room.and_then(|room| room.members.get(&client).map(|sender| (room, sender)))
        else {
            span.record("output.outcome", "not_in_room");
            return self.reject_training_chat(entity, request_id, NOT_READY);
        };
        span.record("output.room.id", room.id);
        let valid = message.len() <= MAX_MESSAGE_CHARS * 4
            && std::str::from_utf8(&message).is_ok_and(|text| {
                !text.trim().is_empty()
                    && text.chars().count() <= MAX_MESSAGE_CHARS
                    && !text
                        .chars()
                        .any(|character| character.is_control() && character != '\n')
            });
        if !valid {
            span.record("output.outcome", "invalid_message");
            return self.reject_training_chat(entity, request_id, WRONG_ARGS);
        }
        let received_at = Instant::now();
        if received_at < session.message_next {
            span.record("output.outcome", "rate_limited");
            return self.reject_training_chat(entity, request_id, IN_COOLDOWN);
        }
        session.message_next = received_at + MESSAGE_COOLDOWN;
        let args = MessengerArgs {
            int64Arg1: sender.account_id,
            floatArg1: now(),
            strArg1: Bytes(message),
            strArg2: Bytes(sender.name.as_bytes().to_vec()),
            ..Default::default()
        };

        let mut recipients = 0;
        let garage_members = room.members.values().filter(|member| !member.in_battle);
        let expected = garage_members.clone().count();
        for member in garage_members {
            let delivery = self.base.entity(member.entity).and_then(|mut account| {
                account.messenger_onActionByServer_chat2(ON_UNIT_MESSAGE_BROADCAST, 0, args.clone())
            });
            match delivery {
                Ok(()) => recipients += 1,
                Err(error) => error!(%error, "Training chat delivery failed"),
            }
        }
        span.record("output.recipients.count", recipients);
        if recipients == expected {
            span.record("output.outcome", "broadcast");
        } else {
            span.record("output.outcome", "delivery_failed");
            span.record("otel.status_code", "ERROR");
        }
        self.base.entity(entity)?.messenger_onActionByServer_chat2(
            RESPONSE_SUCCESS,
            request_id,
            MessengerArgs::default(),
        )?;
        Ok(())
    }

    fn reject_training_chat(
        &mut self,
        entity: EntityKey<Account>,
        request_id: u16,
        error: i32,
    ) -> Result<()> {
        self.base.entity(entity)?.messenger_onActionByServer_chat2(
            RESPONSE_FAILURE,
            request_id,
            MessengerArgs {
                int32Arg1: error,
                ..Default::default()
            },
        )?;
        Ok(())
    }
}
