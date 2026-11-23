use super::{
    Peer, Pickle, Value,
    ammunition_flow::update,
    depot_flow::{row, send_array},
    dict, telemetry_capture,
};
use wot_accounts::test_support::decode_outfits;
use wot_accounts::test_support::{
    Inscription, InscriptionSlot, PaintRegion, ProjectionDecal, VehicleSurface,
};
use wot_platform::mercury::{app::dispatch::ScriptDispatch, script::StringValue};
use wot_platform::pickle;

const SUMMER: i32 = 15_204;
const WINTER: i32 = 15_202;
const GERMAN: i32 = 15_116;
const USA_STARTER: i32 = 545;

#[derive(Clone, Copy)]
pub struct CosmeticFixtures {
    pub has_exterior_styles: bool,
    pub has_extension_styles: bool,
    pub has_inscriptions: bool,
    pub has_effects: bool,
    pub has_decals: bool,
    pub has_paints: bool,
    pub has_attachments: bool,
    pub has_stat_trackers: bool,
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
        has_paints,
        ..
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
            send_outfits(peer, dispatch, 793, 43, &[(15, &[4, 16, 240, 46])], -1);
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

    if has_paints {
        let price_rows = dict(prices.state.as_ref().unwrap(), "_itemsPriceInfo");
        assert_eq!(row(&price_rows, (141 << 8) | 0x1c), pickle!({"gold": 20}));
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
                if has_paints {
                    exercise_paint(peer, dispatch, &moved, gold - 910, &mut revision);
                }
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
    let depot = if has_paints {
        pickle!({1: {141: {0: 3}}, 2: {}})
    } else {
        pickle!({2: {}})
    };
    assert_eq!(row(&cosmetics, 1), depot);
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
        if has_paints {
            assert_eq!(winter_outfit.paints.len(), 1);
            assert_eq!(winter_outfit.paints[&PaintRegion::Hull].id, 141);
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
        let expected = if has_paints {
            pickle!({1: {141: {26705: 1}}, 2: (Pickle::Dict(fitted)), 3: {15001: {26705: 1}}, 5: {1: {26705: 1}}, 7: {8: {26705: 1}}, 9: {3: {26705: 1}}})
        } else if has_decals {
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

fn exercise_paint(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    winter: &[u8],
    gold: i64,
    revision: &mut i64,
) {
    let hull_only = [4, 2, 1, 3, 141, 1, 16];
    let gun_2_only = [4, 2, 1, 3, 141, 1, 128, 128, 1];
    send_outfits(peer, dispatch, 814, 42, &[(1, &hull_only)], -1);
    send_outfits(peer, dispatch, 815, 43, &[(1, &gun_2_only)], -1);

    let with_paint = |entry: &[u8]| {
        let mut outfit = winter.to_vec();
        outfit[1] |= 2;
        outfit.splice(5..5, entry.iter().copied());
        outfit
    };
    let all_surfaces = with_paint(&[1, 3, 141, 1, 145, 34]);
    send_outfits(peer, dispatch, 816, 43, &[(1, &all_surfaces)], 0);
    *revision += 1;
    let bought = update(peer, *revision);
    assert_eq!(dict(&dict(&bought, "stats"), "gold"), (gold - 80).into());
    let replacement = Pickle::Tuple(vec![141.into(), "_r".into()]);
    assert_eq!(
        row(&row(&dict(&bought, "inventory"), 12), 4),
        pickle!({1: {(replacement.clone()): {26705: 4}}})
    );
    send_outfits(peer, dispatch, 817, 43, &[(1, &all_surfaces)], 0);

    send_outfits(peer, dispatch, 818, 43, &[(1, winter)], 0);
    *revision += 1;
    let removed = update(peer, *revision);
    assert_eq!(
        row(&row(&dict(&removed, "inventory"), 12), 1),
        pickle!({1: {(replacement): {0: 4}}})
    );

    let hull = with_paint(&hull_only[2..]);
    send_outfits(peer, dispatch, 819, 43, &[(1, &hull)], 0);
    *revision += 1;
    assert_eq!(
        dict(&update(peer, *revision), "stats"),
        dict(&bought, "stats")
    );
}

pub fn exercise_attachments(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
    grant: impl Fn(
        wot_accounts::test_support::PlayerGrant,
    ) -> Result<
        (
            wot_accounts::contract::AccountView,
            wot_player_sessions::contract::GrantDelivery,
        ),
        wot_player_sessions::contract::GrantRequestError,
    >,
) -> Pickle {
    use wot_accounts::test_support::{Attachment, OutfitSeason, PlayerGrant};
    use wot_player_sessions::contract::{GrantDelivery, GrantRequestError};
    const DRAGON_BOAT: i32 = 2033;
    const EXECUTOR: i32 = 26705;
    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("Missing garage revision");
    };
    let new_grant = |id, count| PlayerGrant::Attachment { id, count };
    let boat = [4, 128, 4, 1, 7, 241, 15, 240, 168, 2, 2]; // Turret anchor 38000 at scale 2.
    let wrong_anchor = [4, 128, 4, 1, 7, 241, 15, 208, 140, 1, 2]; // Universal anchor 18000.
    let wrong_scale = [4, 128, 4, 1, 7, 241, 15, 240, 168, 2, 3];
    let rotated = [4, 128, 4, 1, 15, 241, 15, 240, 168, 2, 2, 1];

    let shop = peer.resource(780);
    let Pickle::Set(hidden) = dict(&dict(&shop, "items"), "notInShopItems") else {
        panic!("Missing shop exclusions");
    };
    assert!(hidden.contains(&((DRAGON_BOAT << 8) | 0xcc).into()));
    send_outfits(peer, dispatch, 820, 43, &[(15, &boat)], -1);
    assert!(matches!(
        grant(new_grant(0, 1)),
        Err(GrantRequestError::Rejected(_))
    ));
    assert!(matches!(
        grant(new_grant(2033, 0)),
        Err(GrantRequestError::Rejected(_))
    ));
    let update_count = |peer: &Peer| {
        peer.methods
            .iter()
            .filter(|(name, _)| name.ends_with("update"))
            .count()
    };
    let prior_updates = update_count(peer);
    assert_eq!(
        grant(new_grant(2033, 1)).unwrap().1,
        GrantDelivery::QueuedForGarage
    );
    let replacement = Pickle::Tuple(vec![DRAGON_BOAT.into(), "_r".into()]);
    let inventory = |cache: &Pickle| row(&dict(cache, "inventory"), 12);
    let gold = |cache: &Pickle| dict(&dict(cache, "stats"), "gold");

    peer.until(dispatch, |peer| update_count(peer) > prior_updates);
    let granted = update(peer, revision + 1);
    assert_eq!(
        row(&inventory(&granted), 1),
        pickle!({12: {(replacement.clone()): {0: 1}}})
    );
    assert_eq!(gold(&granted), gold(before));

    send_outfits(peer, dispatch, 821, 43, &[(1, &boat)], -2);
    send_outfits(peer, dispatch, 822, 43, &[(15, &wrong_anchor)], -1);
    send_outfits(peer, dispatch, 823, 43, &[(15, &wrong_scale)], -1);
    send_outfits(peer, dispatch, 824, 43, &[(15, &rotated)], -1);

    let Pickle::Dict(outfits) = row(&inventory(before), 2) else {
        panic!("Missing outfits");
    };
    let executor_before = outfits
        .get(&EXECUTOR.into())
        .cloned()
        .unwrap_or_else(|| pickle!({}));
    let seasonal_outfit = |table: &Pickle, season| {
        let Pickle::Dict(outfits) = table else {
            panic!("Expected seasonal outfit table");
        };
        match outfits.get(&Pickle::from(season)) {
            Some(Pickle::Bytes(bytes)) => bytes.clone(),
            None => Vec::new(),
            value => panic!("Unexpected Executor outfit {value:?}"),
        }
    };
    let seasonal = [1, 2, 4].map(|season| seasonal_outfit(&executor_before, season));
    let rows = [
        (15, boat.as_slice()),
        (1, seasonal[0].as_slice()),
        (2, seasonal[1].as_slice()),
        (4, seasonal[2].as_slice()),
    ];
    send_outfits(peer, dispatch, 825, 43, &rows, 0);
    let fitted = update(peer, revision + 2);
    assert_eq!(
        row(&inventory(&fitted), 4),
        pickle!({12: {(replacement.clone()): {(EXECUTOR): 1}}})
    );
    assert_eq!(gold(&fitted), gold(before));
    send_outfits(peer, dispatch, 826, 43, &rows, 0);

    send_outfits(peer, dispatch, 827, 43, &[(15, &[])], 0);
    let removed = update(peer, revision + 3);
    assert_eq!(
        row(&inventory(&removed), 1),
        pickle!({12: {(replacement): {0: 1}}})
    );
    send_outfits(peer, dispatch, 828, 43, &[(15, &boat)], 0);
    let outfitted = update(peer, revision + 4);
    assert_eq!(gold(&outfitted), gold(before));

    peer.command(dispatch, "doCmdInt3", sync(829, 100));
    peer.until(dispatch, |peer| peer.has_resource(829));
    let cache = peer.resource(829);
    let executor = row(&row(&inventory(&cache), 2), EXECUTOR);
    let Pickle::Bytes(common) = row(&executor, 15) else {
        panic!("Missing common outfit with attachment");
    };
    let decoded = decode_outfits(&[15], [common.as_slice()].into_iter(), None).unwrap();
    assert_eq!(
        decoded[&OutfitSeason::Common].attachments[&38000],
        Attachment {
            id: 2033,
            scale: 2,
            rotated: false
        },
    );
    for season in [1, 2, 4] {
        assert_eq!(
            seasonal_outfit(&executor, season),
            seasonal_outfit(&executor_before, season)
        );
    }
    assert_eq!(dict(&cache, "stats"), dict(before, "stats"));
    cache
}

pub fn exercise_stat_tracker(
    peer: &mut Peer,
    dispatch: &ScriptDispatch,
    before: &Pickle,
    grant: impl Fn(
        wot_accounts::test_support::PlayerGrant,
    ) -> Result<
        (
            wot_accounts::contract::AccountView,
            wot_player_sessions::contract::GrantDelivery,
        ),
        wot_player_sessions::contract::GrantRequestError,
    >,
) -> Pickle {
    use wot_accounts::test_support::{OutfitSeason, PlayerGrant, StatTracker};
    use wot_player_sessions::contract::{GrantDelivery, GrantRequestError};
    const EXECUTOR: i32 = 26705;
    let Pickle::I64(revision) = dict(before, "rev") else {
        panic!("Missing garage revision");
    };
    let shop = peer.resource(780);
    let Pickle::Set(hidden) = dict(&dict(&shop, "items"), "notInShopItems") else {
        panic!("Missing shop exclusions");
    };
    assert!(hidden.contains(&((1 << 8) | 0xdc).into()));
    let inventory = |cache: &Pickle| row(&dict(cache, "inventory"), 12);
    let executor = row(&row(&inventory(before), 2), EXECUTOR);
    let Pickle::Bytes(boat) = row(&executor, 15) else {
        panic!("Missing common outfit with attachment");
    };
    let with_tracker = |slot| {
        let mut outfit = boat.clone();
        outfit[2] |= 32;
        outfit.extend([1, 3, 1, slot, 140, 1]);
        outfit
    };
    let tracker = with_tracker(211); // Executor's dedicated anchor 18003.
    let wrong_slot = with_tracker(208); // Universal attachment anchor 18000.
    let malformed = [4, 128, 32, 2];
    send_outfits(peer, dispatch, 830, 43, &[(15, &tracker)], -1);
    send_outfits(peer, dispatch, 831, 43, &[(15, &malformed)], -2);

    let make_grant = |vehicle, count| PlayerGrant::StatTracker {
        vehicle,
        id: 1,
        count,
    };
    assert!(matches!(
        grant(make_grant(42, 1)),
        Err(GrantRequestError::Rejected(_))
    ));
    assert!(matches!(
        grant(make_grant(43, 0)),
        Err(GrantRequestError::Rejected(_))
    ));
    let updates = |peer: &Peer| {
        peer.methods
            .iter()
            .filter(|(name, _)| name.ends_with("update"))
            .count()
    };
    let before_updates = updates(peer);
    assert_eq!(
        grant(make_grant(43, 1)).unwrap().1,
        GrantDelivery::QueuedForGarage
    );
    peer.until(dispatch, |peer| updates(peer) > before_updates);
    let replacement = Pickle::Tuple(vec![1.into(), "_r".into()]);
    let stock = pickle!({13: {(replacement.clone()): {(EXECUTOR): 1}}});
    assert_eq!(row(&inventory(&update(peer, revision + 1)), 1), stock);

    send_outfits(peer, dispatch, 832, 43, &[(15, &wrong_slot)], -1);
    send_outfits(peer, dispatch, 833, 43, &[(1, &tracker)], -2);
    send_outfits(peer, dispatch, 834, 43, &[(15, &tracker)], 0);
    let fitted = update(peer, revision + 2);
    assert_eq!(
        row(&inventory(&fitted), 4),
        pickle!({13: {(replacement): {(EXECUTOR): 1}}})
    );
    send_outfits(peer, dispatch, 835, 43, &[(15, &tracker)], 0);
    send_outfits(peer, dispatch, 836, 43, &[(15, &boat)], 0);
    assert_eq!(row(&inventory(&update(peer, revision + 3)), 1), stock);
    send_outfits(peer, dispatch, 837, 43, &[(15, &tracker)], 0);

    peer.command(dispatch, "doCmdInt3", sync(838, 100));
    peer.until(dispatch, |peer| peer.has_resource(838));
    let cache = peer.resource(838);
    assert_eq!(dict(&cache, "rev"), (revision + 4).into());
    assert_eq!(dict(&cache, "stats"), dict(before, "stats"));
    let executor_after = row(&row(&inventory(&cache), 2), EXECUTOR);
    let Pickle::Bytes(common) = row(&executor_after, 15) else {
        panic!("Missing common outfit with stat tracker");
    };
    let before = decode_outfits(&[15], [boat.as_slice()].into_iter(), None).unwrap();
    let after = decode_outfits(&[15], [common.as_slice()].into_iter(), None).unwrap();
    assert_eq!(
        after[&OutfitSeason::Common].stat_tracker,
        Some(StatTracker {
            id: 1,
            slot_id: 18003
        })
    );
    assert_eq!(
        before[&OutfitSeason::Common].attachments,
        after[&OutfitSeason::Common].attachments
    );
    cache
}

pub fn assert_traces(account: i64, fixtures: CosmeticFixtures) {
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
    if fixtures.has_inscriptions {
        requests.extend([(795, "43", "rejected"), (796, "43", "committed")]);
    }
    if fixtures.has_effects {
        requests.extend([(801, "43", "rejected"), (803, "43", "committed")]);
    }
    if fixtures.has_decals {
        requests.extend([(807, "43", "rejected"), (810, "43", "committed")]);
    }
    if fixtures.has_paints {
        requests.extend([
            (814, "42", "rejected"),
            (815, "43", "rejected"),
            (816, "43", "committed"),
            (817, "43", "unchanged"),
            (819, "43", "committed"),
        ]);
    }
    if fixtures.has_attachments {
        requests.extend([
            (820, "43", "rejected"),
            (822, "43", "rejected"),
            (826, "43", "unchanged"),
            (828, "43", "committed"),
        ]);
        assert_cosmetic_grant_trace(
            account,
            &spans,
            "accounts.cosmetics.grant_attachment",
            "input.attachment_id",
            "2033",
        );
    }
    if fixtures.has_stat_trackers {
        requests.extend([
            (830, "43", "rejected"),
            (832, "43", "rejected"),
            (834, "43", "committed"),
            (835, "43", "unchanged"),
        ]);
        assert_cosmetic_grant_trace(
            account,
            &spans,
            "accounts.cosmetics.grant_stat_tracker",
            "input.stat_tracker_id",
            "1",
        );
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
                816 => ("1", "33", "4"),
                819 => ("1", "32", "0"),
                828 => ("1", "11", "0"),
                834 => ("1", "18", "0"),
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
            assert!(child("accounts.postgres.save_progression").is_none());
        }
        if outcome == "rejected" {
            assert_eq!(decision.status, opentelemetry::trace::Status::error(""));
        }
    }
}

fn assert_cosmetic_grant_trace(
    account: i64,
    spans: &[opentelemetry_sdk::trace::SpanData],
    operation: &str,
    item_field: &str,
    item_id: &str,
) {
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, value: &str| {
        span.attributes
            .iter()
            .any(|attr| attr.key.as_str() == key && attr.value.to_string() == value)
    };
    let find_grant = |count| {
        spans
            .iter()
            .find(|span| {
                span.name == operation
                    && has(span, item_field, item_id)
                    && has(span, "input.amount", count)
                    && spans.iter().any(|gateway| {
                        gateway.name == "player_sessions.gateway.grant"
                            && gateway.span_context.trace_id() == span.span_context.trace_id()
                            && has(gateway, "user.id", &account.to_string())
                    })
            })
            .unwrap()
    };
    let rejected = find_grant("0");
    assert!(matches!(
        rejected.status,
        opentelemetry::trace::Status::Error { .. }
    ));
    assert!(!spans.iter().any(|span| {
        span.name == "accounts.postgres.save_progression"
            && span.span_context.trace_id() == rejected.span_context.trace_id()
    }));
    let grant = find_grant("1");
    assert!(has(grant, "output.owned.count", "1"));
    let in_trace = |name| {
        spans
            .iter()
            .find(|span| {
                span.name == name && span.span_context.trace_id() == grant.span_context.trace_id()
            })
            .unwrap()
    };
    let plan = in_trace("accounts.grants.plan_grant");
    assert_eq!(grant.parent_span_id, plan.span_context.span_id());
    let gateway = in_trace("player_sessions.gateway.grant");
    assert_eq!(plan.parent_span_id, gateway.span_context.span_id());
    let request = in_trace("player_sessions.grants.grant");
    assert_eq!(gateway.parent_span_id, request.span_context.span_id());
    assert!(has(gateway, "user.id", &account.to_string()));
    let reply = in_trace("player_sessions.gateway.grant_response");
    assert_eq!(reply.parent_span_id, gateway.span_context.span_id());
    assert!(has(reply, "output.success", "true"));
    let write = in_trace("accounts.postgres.save_progression");
    let save = in_trace("accounts.save_progression");
    assert_eq!(write.parent_span_id, save.span_context.span_id());
    let commit = in_trace("accounts.garage.commit_progression");
    assert_eq!(save.parent_span_id, commit.span_context.span_id());
    assert_eq!(commit.parent_span_id, gateway.span_context.span_id());
    assert!(spans.iter().any(|span| {
        span.name == "postgresql.query"
            && span.parent_span_id == write.span_context.span_id()
            && has(span, "db.operation.name", "UPDATE")
    }));
}

pub(super) fn send_outfits(
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
