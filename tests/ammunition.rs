use std::collections::BTreeMap;
use wot_accounts::test_support::{
    AccountSnapshot, AmmoError, AmmunitionChange, AmmunitionRules, Balances, OwnedVehicle,
    ShellCurrency, ShellPrice, ShellSelection,
};

fn snapshot() -> AccountSnapshot {
    AccountSnapshot {
        crew: None,
        id: 1,
        display_name: "Player".into(),
        revision: 1,
        balances: Balances {
            credits: 100,
            gold: 10,
            ..Balances::default()
        },
        vehicles: vec![OwnedVehicle {
            inventory_id: 42,
            catalog_key: "test:tank".into(),
            ammunition: None,
            has_crew: true,
        }],
        depot_shells: BTreeMap::new(),
        progression: Default::default(),
        settings: BTreeMap::new(),
    }
}

fn rules() -> AmmunitionRules {
    AmmunitionRules {
        capacity: 10,
        defaults: vec![(10, 10), (20, 0), (30, 0)],
        prices: BTreeMap::from([
            (
                10,
                ShellPrice {
                    currency: ShellCurrency::Credits,
                    amount: 5,
                },
            ),
            (
                20,
                ShellPrice {
                    currency: ShellCurrency::Gold,
                    amount: 2,
                },
            ),
        ]),
        gold_to_credits: 400,
    }
}

fn selection(id: i32, count: u32) -> ShellSelection {
    ShellSelection {
        id,
        count,
        alternate_price: false,
    }
}

#[test]
fn unloading_and_reloading_preserve_shells_and_money() {
    let initial = snapshot();
    let unloaded =
        AmmunitionChange::plan(initial.clone(), 42, &rules(), &[selection(10, 4)]).unwrap();
    assert_eq!(unloaded.snapshot.depot_shells, BTreeMap::from([(10, 6)]));
    assert!(unloaded.purchases.is_empty());
    assert_eq!(unloaded.snapshot.revision, 2);
    let reloaded =
        AmmunitionChange::plan(unloaded.snapshot, 42, &rules(), &[selection(10, 10)]).unwrap();
    assert!(reloaded.snapshot.depot_shells.is_empty());
    assert!(reloaded.purchases.is_empty());
    assert_eq!(reloaded.snapshot.balances, initial.balances);
    assert_eq!(reloaded.ammunition().unwrap().loaded, rules().defaults);
    let unchanged =
        AmmunitionChange::plan(reloaded.snapshot, 42, &rules(), &[selection(10, 10)]).unwrap();
    assert!(!unchanged.changed());
    assert!(unchanged.purchases.is_empty());
}

#[test]
fn purchase_uses_depot_before_charging_and_records_total_cost() {
    let mut initial = snapshot();
    initial.depot_shells.insert(20, 2);
    let purchased = AmmunitionChange::plan(initial, 42, &rules(), &[selection(20, 3)]).unwrap();
    assert_eq!(purchased.snapshot.balances.gold, 8);
    assert_eq!(purchased.snapshot.balances.credits, 100);
    assert_eq!(purchased.snapshot.depot_shells, BTreeMap::from([(10, 10)]));
    assert_eq!(purchased.purchases.len(), 1);
    assert_eq!(purchased.purchases[0].count, 1);
    assert_eq!(purchased.purchases[0].cost.amount, 2);
}

#[test]
fn alternate_price_converts_gold_shells_to_credits() {
    let mut initial = snapshot();
    initial.balances.credits = 10_000;
    let requested = [ShellSelection {
        alternate_price: true,
        ..selection(20, 3)
    }];
    let purchased = AmmunitionChange::plan(initial, 42, &rules(), &requested).unwrap();
    assert_eq!(purchased.snapshot.balances.credits, 7600);
    assert_eq!(purchased.snapshot.balances.gold, 10);
    assert_eq!(purchased.purchases[0].cost.currency, ShellCurrency::Credits);
    assert_eq!(purchased.purchases[0].cost.amount, 2400);
}

#[test]
fn rejected_changes_leave_source_snapshot_intact() {
    let initial = snapshot();
    for (requested, expected) in [
        (vec![selection(20, 10)], AmmoError::NotEnoughMoney),
        (vec![selection(30, 1)], AmmoError::NotForSale),
        (vec![selection(10, 11)], AmmoError::InvalidLayout),
        (vec![selection(10, u32::MAX)], AmmoError::InvalidLayout),
        (vec![selection(999, 0)], AmmoError::InvalidLayout),
        (
            vec![selection(10, 1), selection(10, 1)],
            AmmoError::InvalidLayout,
        ),
    ] {
        assert_eq!(
            AmmunitionChange::plan(initial.clone(), 42, &rules(), &requested).err(),
            Some(expected),
        );
        assert_eq!(initial.revision, 1);
        assert_eq!(initial.balances.credits, 100);
        assert!(initial.depot_shells.is_empty());
        assert!(initial.vehicles[0].ammunition.is_none());
    }
    assert_eq!(
        AmmunitionChange::plan(initial, 1, &rules(), &[]).err(),
        Some(AmmoError::VehicleNotFound),
    );
}

#[test]
fn overflow_and_unavailable_prices_are_rejected() {
    let mut initial = snapshot();
    initial.depot_shells.insert(10, u32::MAX);
    assert_eq!(
        AmmunitionChange::plan(initial, 42, &rules(), &[]).err(),
        Some(AmmoError::InvalidLayout),
    );
    let mut rules = rules();
    rules.prices.get_mut(&20).unwrap().amount = i64::MAX;
    assert_eq!(
        AmmunitionChange::plan(snapshot(), 42, &rules, &[selection(20, 2)]).err(),
        Some(AmmoError::NotForSale),
    );
    assert_eq!(
        AmmunitionChange::plan(
            snapshot(),
            42,
            &rules,
            &[ShellSelection {
                alternate_price: true,
                ..selection(20, 1)
            }]
        )
        .err(),
        Some(AmmoError::NotForSale),
    );
    let mut initial = snapshot();
    initial.revision = i64::MAX;
    assert_eq!(
        AmmunitionChange::plan(initial, 42, &rules, &[selection(10, 9)]).err(),
        Some(AmmoError::InvalidLayout),
    );
}

#[test]
fn shell_order_is_preserved_and_omitted_types_are_unloaded() {
    let change = AmmunitionChange::plan(
        snapshot(),
        42,
        &rules(),
        &[selection(30, 0), selection(20, 0), selection(10, 10)],
    )
    .unwrap();
    assert_eq!(
        change.ammunition().unwrap().loaded,
        vec![(30, 0), (20, 0), (10, 10)]
    );
    assert!(change.purchases.is_empty());
}
