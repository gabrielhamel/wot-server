//! The shop: prices, exchange rates and costs of the game, sent in answer to
//! `CMD_SYNC_SHOP` as a zlib-compressed pickled dict that the client keeps in its
//! persistent cache, and read through `Shop.py` and the GUI's `ShopRequester`.
//!
//! The constants are the EU server's, the items' prices those of the game's definitions.

use std::collections::BTreeSet;
use std::io;

use crate::{
    accounts::{
        BERTH_PACK_GOLD, BERTH_PACK_SIZE, CREW_FREE_RESTORE_DURATION, CREW_RESTORE_CREDITS,
        CREW_RESTORE_DURATION, CREW_RESTORE_LIMIT, EquipmentPrice, EquipmentRules, FREE_XP_RATE,
    },
    platform::client_formats::pickle::{Global, Value},
};

use super::item_defs::NATIONS;
use super::item_prices::ItemPrices;

use super::account;

pub(super) const SHOP_REVISION: i32 = 1;
pub(super) const EQUIPMENT_DEMOUNT_GOLD: i64 = 10;
pub(super) const DELUXE_DEMOUNT_BONDS: i64 = 200;
pub(super) const MODERNIZED_2_DEMOUNT_COMPONENTS: i64 = 50;
pub(super) const MODERNIZED_3_DEMOUNT_COMPONENTS: i64 = 100;
pub(super) const AMMO_EXCHANGE_RATE: i64 = 400;
pub(super) const GOLD_EXCHANGE_RATE: i64 = 400;
pub(super) const CRYSTAL_EXCHANGE_RATE: i64 = 200;
pub(super) const SELL_PRICE_DIVISOR: i64 = 2;
pub(super) const FREE_VEHICLE_SLOTS: i64 = 12;
pub(super) const VEHICLE_SLOT_CREDITS: i64 = 250_000;

/// The shop, as streamed.
#[derive(Debug)]
pub struct Shop {
    pub data: Vec<u8>,
}

impl Shop {
    pub fn new(prices: &ItemPrices, equipment: &EquipmentRules) -> io::Result<Self> {
        Ok(Self {
            data: account::compress(&shop(prices, equipment))?,
        })
    }
}

fn shop(prices: &ItemPrices, equipment: &EquipmentRules) -> Value {
    let per_nation = |value: Value| Value::List(vec![value; NATIONS.len()]);
    let empty_set = || Value::Set(BTreeSet::new());

    let item_prices = Value::Dict(
        prices
            .prices
            .iter()
            .map(|(cd, price)| {
                (
                    Value::from(cd.to_int()),
                    Value::dict([(price.currency, Value::from(price.amount))]),
                )
            })
            .chain(
                prices
                    .catalog_only
                    .iter()
                    .map(|cd| (Value::from(cd.to_int()), Value::Dict(Default::default()))),
            )
            .collect(),
    );
    let item_prices = Value::object(
        Global::new("items", "ItemsPrices"),
        Value::dict([("_itemsPriceInfo", item_prices)]),
    );
    let not_in_shop = Value::Set(
        prices
            .not_in_shop
            .iter()
            .map(|cd| Value::from(cd.to_int()))
            .collect(),
    );

    let premium_prices = Value::Dict(
        crate::accounts::PREMIUM_PACKAGES
            .into_iter()
            .map(|(days, gold)| (Value::from(days), Value::from(gold)))
            .collect(),
    );

    let upgrade_prices = Value::Dict(
        equipment
            .items
            .iter()
            .filter_map(|(&item, device)| {
                let upgrade = device.upgrade?;
                let price = match upgrade.price {
                    EquipmentPrice::Credits(amount) => {
                        Value::dict([("credits", Value::from(amount))])
                    }
                    EquipmentPrice::Components(amount) => {
                        Value::dict([("equipCoin", Value::from(amount))])
                    }
                    _ => return None,
                };
                Some((
                    Value::Tuple(vec![Value::from(item), Value::from(upgrade.target)]),
                    price,
                ))
            })
            .collect(),
    );
    let operation_prices = crate::pickle!({"upgrade": (upgrade_prices)});

    crate::pickle!({
        "rev": SHOP_REVISION,
        "battlePassLevelCost": {"gold": 250},
        "berthsPrices": (BERTH_PACK_SIZE, BERTH_PACK_SIZE, [BERTH_PACK_GOLD]),
        "camouflageCost": {0: (250, 1), 7: (25000, 0), 30: (100000, 0)},
        "changeRoleCost": 0,
        "clanCreationCost": 2500,
        "crystalExchangeRate": CRYSTAL_EXCHANGE_RATE,
        "customRoleSlotChangeCost": {
            6: {"credits": 10000}, 7: {"credits": 25000}, 8: {"credits": 50000},
            9: {"credits": 100000}, 10: {"credits": 150000}, 11: {"credits": 200000},
        },
        "dailyXPFactor": 2,
        "defaults": {"exchangeRate": GOLD_EXCHANGE_RATE, "goodies": {"prices": {}}, "items": {"operationPrices": (operation_prices.clone())}, "premiumCost": (premium_prices.clone())},
        "dropSkillsCost": {
            0: {"credits": 0, "gold": 0, "recertificationForm": 0, "xpReuseFraction": 0.8},
            1: {"credits": 20000, "gold": 0, "recertificationForm": 0, "xpReuseFraction": 0.9},
            2: {"credits": 0, "gold": 200, "recertificationForm": 0, "xpReuseFraction": 1.0},
            3: {"credits": 0, "gold": 0, "recertificationForm": 1, "xpReuseFraction": 1.0},
            4: {"credits": 100000, "gold": 0, "recertificationForm": 0, "xpReuseFraction": 1.0},
        },
        "exchangeRate": GOLD_EXCHANGE_RATE,
        "exchangeRateForShellsAndEqs": AMMO_EXCHANGE_RATE,
        "freeXPConversion": (25, 1),
        "freeXPToTManXPRate": (FREE_XP_RATE),
        "goodies": {"goodies": {}, "notInShop": (empty_set()), "prices": {}},
        "isEnabledBuyingGoldEqsForCredits": true,
        "isEnabledBuyingGoldShellsForCredits": true,
        "items": {
            "camouflagePriceFactors": (per_nation(crate::pickle!({}))),
            "inscriptionGroupPriceFactors": (per_nation(crate::pickle!({}))),
            "itemPrices": item_prices,
            "notInShopCamouflages": (per_nation(empty_set())),
            "notInShopInscriptionGroups": (per_nation(empty_set())),
            "notInShopItems": not_in_shop,
            "notInShopPlayerEmblemGroups": (empty_set()),
            "notInShopProgressionLvlItems": {},
            "operationPrices": (operation_prices),
            "playerEmblemGroupPriceFactors": {},
            "progressionLvlPrices": {},
            "vehicleCamouflagePriceFactors": {},
            "vehicleSellPriceFactors": {},
            "vehiclesNotToBuy": (empty_set()),
            "vehiclesRentPrices": {},
            "vehiclesToSellForGold": (empty_set()),
        },
        "paidDeluxeRemovalCost": {"crystal": DELUXE_DEMOUNT_BONDS},
        "paidModernized1RemovalCost": {"gold": EQUIPMENT_DEMOUNT_GOLD},
        "paidModernized2RemovalCost": {"equipCoin": MODERNIZED_2_DEMOUNT_COMPONENTS},
        "paidModernized3RemovalCost": {"equipCoin": MODERNIZED_3_DEMOUNT_COMPONENTS},
        "paidRemovalCost": {"gold": EQUIPMENT_DEMOUNT_GOLD},
        "paidTrophyBasicRemovalCost": {"gold": EQUIPMENT_DEMOUNT_GOLD},
        "paidTrophyUpgradedRemovalCost": {"gold": EQUIPMENT_DEMOUNT_GOLD},
        "playerEmblemCost": {0: (15, 1), 7: (1500, 0), 30: (6000, 0)},
        "playerInscriptionCost": {"nations": {}, 0: (15, 1), 7: (1500, 0), 30: (6000, 0)},
        "premiumCost": (premium_prices),
        "restore_config": {
            "tankmen": {
                "creditsCost": (CREW_RESTORE_CREDITS),
                "creditsDuration": (CREW_RESTORE_DURATION as i64),
                "freeDuration": (CREW_FREE_RESTORE_DURATION as i64),
                "goldCost": 20, "goldDuration": (CREW_RESTORE_DURATION as i64), "limit": (CREW_RESTORE_LIMIT as i64),
            },
            "vehicles": {"actionCooldown": 259200, "premiumDuration": 259200, "sellToRestoreFactor": 1.1},
        },
        "sellPriceFactor": (1.0 / SELL_PRICE_DIVISOR as f64),
        "slotsPrices": (FREE_VEHICLE_SLOTS, [("credits", VEHICLE_SLOT_CREDITS)]),
        "specialPrices": {},
        "spgRedesignFeatures": {"markTargetAreaEnabled": true, "stunEnabled": true},
        "tankman": {
            "recruit": {"options": ({"credits": 0, "gold": 0, "xp": 0},)},
            "retrain": {"options": (
                {"discounts": {}, "skillsEfficiency": 0.3, "skillsEfficiencyWithRoleChange": (-1.0)},
                {"discounts": {"perk_0": 0.0, "perk_1": 0.5}, "skillsEfficiency": 0.6, "skillsEfficiencyWithRoleChange": 0.4},
                {"discounts": {"perk_0": 0.0, "perk_1": 0.5}, "skillsEfficiency": 1.0, "skillsEfficiencyWithRoleChange": 1.0},
            )},
        },
        "tankmanCost": (
            {"baseRoleLoss": 0.2, "classChangeRoleLoss": 0.2, "credits": 0, "gold": 0, "isPremium": false, "roleLevel": 50},
            {"baseRoleLoss": 0.1, "classChangeRoleLoss": 0.1, "credits": 20000, "gold": 0, "isPremium": false, "roleLevel": 75},
            {"baseRoleLoss": 0.0, "classChangeRoleLoss": 0.0, "credits": 0, "gold": 200, "isPremium": true, "roleLevel": 100},
        ),
        "winXPFactorMode": 0,
    })
}
