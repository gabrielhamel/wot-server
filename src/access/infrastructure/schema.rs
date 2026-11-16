use crate::access::mercury::app::entity::entity_schema;
use crate::access::mercury::app::entity::{Bytes, EntityId, Pickle};
use crate::script_dict;
entity_schema! {
    pub Account {
        client {
            fn onChatAction(data: ChatActionData);
            fn messenger_onActionByServer_chat2(action: i16, request: u16, args: MessengerArgs);
            fn showGUI(ctx: Pickle);
            fn onArenaCreated();
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
            fn doCmdIntArrStrArr(request_id: i16, cmd: i16, ints: Vec<i64>, strings: Vec<String>);
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

script_dict! {
    /// The `Account`'s base data, which its client gets as it is created: its player,
    /// the client version it requires and the server's settings.
    #[derive(Debug, Clone)]
    pub struct AccountData {
        pub name: String,
        /// How many sessions the player had, the client telling new players by it.
        pub incarnationID: u64,
        /// The client version the server requires, refusing others.
        pub requiredVersion_2410: String,
        /// A dict of the server's settings, of every feature the client reads at login.
        pub initialServerSettings: Pickle,
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
