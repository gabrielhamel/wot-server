use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use wot_platform::mercury::{
    app::{
        dispatch::ScriptDispatch,
        entity::{Bytes, EntityId, Pickle, ScriptType, dict_check, dict_field_ty, entity_schema},
    },
    script::{PropertyFlags, StringValue, Ty, Value},
};
use wot_platform::script_dict;
entity_schema! {
    pub Account {
        client {
            fn onChatAction(data: ChatActionData);
            fn messenger_onActionByServer_chat2(action: i16, request: u16, args: MessengerArgs);
            fn showGUI(ctx: Pickle);
            fn onKickedFromServer(reason: String, kind: u8, expires_at: u32);
            fn onArenaCreated();
            fn onPrebattleJoined(room_id: EntityId);
            fn onPrebattleJoinFailure(reason: u8);
            fn onPrebattleLeft();
            fn onKickedFromPrebattle(reason: u8);
            fn updatePrebattle(update_type: u8, data: Pickle);
            fn onCmdResponse(request_id: i16, result_id: i16, error: String);
            fn onCmdResponseExt(request_id: i16, result_id: i16, error: String, ext: Pickle);
            fn update(diff: Pickle);
            fn receiveServerStats(stats: ServerStats);
            fn receiveQueueInfo(info: Pickle);
            fn onEnqueued(queue_type: u8);
            fn onTokenReceived(request_id: u16, token_type: u8, data: Pickle);
        }
        base {
            fn messenger_onActionByClient_chat2(action: i16, request: u16, args: MessengerArgs);
            fn doCmdStr(request_id: i16, cmd: i16, arg: String);
            fn doCmdInt(request_id: i16, cmd: i16, arg: i64);
            fn doCmdInt2(request_id: i16, cmd: i16, arg0: i64, arg1: i64);
            fn doCmdInt3(request_id: i16, cmd: i16, arg0: i64, arg1: i64, arg2: i64);
            fn doCmdInt4(request_id: i16, cmd: i16, arg0: i64, arg1: i64, arg2: i64, arg3: i64);
            fn doCmdIntArr(request_id: i16, cmd: i16, args: Vec<i32>);
            fn accountPrebattle_sendPrebattleInvites(recipients: Vec<i64>, comment: Bytes);
            fn accountPrebattle_createTraining(arena_type_id: i32, round_length: i32, is_opened: u8, comment: Bytes);
            fn doCmdIntArrStrArr(request_id: i16, cmd: i16, ints: Vec<i64>, strings: Vec<Bytes>);
            fn requestToken(request_id: u16, token_type: u8);
            fn chatCommandFromClient(
                request_id: i64,
                action: u8,
                channel: EntityId,
                int_arg: i64,
                short_arg: i16,
                str_arg: String,
                str_arg2: String,
            );
        }
    }
}

script_dict! {
    /// How many players are online, on the player's cluster and the whole region.
    #[derive(Debug, Clone)]
    pub struct ServerStats {
        pub clusterCCU: u32,
        pub regionCCU: u32,
    }
}

pub(super) struct AccountBootstrap {
    version_property: Arc<str>,
    required_version: String,
    settings_type: Ty,
    pub(super) static_components_count: u8,
}

impl AccountBootstrap {
    pub(super) fn bind(dispatch: &ScriptDispatch) -> Result<Self> {
        let interface = dispatch
            .script
            .interfaces
            .iter()
            .find(|interface| &*interface.name == "AccountVersion")
            .context("Missing AccountVersion interface")?;
        let mut versions = interface
            .properties
            .iter()
            .filter(|property| property.name.starts_with("requiredVersion_"));
        let property = versions
            .next()
            .context("Unsupported AccountVersion property")?;
        ensure!(
            versions.next().is_none(),
            "Ambiguous AccountVersion properties"
        );
        ensure!(
            property.flags == PropertyFlags::BaseAndClient && String::check(&property.ty),
            "AccountVersion property must be a BASE_AND_CLIENT STRING"
        );
        let required_version = property
            .default
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .context("Missing AccountVersion property default")?
            .clone();
        let (_, account) = dispatch
            .entity_from_name("Account")
            .context("Missing Account entity")?;
        ensure!(
            dict_check(
                &account.data_ty,
                &[
                    ("name", String::check),
                    ("incarnationID", u64::check),
                    (&property.name, String::check),
                    ("initialServerSettings", Pickle::check),
                ],
            ),
            "Account creation data does not match the server bootstrap schema"
        );
        let settings_type = dict_field_ty(&account.data_ty, "initialServerSettings")
            .context("Missing Account initialServerSettings type")?
            .clone();
        Ok(Self {
            version_property: property.name.clone(),
            required_version,
            settings_type,
            static_components_count: account.static_components_count,
        })
    }

    pub(super) fn version_property(&self) -> &str {
        &self.version_property
    }

    pub(super) fn encode(&self, name: &str, settings: Pickle) -> Value {
        Value::Dict(
            [
                (
                    "name".into(),
                    Value::String(StringValue::String(name.to_owned())),
                ),
                ("incarnationID".into(), Value::UInt64(1000)),
                (
                    self.version_property.clone(),
                    Value::String(StringValue::String(self.required_version.clone())),
                ),
                (
                    "initialServerSettings".into(),
                    settings.into_value(&self.settings_type),
                ),
            ]
            .into(),
        )
    }
}

script_dict! {
    #[derive(Debug, Clone)]
    pub struct ChatActionData {
        pub requestID: i64,
        pub action: u8,
        pub actionResponse: u8,
        pub time: f64,
        pub sentTime: f64,
        pub channel: EntityId,
        pub originator: i64,
        pub originatorNickName: String,
        pub group: u8,
        pub data: Pickle,
        pub flags: u8,
    }
}
script_dict! {
    #[derive(Debug, Clone, Default)]
    pub struct MessengerArgs {
        pub int32Arg1: i32,
        pub int64Arg1: i64,
        pub floatArg1: f64,
        pub strArg1: Bytes,
        pub strArg2: Bytes,
        pub int8Arg1: i8,
    }
}
