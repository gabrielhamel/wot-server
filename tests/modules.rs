use ModuleKind::{Chassis, Gun, Turret};
use std::collections::{BTreeMap, BTreeSet};
use wot_server::accounts::*;

fn snapshot() -> AccountSnapshot {
    AccountSnapshot {
        crew: None,
        id: 1,
        display_name: "Player".into(),
        revision: 1,
        balances: Balances {
            credits: 100,
            free_xp: 50,
            ..Default::default()
        },
        vehicles: vec![OwnedVehicle {
            inventory_id: 42,
            catalog_key: "test:tank".into(),
            ammunition: None,
            has_crew: true,
        }],
        depot_shells: Default::default(),
        progression: Default::default(),
        settings: Default::default(),
    }
}
fn rules() -> ModuleRules {
    ModuleRules {
        vehicle: 1,
        stock: BTreeMap::from([(Chassis, 20), (Turret, 30), (Gun, 40)]),
        modules: [
            (20, Chassis, 0),
            (21, Chassis, 10),
            (30, Turret, 0),
            (31, Turret, 20),
            (40, Gun, 0),
            (41, Gun, 30),
            (42, Gun, 50),
        ]
        .into_iter()
        .map(|(id, kind, price)| {
            (
                id,
                ModuleDefinition {
                    kind,
                    price: Some(price),
                    weight: 1.0,
                    max_weight: None,
                },
            )
        })
        .collect(),
        turret_guns: BTreeMap::from([
            (30, BTreeSet::from([40, 41])),
            (31, BTreeSet::from([41, 42])),
        ]),
        research: vec![
            ResearchStep {
                item: 41,
                xp: 35,
                prerequisites: vec![40],
            },
            ResearchStep {
                item: 42,
                xp: 10,
                prerequisites: vec![41, 31],
            },
        ],
        hull_weight: 10.0,
    }
}
fn defaults() -> BTreeSet<i32> {
    BTreeSet::from([1, 20, 30, 40])
}

#[test]
fn research_uses_vehicle_xp_then_free_xp_and_checks_prerequisites() {
    let rules = rules();
    let mut s = snapshot();
    assert!(rules.research(&mut s, 1, &defaults()).is_err());
    assert!(rules.research(&mut s, usize::MAX, &defaults()).is_err());
    s.progression.vehicle_xp.insert(1, 10);
    rules.research(&mut s, 0, &defaults()).unwrap();
    assert_eq!(s.balances.free_xp, 25);
    assert_eq!(s.progression.vehicle_xp[&1], 0);
    assert!(s.progression.unlocks.contains(&41));
    assert!(rules.research(&mut s, 0, &defaults()).is_err());
    s.progression.unlocks.insert(31);
    rules.research(&mut s, 1, &defaults()).unwrap();
    assert_eq!(s.balances.free_xp, 15);
}

#[test]
fn purchase_install_and_depot_swap_conserve_modules_and_money() {
    let rules = rules();
    let mut s = snapshot();
    s.progression.unlocks.insert(41);
    rules.equip(&mut s, 42, 41, 0, true, &defaults()).unwrap();
    assert_eq!(s.balances.credits, 70);
    assert_eq!(s.progression.depot, BTreeMap::from([(40, 1)]));
    rules.equip(&mut s, 42, 40, 0, false, &defaults()).unwrap();
    assert_eq!(s.balances.credits, 70);
    assert_eq!(s.progression.depot, BTreeMap::from([(41, 1)]));
    rules.equip(&mut s, 42, 41, 0, true, &defaults()).unwrap();
    assert_eq!(s.balances.credits, 70);
    assert_eq!(s.progression.depot, BTreeMap::from([(40, 1)]));
}

#[test]
fn turret_and_gun_change_is_atomic_and_checks_both_unlocks() {
    let rules = rules();
    let mut s = snapshot();
    s.progression.unlocks.insert(31);
    let before = s.progression.clone();
    assert!(rules.equip(&mut s, 42, 31, 41, true, &defaults()).is_err());
    assert_eq!(s.progression, before);
    assert_eq!(s.balances.credits, 100);
    s.progression.unlocks.insert(41);
    rules.equip(&mut s, 42, 31, 41, true, &defaults()).unwrap();
    assert_eq!(s.balances.credits, 50);
    assert_eq!(s.progression.depot, BTreeMap::from([(30, 1), (40, 1)]));
    assert_eq!(s.progression.installed[&42].get(&Gun), Some(&41));
}

#[test]
fn invalid_and_unaffordable_changes_leave_money_and_inventory_unchanged() {
    for (item, gun, buy) in [
        (i32::MIN, 0, true),
        (42, 0, true),
        (31, 0, true),
        (41, 42, true),
        (41, 0, false),
    ] {
        let mut s = snapshot();
        let before = s.progression.clone();
        assert!(
            rules()
                .equip(&mut s, 42, item, gun, buy, &defaults())
                .is_err()
        );
        assert_eq!(s.progression, before);
        assert_eq!(s.balances.credits, 100);
    }
    let mut s = snapshot();
    s.progression.unlocks.insert(41);
    s.balances.credits = 0;
    assert!(rules().equip(&mut s, 42, 41, 0, true, &defaults()).is_err());
    assert!(s.progression.installed.is_empty());
    let mut r = rules();
    r.modules.get_mut(&20).unwrap().max_weight = Some(12.0);
    assert!(r.equip(&mut s, 42, 41, 0, true, &defaults()).is_err());
    let mut s = snapshot();
    s.balances.free_xp = 0;
    assert!(r.research(&mut s, 0, &defaults()).is_err());
    assert!(s.progression.unlocks.is_empty());
    s.balances.free_xp = 50;
    r.research(&mut s, 0, &defaults()).unwrap();
}

#[test]
fn overflowing_depot_and_combined_prices_are_rejected_atomically() {
    let mut s = snapshot();
    s.progression.unlocks.extend([31, 41]);
    s.progression.depot.insert(40, u32::MAX);
    let before = s.progression.clone();
    assert!(rules().equip(&mut s, 42, 41, 0, true, &defaults()).is_err());
    assert_eq!(s.progression, before);
    assert_eq!(s.balances.credits, 100);
    let mut r = rules();
    r.modules.get_mut(&31).unwrap().price = Some(i64::MAX);
    s.progression.depot.clear();
    assert!(r.equip(&mut s, 42, 31, 41, true, &defaults()).is_err());
    assert!(s.progression.installed.is_empty());
    assert!(s.progression.depot.is_empty());
    assert_eq!(s.balances.credits, 100);
}
