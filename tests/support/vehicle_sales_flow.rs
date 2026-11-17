use super::{
    Peer, Pickle, Value,
    ammunition_flow::update,
    depot_flow::{row, send_array},
    dict,
};
use wot_server::{
    accounts::{
        AccountRepository, GarageCatalog, PostgresAccounts, ProgressionCommand, ProgressionError,
        VehiclePurchaseOptions, VehicleSaleRequest,
    },
    client_gateway::mercury::app::dispatch::ScriptDispatch,
    pickle,
};

const AIMING: i32 = (101 << 8) | 0xf9;
const RAMMER: i32 = (81 << 8) | 0xf9;
const REPAIR_KIT: i32 = (4 << 8) | 0xfb;
const T34: i32 = 1;
const T34_SHELL: i32 = (2172 << 8) | 10;

pub fn exercise(peer: &mut Peer, dispatch: &ScriptDispatch, before: &Pickle) -> Pickle {
    let revision = integer(dict(before, "rev"));
    let inventory = dict(before, "inventory");
    let vehicles = row(&inventory, 1);
    let Pickle::List(shells) = row(&dict(&vehicles, "shells"), 1) else {
        panic!("Missing T-34 shells");
    };
    let shell = integer(shells[0].clone()) as i32;
    assert_eq!(shell, T34_SHELL);
    let depot_count = integer(row(&row(&inventory, 10), shell));
    let Pickle::List(crew) = row(&dict(&vehicles, "crew"), 1) else {
        panic!("Missing T-34 crew");
    };
    for (id, args, code) in [
        (750, vec![1, 1, 0, i32::MAX], -2),
        (751, vec![1, 1, 0, -1, 0, 0, 0, 0], -2),
        (752, vec![1, 1, 0, 0, 0, 0, 0, 0, 0], -2),
        (753, vec![0, 1, 0, 0, 0, 0, 0, 0], -4),
        (754, vec![1, 99, 0, 0, 0, 0, 0, 0], -1),
        (755, vec![1, 1, 0, 2, AIMING, AIMING, 0, 0, 0, 0], -1),
        (756, vec![1, 1, 0, 1, shell, 1, RAMMER, 0, 0, 0], -1),
    ] {
        send_array(peer, dispatch, id, 304, &args, code);
    }
    send_array(
        peer,
        dispatch,
        757,
        304,
        &[1, 1, 0, 1, REPAIR_KIT, 1, shell, 0, 0, 0],
        0,
    );
    let sold = update(peer, revision + 1);
    assert_eq!(dict(&sold, "prevRev"), revision.into());
    let delta = dict(&sold, "inventory");
    let removed = row(&delta, 1);
    for table in [
        "compDescr",
        "crew",
        "shellsLayout",
        "devicesLayout",
        "eqsLayout",
        "settings",
    ] {
        assert_eq!(dict(&removed, table), pickle!({1: None}));
    }
    let assignments = dict(&row(&delta, 8), "vehicle");
    for member in crew.into_iter().filter(|member| *member != Pickle::None) {
        assert_eq!(row(&assignments, integer(member) as i32), Pickle::None);
    }
    assert_eq!(row(&delta, 10), pickle!({(shell): None}));
    assert_eq!(row(&delta, 9), pickle!({(AIMING): 1}));
    let stats = dict(&sold, "stats");
    let previous = dict(before, "stats");
    assert_eq!(
        dict(&stats, "gold"),
        (integer(dict(&previous, "gold")) - 10).into()
    );
    assert_eq!(
        dict(&stats, "credits"),
        (integer(dict(&previous, "credits")) + 178_350 + 1_500 + 28 * depot_count).into()
    );

    send_array(peer, dispatch, 758, 304, &[1, 1, 0, 0, 0, 0, 0, 0], -1);
    send_array(peer, dispatch, 759, 304, &[1, 4, 1, 0, 0, 0, 0, 0], 0);
    let dismissed = update(peer, revision + 2);
    let Pickle::Dict(buffer) = dict(&dict(&dict(&dismissed, "recycleBin"), "tankmen"), "buffer")
    else {
        panic!("Missing 4TP dismissed crew");
    };
    assert!(!buffer.is_empty());
    assert_eq!(
        dict(&dict(&dismissed, "stats"), "credits"),
        dict(&stats, "credits")
    );
    let Pickle::List(shells) = row(&dict(&vehicles, "shells"), 5) else {
        panic!("Missing Renault FT shells");
    };
    let mut sale = vec![1, 5, 0, 0];
    for pair in shells.as_chunks::<2>().0 {
        if integer(pair[1].clone()) > 0 {
            sale.push(integer(pair[0].clone()) as i32);
        }
    }
    sale[3] = (sale.len() - 4) as i32;
    sale.extend([0, 0, 0, 0]);
    send_array(peer, dispatch, 760, 304, &sale, 0);
    let renault = update(peer, revision + 3);
    assert_eq!(
        dict(&dict(&renault, "stats"), "credits"),
        dict(&stats, "credits")
    );

    peer.command(
        dispatch,
        "doCmdInt3",
        vec![
            Value::Int16(761),
            Value::Int16(100),
            Value::Int64(0),
            Value::Int64(0),
            Value::Int64(0),
        ],
    );
    peer.until(dispatch, |p| p.has_resource(761));
    let full = peer.resource(761);
    assert_eq!(
        dict(&dict(&full, "stats"), "slots"),
        dict(&previous, "slots")
    );
    full
}

pub async fn verify_storage(repository: &PostgresAccounts, catalog: &GarageCatalog, account: i64) {
    let stored = repository.snapshot(account).await.unwrap();
    assert_eq!(
        stored
            .vehicles
            .iter()
            .map(|v| v.inventory_id)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(stored.progression.unlocks.contains(&T34));
    assert!(!stored.progression.installed.contains_key(&1));
    assert_eq!(stored.progression.equipment_layout(1), [0; 4]);
    assert_eq!(
        stored.progression.vehicle_consumables(1),
        Default::default()
    );
    assert_eq!(stored.progression.directive_slot(1), Default::default());
    let roster = stored.crew.as_ref().unwrap();
    assert!(!roster.accelerated_training.contains(&1));
    assert!(
        roster
            .members
            .values()
            .any(|member| member.dismissed_at.is_some())
    );
    let rebuy = catalog
        .plan_progression(
            stored.clone(),
            ProgressionCommand::BuyVehicle {
                vehicle: T34,
                options: VehiclePurchaseOptions {
                    crew: None,
                    ammunition: false,
                },
                consumables: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(rebuy.acquired_vehicle, Some(6));

    let sale = |dismiss_crew| {
        ProgressionCommand::SellVehicle(VehicleSaleRequest {
            inventory_id: 2,
            dismiss_crew,
            mounted_items: vec![],
            depot_items: vec![],
        })
    };
    let mut full = stored.clone();
    full.balances.berths = 0;
    assert!(matches!(
        catalog.plan_progression(full, sale(false)),
        Err(ProgressionError::NoFreeBerth)
    ));
    let proposal = catalog.plan_progression(stored, sale(true)).unwrap();
    let (first, repeated) = tokio::join!(
        repository.save_progression(&proposal),
        repository.save_progression(&proposal),
    );
    assert_ne!(first.unwrap(), repeated.unwrap());
    let committed = repository.snapshot(account).await.unwrap();
    assert_eq!(committed.vehicles, proposal.snapshot.vehicles);
    assert_eq!(committed.balances, proposal.snapshot.balances);
    assert_eq!(committed.crew, proposal.snapshot.crew);
    assert_eq!(committed.depot_shells, proposal.snapshot.depot_shells);
}

fn integer(value: Pickle) -> i64 {
    let Pickle::I64(value) = value else {
        panic!("Missing integer");
    };
    value
}
