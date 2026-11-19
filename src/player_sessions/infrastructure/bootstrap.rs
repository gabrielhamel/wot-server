use crate::platform::client_formats::pickle::Value as PickleValue;
use crate::player_sessions::mercury::app::entity::Pickle;
use std::time::{SystemTime, UNIX_EPOCH};
fn pickle_dict(entries: impl IntoIterator<Item = (&'static str, PickleValue)>) -> PickleValue {
    PickleValue::Dict(
        entries
            .into_iter()
            .map(|(key, value)| (PickleValue::Bytes(key.as_bytes().to_vec()), value))
            .collect(),
    )
}

fn server_settings(store_url: &str, training_room_visible: bool, bounty_upgrades: bool) -> Pickle {
    let shop = if store_url.is_empty() {
        crate::pickle!({"isStorageEnabled": true})
    } else {
        crate::pickle!({"isStorageEnabled": true, "hostUrl": (store_url)})
    };
    Pickle(crate::pickle!({
        "file_server": {},
        "shop": (shop),
        "isTrainingBattleEnabled": (training_room_visible),
        "isTrophyDevicesEnabled": (bounty_upgrades),
        "isCrewBooksPurchaseEnabled": true,
        "isCrewBooksSaleEnabled": true,
        "isJunkCrewConversionEnabled": true,
        "rewardBookId": 62,
        "XppToConvert": 100000,
        "ranked_config": {"isEnabled": false},
        "fun_random_config": {"isEnabled": false},
        "comp7_config": {"isEnabled": false},
        "comp7_ranks_config": {},
        "comp7_light_config": {"isEnabled": false},
        "spgRedesignFeatures": {"markTargetAreaEnabled": true, "stunEnabled": true},
        "battle_royale_config": {"isEnabled": false},
        "epic_config": {"isEnabled": false, "epicMetaGame": {}},
        "vehicle_post_progression_config": {},
        "regional_settings": {
            "starting_day_of_a_new_week": 0,
            "starting_time_of_a_new_day": (4 * 3600),
            "starting_time_of_a_new_game_day": (4 * 3600),
        },
        "armor_flashlight_config": {"enabled": true},
        "armor_inspector_config": {"enabled": true},
        "commendations_config": {"isCommendationsEnabled": true, "isLiveTagsEnabled": true},
        "fort_rush_battles_config": {
            "isEnabled": false,
            "technicalPause": false,
            "isInfoPageEnabled": false,
            "startDatetime": "2024-06-06T02:00:00+00:00",
            "endDatetime": "2028-07-03T22:00:00+00:00",
            "eligibleVehicleTiers": "10",
            "baseProgressionToken": "frevent:progression_token",
            "completedProgressionToken": "frevent:progression_completed_token",
            "progressionQuestPrefix": "frevent:progression:",
            "maxRewardsAmountPerStage": 3,
            "progression": {"stage": []},
            "battleEconomy": {
                "creditsByPlace": {
                    "win": "25000 20000 15000 10000 10000 10000 5000",
                    "draw": "20000 15000 10000 5000 5000 5000 1000",
                    "loss": "20000 15000 10000 5000 5000 5000 1000",
                },
                "progressionPointsByPlace": {"win": "6 6 6 4 4 4 2", "draw": "4 4 4 2 2 2 0", "loss": "4 4 4 2 2 2 0"},
            },
            "dailyMissionsConfig": {
                "dailyGroupsCount": 5,
                "dailyMissionQuestPrefix": "fort_rush_daily_mission:",
                "dailyMissionTokenPostfix": "{group}:{num}",
                "dailyMissionTokenPrefix": "fort_rush_daily_token:",
                "dailyMissionTokenTtl": 24,
                "dailyQuestMaxDays": 12,
                "hardDailyMissionQuestPrefix": "fort_rush_hard_daily_mission:",
                "tokenLimit": 1,
            },
        },
        "halloween_anomalies_config": {"anomalies": {"anomaly": []}, "recipes": {"recipe": []}},
        "halloween_bestiary_config": {"enemies": {"enemy": []}},
        "halloween_gsw_config": {"cards": {"card": []}},
        "halloween_loot_config": {"loot": []},
        "newbie_battle_hints_config": {"enabled": false},
        "newbie_prebattle_hints_config": {"enabled": false, "hintDisplayCount": 2},
        "oit_availability_config": {},
        "player_satisfaction_config": {"enabled": true, "enabledInterfaces": {}},
        "prebattle_hints_config": {"enabled": true, "battleTimerThreshold": 5},
        "prefab_effects_availability_config": {},
        "story_mode_battle_mgr_quotums": {"quotums": []},
        "story_mode_missions": {"missions": [], "onboarding": {}},
        "story_mode_settings": {
            "enabled": false,
            "waitTimeQueue": 20,
            "hideGameLoadingTimeout": 20,
            "modeSelectorCardColumn": 3,
            "modeSelectorCardPriority": 50,
            "entryPoint": {"eventStartAt": "2024-06-06T02:00:00+00:00", "eventEndAt": "2024-07-03T22:00:00+00:00"},
            "newbieBannerEnabled": false,
            "newbieAdvertisingEnabled": false,
            "parallaxEnabled": true,
            "eventName": "story_mode_vday",
        },
        "umgConfig": {"enableAllDaily": 1, "enableAllWeekly": 1, "enableDailyWeeklyUI": 1, "enablePM3Banner": 1, "slides": []},
        "umgEvents": {"weights": []},
        "umgMissions": {"weights": []},
        "veh_playlists_config": {"isVehPlaylistsEnabled": true},
        "weekly_quests_config": {"enabled": true, "rerollTimeout": 14400},
    }))
}

pub fn show_gui_ctx(database_id: i64) -> Pickle {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Pickle(pickle_dict([
        ("aogasStartedAt", PickleValue::F64(now.as_secs_f64())),
        ("collectUiStats", PickleValue::Bool(false)),
        ("databaseID", PickleValue::I64(database_id)),
        ("isAogasEnabled", PickleValue::Bool(false)),
        ("logUXEvents", PickleValue::Bool(false)),
        ("serverUTC", PickleValue::F64(now.as_secs_f64())),
        ("sessionStartedAt", PickleValue::I64(now.as_secs() as i64)),
    ]))
}

pub fn token_data(token_type: u8, database_id: i64) -> Pickle {
    let token = match token_type {
        1 => PickleValue::I64(0),
        _ => PickleValue::Bytes(Vec::new()),
    };
    Pickle(pickle_dict([
        ("databaseID", PickleValue::I64(database_id)),
        ("token", token),
    ]))
}

pub fn server_settings_with_chat(
    address: std::net::SocketAddrV4,
    store: Option<(&str, &str)>,
    training_room_visible: bool,
    bounty_upgrades: bool,
) -> Pickle {
    let mut settings = server_settings(
        store.map_or("", |(url, _)| url),
        training_room_visible,
        bounty_upgrades,
    );
    if let PickleValue::Dict(ref mut dict) = settings.0 {
        let values = crate::pickle!({"xmpp_enabled": true, "xmpp_host": (super::xmpp::DOMAIN), "xmpp_port": (i32::from(address.port())), "xmpp_resource": (super::xmpp::RESOURCE), "xmpp_connections": [((address.ip().to_string().as_str()), (i32::from(address.port())))], "xmpp_muc_services": []});
        if let PickleValue::Dict(values) = values {
            dict.extend(values);
        }
        if let Some((_, host)) = store {
            dict.insert(
                PickleValue::from("igbWhitelist"),
                crate::pickle!({
                    "isEnabled": true,
                    "defaultWhitelist": {(host)},
                    "whitelistUrl": "",
                }),
            );
        }
    }
    settings
}
