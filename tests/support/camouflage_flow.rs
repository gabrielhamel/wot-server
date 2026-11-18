use super::{
    Peer, Pickle, Value,
    ammunition_flow::update,
    depot_flow::{row, send_array},
    dict, telemetry_capture,
};
use wot_server::{
    accounts::{Inscription, InscriptionSlot, ProjectionDecal, VehicleSurface, decode_outfits},
    pickle,
    player_sessions::mercury::{app::dispatch::ScriptDispatch, script::StringValue},
};

const SUMMER: i32 = 15_204;
const WINTER: i32 = 15_202;
const GERMAN: i32 = 15_116;
const USA_STARTER: i32 = 545;

pub struct CosmeticFixtures {
    pub has_exterior_styles: bool,
    pub has_extension_styles: bool,
    pub has_inscriptions: bool,
    pub has_effects: bool,
    pub has_decals: bool,
}

pub fn exercise(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
    fixtures: CosmeticFixtures,
) -> Pickle {
    let CosmeticFixtures {
        has_exterior_styles,
        has_extension_styles,
        has_inscriptions,
        has_effects,
        has_decals,
    } = fixtures;
    let Pickle::I64(mut revision) = dict(before, "rev") else {
        panic!("Missing garage revision");
    };
    let Pickle::I64(gold) = dict(&dict(before, "stats"), "gold") else {
        panic!("Missing gold");
    };
    let descriptor = |id| (id << 8) | 0x2c;
    let replacement = |id: i32| Pickle::Tuple(vec![id.into(), "_r".into()]);
    let summer = [4, 4, 1, 15, 228, 118, 2, 144, 34, 2]; // All three surfaces, scale/palette 2.
    let winter = [4, 4, 1, 5, 226, 118, 16]; // Hull with default scale and palette.
    let blank: &[u8] = &[];

    peer.command(dispatch, "doCmdInt3", sync(780, 300));
    peer.until(dispatch, |p| p.has_resource(780));
    let shop = peer.resource(780);
    let Pickle::Object(prices) = dict(&dict(&shop, "items"), "itemPrices") else {
        panic!("Missing shop prices");
    };
    assert_eq!(
        row(
            &dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo"),
            descriptor(SUMMER)
        ),
        pickle!({"gold": 50})
    );

    if has_exterior_styles {
        const GENERAL_2D_STYLE: i32 = (994 << 8) | 0x4c;
        let shop_items = dict(&shop, "items");
        assert_eq!(
            row(
                &dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo"),
                GENERAL_2D_STYLE
            ),
            pickle!({})
        );
        let Pickle::Set(hidden) = dict(&shop_items, "notInShopItems") else {
            panic!("Missing shop exclusions");
        };
        assert!(hidden.contains(&GENERAL_2D_STYLE.into()));
        if has_extension_styles {
            const LAST_STAND_STYLE: i32 = (6000 << 8) | 0x4c;
            let Pickle::Dict(price_rows) = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo")
            else {
                panic!("Missing price rows");
            };
            assert!(hidden.contains(&LAST_STAND_STYLE.into()));
            assert!(!price_rows.contains_key(&LAST_STAND_STYLE.into()));
            send_outfits(peer, dispatch, 793, 43, &[(15, &[4, 16, 240, 46])], -2);
        }
        send_array(peer, dispatch, 792, 118, &[1, 42, GENERAL_2D_STYLE, 1], -2);
    }

    if has_effects {
        let price_rows = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo");
        assert_eq!(row(&price_rows, (1 << 8) | 0x5c), pickle!({"gold": 300}));
        let Pickle::Set(hidden) = dict(&dict(&shop, "items"), "notInShopItems") else {
            panic!("Missing shop exclusions");
        };
        assert!(hidden.contains(&((3 << 8) | 0x5c).into()));
    }

    if has_decals {
        let price_rows = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo");
        assert_eq!(row(&price_rows, (8 << 8) | 0x7c), pickle!({"gold": 160}));
        let Pickle::Set(hidden) = dict(&dict(&shop, "items"), "notInShopItems") else {
            panic!("Missing shop exclusions");
        };
        assert!(hidden.contains(&((501 << 8) | 0x7c).into()));
    }

    send_array(
        peer,
        dispatch,
        781,
        118,
        &[0, 42, descriptor(SUMMER), 1],
        -4,
    );
    send_array(
        peer,
        dispatch,
        782,
        118,
        &[1, 42, descriptor(GERMAN), 1],
        -1,
    );
    send_array(peer, dispatch, 783, 118, &[1, 42, descriptor(513), 1], -1);
    send_array(peer, dispatch, 784, 118, &[1, 42, descriptor(SUMMER), 1], 0);
    revision += 1;
    let bought = update(peer, revision);
    assert_eq!(dict(&dict(&bought, "stats"), "gold"), (gold - 50).into());
    assert_eq!(
        row(&dict(&bought, "inventory"), 12),
        pickle!({1: {2: {(replacement(SUMMER)): {0: 1}}}})
    );

    send_outfits(peer, dispatch, 785, 42, &[(1, &summer)], -1);
    send_outfits(peer, dispatch, 786, 42, &[(2, &summer[..9])], -2);
    send_outfits(
        peer,
        dispatch,
        787,
        42,
        &[(15, blank), (1, &winter), (2, &summer), (4, blank)],
        0,
    );
    revision += 1;
    let applied = update(peer, revision);
    let inventory = row(&dict(&applied, "inventory"), 12);
    assert_eq!(row(&inventory, 1), pickle!({2: {(SUMMER): None}}));
    assert_eq!(
        row(&inventory, 4),
        pickle!({2: {(replacement(SUMMER)): {(USA_STARTER): 3}, (replacement(WINTER)): {(USA_STARTER): 1}}})
    );
    assert_eq!(dict(&dict(&applied, "stats"), "gold"), (gold - 200).into());
    let outfits = row(&row(&inventory, 2), USA_STARTER);
    let Pickle::Bytes(saved_summer) = row(&outfits, 2) else {
        panic!("Missing summer outfit")
    };
    let decoded = decode_outfits(&[2], [saved_summer.as_slice()].into_iter(), None).unwrap();
    for surface in [
        VehicleSurface::Hull,
        VehicleSurface::Turret,
        VehicleSurface::Gun,
    ] {
        let camo = decoded.values().next().unwrap().camouflage[&surface];
        assert_eq!(
            (i32::from(camo.id), camo.pattern_size, camo.palette),
            (SUMMER, 2, 2)
        );
    }

    send_outfits(peer, dispatch, 788, 42, &[(1, &winter), (2, &summer)], 0);
    send_outfits(peer, dispatch, 789, 42, &[(2, blank)], 0);
    revision += 1;
    let removed = update(peer, revision);
    let inventory = row(&dict(&removed, "inventory"), 12);
    assert_eq!(
        row(&inventory, 1),
        pickle!({2: {(replacement(SUMMER)): {0: 3}}})
    );
    assert_eq!(row(&inventory, 2), pickle!({(USA_STARTER): {2: None}}));
    assert_eq!(row(&inventory, 4), pickle!({2: {(SUMMER): None}}));
    assert_eq!(dict(&dict(&removed, "stats"), "gold"), (gold - 200).into());

    send_outfits(peer, dispatch, 790, 42, &[(2, &summer)], 0);
    revision += 1;
    let remounted = update(peer, revision);
    assert_eq!(dict(&remounted, "stats"), dict(&removed, "stats"));
    if has_extension_styles {
        // Grass-green UK camouflage on all three Executor surfaces alongside its national emblem.
        let executor_summer = [4, 12, 1, 1, 142, 121, 1, 3, 146, 121, 16];
        send_outfits(
            peer,
            dispatch,
            794,
            43,
            &[(15, blank), (1, blank), (2, &executor_summer), (4, blank)],
            0,
        );
        revision += 1;
        let executor = update(peer, revision);
        assert_eq!(dict(&dict(&executor, "stats"), "gold"), (gold - 350).into());
        let cosmetics = row(&dict(&executor, "inventory"), 12);
        assert_eq!(
            row(&cosmetics, 4),
            pickle!({2: {(replacement(15_502)): {26705: 3}}})
        );
    }
    if has_inscriptions {
        let number = [4, 128, 1, 1, 7, 3, 3, b'0', b'6', b'7', 128, 8];
        let mut prohibited = number;
        prohibited[7..10].copy_from_slice(b"666");
        let mut edited = number;
        edited[9] = b'8';
        send_outfits(peer, dispatch, 795, 43, &[(1, &prohibited)], -1);
        send_outfits(peer, dispatch, 796, 43, &[(1, &number)], 0);
        revision += 1;
        let installed = update(peer, revision);
        assert_eq!(
            dict(&dict(&installed, "stats"), "gold"),
            (gold - 400).into()
        );
        let inventory = row(&dict(&installed, "inventory"), 12);
        assert_eq!(
            row(&inventory, 4),
            pickle!({9: {(replacement(3)): {26705: 1}}})
        );
        send_outfits(peer, dispatch, 797, 43, &[(1, &edited)], 0);
        revision += 1;
        assert_eq!(
            dict(&update(peer, revision), "stats"),
            dict(&installed, "stats")
        );
        send_outfits(peer, dispatch, 798, 43, &[(1, blank)], 0);
        revision += 1;
        let removed = update(peer, revision);
        let inventory = row(&dict(&removed, "inventory"), 12);
        assert_eq!(row(&inventory, 1), pickle!({9: {(replacement(3)): {0: 1}}}));
        send_outfits(peer, dispatch, 799, 43, &[(1, &number)], 0);
        revision += 1;
        assert_eq!(
            dict(&update(peer, revision), "stats"),
            dict(&installed, "stats")
        );

        let mut mixed = vec![4, 136, 1, 1, 3, 153, 117, 128, 16];
        mixed.extend_from_slice(&number[3..]);
        send_outfits(peer, dispatch, 800, 43, &[(1, &mixed)], 0);
        revision += 1;
        let with_inscriptions = update(peer, revision);
        assert_eq!(
            dict(&dict(&with_inscriptions, "stats"), "gold"),
            (gold - 450).into()
        );

        if has_effects {
            send_outfits(peer, dispatch, 801, 43, &[(1, &[4, 1, 1, 3])], -1);
            send_outfits(peer, dispatch, 802, 43, &[(1, &[4, 1, 2, 1, 2])], -2);
            let mut effect = vec![4, 137, 1, 1, 1];
            effect.extend_from_slice(&mixed[3..]);
            send_outfits(peer, dispatch, 803, 43, &[(1, &effect)], 0);
            revision += 1;
            let worn = update(peer, revision);
            assert_eq!(dict(&dict(&worn, "stats"), "gold"), (gold - 750).into());
            assert_eq!(
                row(&row(&dict(&worn, "inventory"), 12), 4),
                pickle!({5: {(replacement(1)): {26705: 1}}})
            );
            send_outfits(peer, dispatch, 804, 43, &[(1, &effect)], 0);
            send_outfits(peer, dispatch, 805, 43, &[(1, &mixed)], 0);
            revision += 1;
            let removed = update(peer, revision);
            assert_eq!(
                row(&row(&dict(&removed, "inventory"), 12), 1),
                pickle!({5: {(replacement(1)): {0: 1}}})
            );
            send_outfits(peer, dispatch, 806, 43, &[(1, &effect)], 0);
            revision += 1;
            assert_eq!(dict(&update(peer, revision), "stats"), dict(&worn, "stats"));
            if has_decals {
                // Tiger fits a square hull anchor, and this anchor only allows horizontal mirroring.
                send_outfits(
                    peer,
                    dispatch,
                    807,
                    43,
                    &[(1, &[4, 32, 1, 5, 8, 153, 117])],
                    -1,
                );
                send_outfits(
                    peer,
                    dispatch,
                    808,
                    43,
                    &[(1, &[4, 32, 1, 7, 8, 2, 152, 117])],
                    -1,
                );
                send_outfits(
                    peer,
                    dispatch,
                    809,
                    43,
                    &[(1, &[4, 32, 2, 5, 8, 152, 117, 5, 8, 152, 117])],
                    -2,
                );
                let with_decal = |slot, scale| {
                    let mut outfit = effect.clone();
                    outfit[1] |= 32;
                    // Insert the projected decal before the personal-number array.
                    outfit.splice(11..11, [1, 15, 8, 1, slot, 117, scale]);
                    outfit
                };
                let tiger = with_decal(152, 1);
                send_outfits(peer, dispatch, 810, 43, &[(1, &tiger)], 0);
                revision += 1;
                let bought = update(peer, revision);
                assert_eq!(dict(&dict(&bought, "stats"), "gold"), (gold - 910).into());
                assert_eq!(
                    row(&row(&dict(&bought, "inventory"), 12), 4),
                    pickle!({7: {(replacement(8)): {26705: 1}}})
                );
                let moved = with_decal(157, 2);
                send_outfits(peer, dispatch, 811, 43, &[(1, &moved)], 0);
                revision += 1;
                assert_eq!(
                    dict(&update(peer, revision), "stats"),
                    dict(&bought, "stats")
                );
                send_outfits(peer, dispatch, 812, 43, &[(1, &effect)], 0);
                revision += 1;
                let removed = update(peer, revision);
                assert_eq!(
                    row(&row(&dict(&removed, "inventory"), 12), 1),
                    pickle!({7: {(replacement(8)): {0: 1}}})
                );
                send_outfits(peer, dispatch, 813, 43, &[(1, &moved)], 0);
                revision += 1;
                assert_eq!(
                    dict(&update(peer, revision), "stats"),
                    dict(&bought, "stats")
                );
            }
        }
    }
    peer.command(dispatch, "doCmdInt3", sync(791, 100));
    peer.until(dispatch, |p| p.has_resource(791));
    let cache = peer.resource(791);
    assert_eq!(dict(&cache, "rev"), revision.into());
    let cosmetics = row(&dict(&cache, "inventory"), 12);
    let outfits = row(&row(&cosmetics, 2), USA_STARTER);
    assert_eq!(row(&outfits, 2), Pickle::Bytes(saved_summer));
    assert!(matches!(row(&outfits, 1), Pickle::Bytes(_)));
    assert_eq!(row(&cosmetics, 1), pickle!({2: {}}));
    let mut fitted = std::collections::BTreeMap::from([
        (SUMMER.into(), pickle!({(USA_STARTER): 3})),
        (WINTER.into(), pickle!({(USA_STARTER): 1})),
    ]);
    if has_extension_styles {
        fitted.insert(15_502.into(), pickle!({26705: 3}));
    }
    if has_inscriptions {
        let executor = row(&row(&cosmetics, 2), 26705);
        let Pickle::Bytes(winter) = row(&executor, 1) else {
            panic!("Missing Executor winter outfit")
        };
        let decoded = decode_outfits(&[1], [winter.as_slice()].into_iter(), None).unwrap();
        let winter_outfit = decoded.values().next().unwrap();
        assert_eq!(winter_outfit.effect, has_effects.then_some(1));
        if has_decals {
            assert_eq!(
                winter_outfit.projection_decals,
                vec![ProjectionDecal {
                    id: 8,
                    slot_id: 15005,
                    options: 1,
                    scale: 2
                }]
            );
        }
        let winter = &winter_outfit.inscriptions;
        assert_eq!(
            winter[&InscriptionSlot::TurretFirst],
            Inscription::Number {
                id: 3,
                digits: "067".into()
            }
        );
        assert_eq!(
            winter[&InscriptionSlot::TurretSecond],
            Inscription::Preset(15_001)
        );
        assert!(matches!(row(&executor, 2), Pickle::Bytes(_)));
        let expected = if has_decals {
            pickle!({2: (Pickle::Dict(fitted)), 3: {15001: {26705: 1}}, 5: {1: {26705: 1}}, 7: {8: {26705: 1}}, 9: {3: {26705: 1}}})
        } else if has_effects {
            pickle!({2: (Pickle::Dict(fitted)), 3: {15001: {26705: 1}}, 5: {1: {26705: 1}}, 9: {3: {26705: 1}}})
        } else {
            pickle!({2: (Pickle::Dict(fitted)), 3: {15001: {26705: 1}}, 9: {3: {26705: 1}}})
        };
        assert_eq!(row(&cosmetics, 4), expected);
    } else {
        assert_eq!(row(&cosmetics, 4), pickle!({2: (Pickle::Dict(fitted))}));
    }
    cache
}

pub fn assert_traces(account: i64, has_inscriptions: bool, has_effects: bool, has_decals: bool) {
    let spans = telemetry_capture::exporter().get_finished_spans().unwrap();
    let field = |span: &opentelemetry_sdk::trace::SpanData, key: &str, expected: &str| {
        span.attributes
            .iter()
            .any(|a| a.key.as_str() == key && a.value.to_string() == expected)
    };
    let mut requests = vec![
        (785, "42", "rejected"),
        (787, "42", "committed"),
        (788, "42", "unchanged"),
    ];
    if has_inscriptions {
        requests.extend([(795, "43", "rejected"), (796, "43", "committed")]);
    }
    if has_effects {
        requests.extend([(801, "43", "rejected"), (803, "43", "committed")]);
    }
    if has_decals {
        requests.extend([(807, "43", "rejected"), (810, "43", "committed")]);
    }
    for (request, inventory_id, outcome) in requests {
        let apply = spans
            .iter()
            .find(|span| {
                span.name == "game.cosmetics.apply"
                    && field(span, "input.request_id", &request.to_string())
                    && field(span, "input.inventory_id", inventory_id)
                    && spans.iter().any(|root| {
                        root.name == "game.command"
                            && field(root, "user.id", &account.to_string())
                            && root.span_context.trace_id() == span.span_context.trace_id()
                    })
            })
            .expect("camouflage entry point");
        let child = |name: &str| {
            spans.iter().find(|span| {
                span.name == name && span.span_context.trace_id() == apply.span_context.trace_id()
            })
        };
        let command = child("player_sessions.commands.command").unwrap();
        assert_eq!(apply.parent_span_id, command.span_context.span_id());
        let progression = child("player_sessions.gateway.change_progression").unwrap();
        assert_eq!(progression.parent_span_id, apply.span_context.span_id());
        let plan = child("accounts.progression.plan_progression").unwrap();
        assert_eq!(plan.parent_span_id, progression.span_context.span_id());
        let decision = child("accounts.cosmetics.change").unwrap();
        assert_eq!(decision.parent_span_id, plan.span_context.span_id());
        assert!(field(decision, "input.operation", "apply"));
        let reply = child("player_sessions.gateway.progression_response").unwrap();
        assert_eq!(reply.parent_span_id, progression.span_context.span_id());
        assert!(field(reply, "output.outcome", outcome));
        assert!(field(reply, "output.delivery", "queued"));
        if outcome == "committed" {
            let (count, bytes, units) = match request {
                796 => ("1", "12", "1"),
                803 => ("1", "20", "1"),
                810 => ("1", "27", "1"),
                _ => ("4", "17", "3"),
            };
            assert!(field(apply, "input.outfits.count", count));
            assert!(field(apply, "input.outfit.bytes", bytes));
            assert!(field(decision, "output.purchases.count", units));
            let persistence = child("accounts.postgres.save_progression").unwrap();
            assert!(spans.iter().any(|span| {
                span.name == "postgresql.query"
                    && span.parent_span_id == persistence.span_context.span_id()
                    && field(span, "db.operation.name", "UPDATE")
                    && span
                        .attributes
                        .iter()
                        .any(|a| a.key.as_str() == "db.query.text")
            }));
        } else {
            assert!(child("postgresql.query").is_none());
        }
        if outcome == "rejected" {
            assert_eq!(decision.status, opentelemetry::trace::Status::error(""));
        }
    }
}

fn send_outfits(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    request: i16,
    inventory_id: i64,
    rows: &[(i64, &[u8])],
    result: i16,
) {
    peer.command(
        dispatch,
        "doCmdIntArrStrArr",
        vec![
            Value::Int16(request),
            Value::Int16(119),
            Value::Seq(
                [1, inventory_id]
                    .into_iter()
                    .chain(rows.iter().map(|row| row.0))
                    .map(Value::Int64)
                    .collect(),
            ),
            Value::Seq(
                rows.iter()
                    .map(|row| Value::String(StringValue::Raw(row.1.to_vec())))
                    .collect(),
            ),
        ],
    );
    peer.until(dispatch, |peer| peer.responses.contains(&(request, result)));
}

fn sync(request: i16, command: i16) -> Vec<Value> {
    vec![
        Value::Int16(request),
        Value::Int16(command),
        Value::Int64(0),
        Value::Int64(0),
        Value::Int64(0),
    ]
}
