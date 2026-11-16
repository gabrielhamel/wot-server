use wot_server::accounts::*;

fn snapshot() -> AccountSnapshot {
    AccountSnapshot {
        crew: None,
        id: 1,
        display_name: "Player".into(),
        revision: 1,
        balances: Balances {
            credits: 10_000,
            gold: 1_000,
            slots: 2,
            ..Default::default()
        },
        vehicles: vec![OwnedVehicle {
            inventory_id: 42,
            catalog_key: "test:starter".into(),
            ammunition: None,
            has_crew: true,
        }],
        depot_shells: Default::default(),
        progression: Default::default(),
        settings: Default::default(),
    }
}
fn rules() -> VehiclePurchaseRules {
    VehiclePurchaseRules {
        vehicle: 2065,
        catalog_key: "test:next".into(),
        price: Some(VehicleCost {
            credits: 3_000,
            gold: 0,
        }),
        requires_research: true,
        crew_size: 3,
        ammunition: vec![(100, 20), (200, 0)],
        ammunition_cost: Some(VehicleCost {
            credits: 60,
            gold: 0,
        }),
    }
}
fn options() -> VehiclePurchaseOptions {
    VehiclePurchaseOptions {
        crew: Some(CrewRecruitment::Free),
        ammunition: true,
    }
}

#[test]
fn purchase_adds_stable_ownership_and_charges_requested_options() {
    let mut s = snapshot();
    let id = rules().buy(&mut s, options(), true).unwrap();
    assert_eq!(id, 43);
    assert_eq!(s.balances.credits, 6_940);
    assert_eq!(s.balances.slots, 2);
    assert_eq!(s.vehicles[0].inventory_id, 42);
    let vehicle = &s.vehicles[1];
    assert!(vehicle.has_crew);
    assert_eq!(
        vehicle.ammunition.as_ref().unwrap().loaded,
        vec![(100, 20), (200, 0)]
    );
    assert_eq!(
        rules().buy(&mut s, options(), true).err(),
        Some(ProgressionError::VehicleAlreadyOwned)
    );
    assert_eq!(s.balances.credits, 6_940);
}

#[test]
fn buying_without_crew_or_shells_is_persistable_and_does_not_grant_ammunition() {
    let mut s = snapshot();
    rules()
        .buy(
            &mut s,
            VehiclePurchaseOptions {
                crew: None,
                ammunition: false,
            },
            true,
        )
        .unwrap();
    assert_eq!(s.balances.credits, 7_000);
    assert!(!s.vehicles[1].has_crew);
    assert_eq!(
        s.vehicles[1].ammunition.as_ref().unwrap().loaded,
        vec![(100, 0), (200, 0)]
    );
}

#[test]
fn crew_and_premium_prices_use_the_correct_wallet_currency() {
    let mut s = snapshot();
    s.balances.credits = 100_000;
    rules()
        .buy(
            &mut s,
            VehiclePurchaseOptions {
                crew: Some(CrewRecruitment::Credits),
                ammunition: false,
            },
            true,
        )
        .unwrap();
    assert_eq!(s.balances.credits, 37_000);
    assert_eq!(s.balances.gold, 1_000);
    let mut s = snapshot();
    let mut premium = rules();
    premium.requires_research = false;
    premium.price = Some(VehicleCost {
        credits: 0,
        gold: 100,
    });
    premium
        .buy(
            &mut s,
            VehiclePurchaseOptions {
                crew: Some(CrewRecruitment::Gold),
                ammunition: false,
            },
            false,
        )
        .unwrap();
    assert_eq!(s.balances.gold, 300);
    assert_eq!(s.balances.credits, 10_000);
}

#[test]
fn unavailable_research_slots_and_money_never_change_ownership_or_balances() {
    for (researched, slots, credits, expected) in [
        (false, 2, 10_000, ProgressionError::InvalidRequest),
        (true, 1, 10_000, ProgressionError::NoFreeSlot),
        (true, 0, 10_000, ProgressionError::NoFreeSlot),
        (true, 2, 3_000, ProgressionError::NotEnoughMoney),
    ] {
        let mut s = snapshot();
        s.balances.slots = slots;
        s.balances.credits = credits;
        let before = s.balances.clone();
        assert_eq!(
            rules().buy(&mut s, options(), researched).err(),
            Some(expected)
        );
        assert_eq!(s.vehicles.len(), 1);
        assert_eq!(s.balances, before);
    }
    let mut s = snapshot();
    let mut r = rules();
    r.price = None;
    assert_eq!(
        r.buy(&mut s, options(), true).err(),
        Some(ProgressionError::NotForSale)
    );
    assert_eq!(s.vehicles.len(), 1);
}

#[test]
fn inventory_and_cost_overflow_are_rejected_before_charging() {
    let mut s = snapshot();
    s.vehicles[0].inventory_id = i64::MAX;
    assert!(rules().buy(&mut s, options(), true).is_err());
    assert_eq!(s.balances.credits, 10_000);
    let mut s = snapshot();
    let mut r = rules();
    r.price = Some(VehicleCost {
        credits: i64::MAX,
        gold: 0,
    });
    assert!(r.buy(&mut s, options(), true).is_err());
    assert_eq!(s.balances.credits, 10_000);
    assert_eq!(s.vehicles.len(), 1);
}

#[test]
fn unavailable_shell_bundle_does_not_prevent_buying_an_empty_vehicle() {
    let mut s = snapshot();
    let mut r = rules();
    r.ammunition_cost = None;
    assert_eq!(
        r.buy(&mut s, options(), true).err(),
        Some(ProgressionError::NotForSale)
    );
    assert_eq!(s.vehicles.len(), 1);
    r.buy(
        &mut s,
        VehiclePurchaseOptions {
            crew: None,
            ammunition: false,
        },
        true,
    )
    .unwrap();
    assert_eq!(s.balances.credits, 7_000);
}
