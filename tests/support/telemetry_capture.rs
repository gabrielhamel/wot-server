use opentelemetry::trace::{Status, TracerProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use std::{
    net::SocketAddr,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tracing_subscriber::prelude::*;

pub fn exporter() -> &'static InMemorySpanExporter {
    static EXPORTER: OnceLock<InMemorySpanExporter> = OnceLock::new();
    EXPORTER.get_or_init(|| {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("garage-test");
        tracing_subscriber::registry()
            .with(
                tracing_opentelemetry::layer()
                    .with_tracer(tracer)
                    .with_filter(
                        tracing_subscriber::filter::Targets::new()
                            .with_target("wot_server", tracing::Level::INFO),
                    ),
            )
            .init();
        exporter
    })
}

fn xmpp_connection_span(peer: SocketAddr) -> (SpanData, Vec<SpanData>) {
    let address = peer.to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let spans = exporter().get_finished_spans().unwrap();
        if let Some(span) = spans.iter().find(|span| {
            span.name == "player_sessions.serve"
                && has_attribute(span, "input.client.address", &address)
        }) {
            assert!(has_attribute(
                span,
                "ddd.bounded_context.name",
                "player_sessions"
            ));
            return (span.clone(), spans);
        }
        assert!(Instant::now() < deadline, "Missing XMPP span for {peer}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn has_attribute(span: &SpanData, key: &str, expected: &str) -> bool {
    span.attributes
        .iter()
        .any(|attribute| attribute.key.as_str() == key && attribute.value.to_string() == expected)
}

pub fn assert_unimplemented_command_traces(account: i64) {
    let spans = exporter().get_finished_spans().unwrap();
    let method = spans
        .iter()
        .find(|span| {
            span.name == "game.command"
                && has_attribute(span, "user.id", &account.to_string())
                && has_attribute(span, "rpc.method", "doCmdNoArgs")
        })
        .expect("missing unimplemented base method trace");
    assert!(matches!(method.status, Status::Error { .. }));
    assert!(has_attribute(method, "output.outcome", "unimplemented"));
    assert!(
        method
            .events
            .iter()
            .any(|event| { event.name == "Account base method is not implemented" })
    );

    for (request, expects_response) in [("202", false), ("227", true)] {
        let root = spans
            .iter()
            .find(|span| {
                span.name == "game.command"
                    && has_attribute(span, "user.id", &account.to_string())
                    && has_attribute(span, "rpc.method", "doCmdInt3")
                    && has_attribute(span, "input.request_id", request)
                    && has_attribute(span, "input.command_id", "12345")
            })
            .expect("missing game command trace");
        let command = spans
            .iter()
            .find(|span| {
                span.name == "player_sessions.commands.command"
                    && span.span_context.trace_id() == root.span_context.trace_id()
                    && span.parent_span_id == root.span_context.span_id()
            })
            .expect("missing decoded command span");
        assert!(matches!(command.status, Status::Error { .. }));
        assert!(has_attribute(command, "input.request_id", request));
        assert!(has_attribute(command, "input.command_id", "12345"));
        assert!(has_attribute(command, "output.outcome", "unimplemented"));
        assert!(command.events.iter().any(|event| {
            event.name == "Account command is not implemented"
                && event.attributes.iter().any(|attribute| {
                    attribute.key.as_str() == "level" && attribute.value.to_string() == "ERROR"
                })
        }));

        let response = spans.iter().find(|span| {
            span.name == "mercury.response"
                && span.span_context.trace_id() == command.span_context.trace_id()
                && span.parent_span_id == command.span_context.span_id()
        });
        if expects_response {
            let response = response.expect("missing unimplemented command response");
            assert!(has_attribute(response, "input.request_id", request));
            assert!(has_attribute(response, "output.response_code", "-1"));
            assert!(has_attribute(response, "output.delivery", "sent"));
        } else {
            assert!(
                response.is_none(),
                "unexpected response to request {request}"
            );
        }
    }
}

pub fn assert_unimplemented_chat_traces(account: i64) {
    let spans = exporter().get_finished_spans().unwrap();
    let account = account.to_string();
    let assert_error = |span: &SpanData, message| {
        assert!(matches!(span.status, Status::Error { .. }));
        assert!(has_attribute(span, "output.outcome", "unimplemented"));
        assert!(span.events.iter().any(|event| event.name == message));
    };
    let child_of = |span: &SpanData, parent: &SpanData| {
        span.span_context.trace_id() == parent.span_context.trace_id()
            && span.parent_span_id == parent.span_context.span_id()
    };

    for (request, action, response) in [
        ("1901", "32767", "messenger_onActionByServer_chat2"),
        ("1902", "255", "onChatAction"),
    ] {
        let root = spans
            .iter()
            .find(|span| {
                span.name == "game.command"
                    && has_attribute(span, "user.id", &account)
                    && has_attribute(span, "input.request_id", request)
            })
            .expect("missing Mercury chat request");
        let command = spans
            .iter()
            .find(|span| {
                span.name == "player_sessions.social.social_command"
                    && span.span_context.trace_id() == root.span_context.trace_id()
                    && has_attribute(span, "input.action", action)
                    && has_attribute(span, "input.request_id", request)
            })
            .expect("missing Mercury chat action");
        assert_error(command, "Chat action is not implemented");
        assert!(spans.iter().any(|span| {
            span.name == "mercury.response"
                && span.span_context.trace_id() == root.span_context.trace_id()
                && has_attribute(span, "rpc.method", response)
                && has_attribute(span, "output.delivery", "sent")
        }));
    }

    for (id, query) in [
        ("unsupported-stanza", None),
        ("unsupported-storage", Some("storage")),
        ("unsupported-query", Some("unknown")),
    ] {
        let handling = spans
            .iter()
            .find(|span| {
                span.name == "player_sessions.xmpp.handle"
                    && has_attribute(span, "xmpp.stanza.id", id)
                    && has_attribute(span, "user.id", &account)
            })
            .expect("missing XMPP request handling span");
        let root = spans
            .iter()
            .find(|span| span.name == "xmpp.request" && child_of(handling, span))
            .expect("missing XMPP request root");
        if let Some(query) = query {
            let iq = spans
                .iter()
                .find(|span| span.name == "player_sessions.xmpp.iq" && child_of(span, handling))
                .expect("missing XMPP IQ span");
            assert!(has_attribute(iq, "input.xmpp.query", query));
            assert_error(iq, "XMPP IQ operation is not implemented");
            assert!(
                spans
                    .iter()
                    .any(|span| { span.name == "xmpp.response" && child_of(span, root) })
            );
        } else {
            assert!(has_attribute(handling, "xmpp.stanza.name", "unknown"));
            assert_error(handling, "XMPP stanza is not implemented");
            assert!(has_attribute(root, "output.stanza_count", "0"));
        }
    }
}

pub fn assert_xmpp_disconnect(peer: SocketAddr, account: i64, reason: &str) {
    let (connection, spans) = xmpp_connection_span(peer);
    assert_eq!(connection.status, Status::Unset);
    assert!(
        connection
            .events
            .iter()
            .all(|event| event.name != "exception")
    );
    assert!(has_attribute(&connection, "output.outcome", "disconnected"));
    assert!(has_attribute(
        &connection,
        "output.disconnect.reason",
        reason
    ));
    assert!(spans.iter().any(|span| {
        span.name == "player_sessions.presence"
            && span.span_context.trace_id() == connection.span_context.trace_id()
            && span.parent_span_id == connection.span_context.span_id()
            && has_attribute(span, "input.id", &account.to_string())
            && has_attribute(span, "input.available", "false")
    }));
}

pub fn assert_xmpp_connection_failure(peer: SocketAddr, reason: &str) {
    let (connection, _) = xmpp_connection_span(peer);
    assert_eq!(connection.status, Status::error(reason.to_owned()));
    assert!(
        connection
            .events
            .iter()
            .any(|event| event.name == "exception")
    );
    assert!(has_attribute(&connection, "output.outcome", "failed"));
}

pub fn assert_player_spans(account: i64, login: &str, display_name: &str) {
    let spans = exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key: &str, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    assert!(spans.iter().any(|span| {
        span.name == "game.authenticate"
            && has(span, "ddd.bounded_context.name", "player_sessions")
            && has(span, "user.id", &account.to_string())
            && has(span, "user.name", display_name)
            && if login.contains('@') {
                has(span, "user.email", login)
            } else {
                !span
                    .attributes
                    .iter()
                    .any(|attribute| attribute.key.as_str() == "user.email")
            }
            && has(span, "outcome", "authenticated")
            && has(span, "wot.client.language", "en")
            && has(span, "wot.client.protocol_version", "0")
    }));
    assert!(spans.iter().any(|span| {
        span.name == "game.command"
            && has(span, "ddd.bounded_context.name", "player_sessions")
            && has(span, "user.id", &account.to_string())
            && span
                .attributes
                .iter()
                .any(|attribute| attribute.key.as_str() == "rpc.method")
    }));
    assert!(
        spans
            .iter()
            .filter(|span| span.name == "game.command" || span.name == "xmpp.request")
            .all(|span| {
                !span
                    .attributes
                    .iter()
                    .any(|attribute| matches!(attribute.key.as_str(), "user.name" | "user.email"))
            }),
        "requests without identity payloads must not inherit names or emails"
    );
    for name in ["game.authenticate", "game.command", "xmpp.request"] {
        let roots: Vec<_> = spans.iter().filter(|span| span.name == name).collect();
        assert!(!roots.is_empty(), "missing {name}");
        for span in roots {
            assert!(
                has(span, "client.address", "127.0.0.1"),
                "missing IP on {name}"
            );
            assert!(has(span, "network.peer.address", "127.0.0.1"));
            for key in ["client.port", "network.peer.port"] {
                assert!(span.attributes.iter().any(|a| {
                    a.key.as_str() == key
                        && a.value
                            .to_string()
                            .parse::<u16>()
                            .is_ok_and(|port| port != 0)
                }));
            }
        }
    }
    assert!(spans.iter().any(|span| has(span, "outcome", "rejected")));
    assert!(
        spans
            .iter()
            .filter(|span| has(span, "outcome", "rejected"))
            .all(|span| {
                !span
                    .attributes
                    .iter()
                    .any(|attribute| attribute.key.as_str().starts_with("user."))
            })
    );
}

pub fn assert_garage_bootstrap(visible: bool, version_property: &str) {
    let spans = exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    assert!(
        spans.iter().any(|bootstrap| {
            bootstrap.name == "player_sessions.enable"
                && has(
                    bootstrap,
                    "output.account.version_property",
                    version_property,
                )
                && has(
                    bootstrap,
                    "output.training_room.visible",
                    &visible.to_string(),
                )
                && has(
                    bootstrap,
                    "output.training_room.description_enabled",
                    &visible.to_string(),
                )
                && spans.iter().any(|response| {
                    response.name == "mercury.response"
                        && response.span_context.trace_id() == bootstrap.span_context.trace_id()
                        && response.parent_span_id == bootstrap.span_context.span_id()
                        && has(response, "rpc.method", "showGUI")
                        && has(response, "output.delivery", "sent")
                })
        }),
        "Missing Account version property or Training Room settings on a delivered garage bootstrap trace"
    );
}

pub fn assert_equipment_traces(account: i64) {
    use opentelemetry_sdk::trace::SpanData;

    let spans = exporter().get_finished_spans().unwrap();
    let has = |span: &SpanData, key: &str, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    let child = |parent: &SpanData, name| {
        spans
            .iter()
            .find(|span| {
                span.name == name
                    && span.span_context.trace_id() == parent.span_context.trace_id()
                    && span.parent_span_id == parent.span_context.span_id()
            })
            .unwrap_or_else(|| panic!("Missing {name} under {}", parent.name))
    };
    let cases = [
        (
            "863",
            "310",
            "sell_items",
            "accounts.depot.sell_depot_stock",
            None,
        ),
        (
            "865",
            "305",
            "sell_items",
            "accounts.depot.sell_depot_stock",
            Some(("9", "1600", "14400", "14400")),
        ),
        (
            "867",
            "310",
            "sell_items",
            "accounts.depot.sell_depot_stock",
            Some(("1", "400", "400", "14900")),
        ),
        (
            "870",
            "10115",
            "disassemble_equipment",
            "accounts.equipment.apply",
            None,
        ),
        (
            "871",
            "10115",
            "disassemble_equipment",
            "accounts.equipment.apply",
            Some(("1", "1600", "1600", "16500")),
        ),
        (
            "874",
            "102",
            "change_equipment",
            "accounts.equipment.apply",
            Some(("1", "100", "100", "16600")),
        ),
    ];
    let upgrades = [
        (
            "888",
            "accounts.equipment.upgrade_depot",
            Some(("29689", "29945", "input.price.credits", "3000000")),
        ),
        ("889", "accounts.equipment.upgrade_depot", None),
        (
            "891",
            "accounts.equipment.upgrade_mounted",
            Some(("13049", "13305", "input.price.credits", "3000000")),
        ),
        ("892", "accounts.equipment.upgrade_mounted", None),
        (
            "894",
            "accounts.equipment.upgrade_depot",
            Some(("40953", "41209", "input.price.components", "400")),
        ),
        ("895", "accounts.equipment.upgrade_depot", None),
        (
            "897",
            "accounts.equipment.upgrade_mounted",
            Some(("41209", "41465", "input.price.components", "2000")),
        ),
        ("898", "accounts.equipment.upgrade_mounted", None),
    ];
    let upgrade_cases =
        upgrades.map(|(request, span, _)| (request, "10050", "upgrade_equipment", span, None));
    for (request, command, operation, use_case, payout) in cases.into_iter().chain(upgrade_cases) {
        let root = spans
            .iter()
            .find(|span| {
                span.name == "game.command"
                    && has(span, "user.id", &account.to_string())
                    && has(span, "input.command_id", command)
                    && has(span, "input.request_id", request)
            })
            .unwrap();
        let dispatch = child(root, "player_sessions.commands.command");
        let gateway = child(dispatch, "player_sessions.gateway.change_progression");
        assert!(has(gateway, "input.shop_revision", "1"));
        let plan = child(gateway, "accounts.progression.plan_progression");
        assert!(has(plan, "input.operation", operation));
        let completion = child(gateway, "player_sessions.gateway.progression_response");
        let response = spans
            .iter()
            .find(|span| {
                span.name == "mercury.response"
                    && span.parent_span_id == completion.span_context.span_id()
                    && has(span, "input.request_id", request)
            })
            .unwrap();
        assert!(has(response, "output.delivery", "sent"));

        let use_case = child(plan, use_case);
        let upgrade_result = upgrades
            .iter()
            .find(|(id, _, _)| *id == request)
            .and_then(|(_, _, result)| *result);
        if let Some((item, target, price_attribute, amount)) = upgrade_result {
            assert!(has(use_case, "input.item", item));
            assert!(has(use_case, price_attribute, amount));
            assert!(has(use_case, "output.item", target));
            if request == "891" {
                assert!(has(use_case, "input.inventory_id", "3"));
                assert!(has(use_case, "input.slot", "1"));
            }
        }
        if let Some((count, unit_value, received, balance)) = payout {
            assert!(spans.iter().any(|span| {
                span.name == "accounts.equipment.credit_disassembly"
                    && span.parent_span_id == use_case.span_context.span_id()
                    && has(span, "input.count", count)
                    && has(span, "input.components.per_item", unit_value)
                    && has(span, "output.components.received", received)
                    && has(span, "output.components.balance", balance)
            }));
        }
        if payout.is_some() || upgrade_result.is_some() {
            let commit = child(gateway, "accounts.garage.commit_progression");
            let save = child(commit, "accounts.save_progression");
            let repository = child(save, "accounts.postgres.save_progression");
            assert!(spans.iter().any(|span| {
                span.name == "postgresql.query"
                    && span.parent_span_id == repository.span_context.span_id()
                    && has(span, "db.operation.name", "UPDATE")
            }));
            assert!(has(completion, "output.outcome", "committed"));
            assert!(has(response, "output.response_code", "0"));
            assert!(spans.iter().any(|span| {
                span.name == "mercury.response"
                    && span.parent_span_id == completion.span_context.span_id()
                    && has(span, "rpc.method", "update")
                    && has(span, "output.delivery", "sent")
            }));
        } else {
            assert!(matches!(
                plan.status,
                opentelemetry::trace::Status::Error { .. }
            ));
            assert!(has(completion, "output.outcome", "rejected"));
            assert!(has(response, "output.response_code", "-1"));
            assert!(!spans.iter().any(|span| {
                span.name == "accounts.postgres.save_progression"
                    && span.span_context.trace_id() == root.span_context.trace_id()
            }));
        }
    }
}

pub fn assert_operation_traces(account: i64, research_account: i64, online_players: usize) {
    let spans = exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key: &str, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    let descendant = |span: &opentelemetry_sdk::trace::SpanData,
                      root: &opentelemetry_sdk::trace::SpanData| {
        if span.span_context.trace_id() != root.span_context.trace_id() {
            return false;
        }
        let mut parent = span.parent_span_id;
        for _ in 0..spans.len() {
            if parent == root.span_context.span_id() {
                return true;
            }
            let Some(ancestor) = spans
                .iter()
                .find(|candidate| candidate.span_context.span_id() == parent)
            else {
                break;
            };
            parent = ancestor.parent_span_id;
        }
        false
    };
    assert!(
        spans.iter().all(|span| {
            [
                "player_sessions",
                "accounts",
                "social",
                "player_administration",
                "server",
            ]
            .iter()
            .any(|context| has(span, "ddd.bounded_context.name", context))
        }),
        "every exported operation must identify its bounded context"
    );
    for (outcome, response_code) in [("empty", "1"), ("invalid_request", "-1")] {
        let listing = spans
            .iter()
            .find(|span| {
                span.name == "player_sessions.list_training_rooms"
                    && has(span, "input.args.count", "5")
                    && has(span, "output.outcome", outcome)
                    && spans.iter().any(|root| {
                        root.name == "game.command"
                            && has(root, "user.id", &account.to_string())
                            && has(root, "input.command_id", "520")
                            && has(root, "input.request_id", "200")
                            && descendant(span, root)
                    })
            })
            .unwrap_or_else(|| panic!("Missing Training Rooms list trace for {outcome}"));
        assert!(spans.iter().any(|span| {
            span.name == "mercury.response"
                && has(span, "output.response_code", response_code)
                && has(span, "output.delivery", "sent")
                && descendant(span, listing)
        }));
        let resource = spans
            .iter()
            .find(|span| span.name == "mercury.response.resource" && descendant(span, listing));
        if outcome == "empty" {
            assert!(has(listing, "output.rooms.count", "0"));
            assert_eq!(
                resource.unwrap().parent_span_id,
                listing.span_context.span_id()
            );
        } else {
            assert!(resource.is_none());
        }
    }
    let command = spans
        .iter()
        .find(|root| {
            root.name == "game.command"
                && has(root, "user.id", &research_account.to_string())
                && spans
                    .iter()
                    .any(|span| span.name == "accounts.modules.research" && descendant(span, root))
                && spans.iter().any(|span| {
                    span.name == "player_sessions.gateway.progression_response"
                        && has(span, "ddd.bounded_context.name", "player_sessions")
                        && descendant(span, root)
                })
                && spans.iter().any(|span| {
                    span.name == "accounts.postgres.save_progression" && descendant(span, root)
                })
        })
        .expect("command trace must include queued persistence and response");
    assert_eq!(
        command.parent_span_id,
        opentelemetry::trace::SpanId::INVALID
    );
    for operation in [
        "accounts.project_garage",
        "accounts.encode_garage",
        "accounts.save_progression",
        "accounts.postgres.save_progression",
        "accounts.modules.research",
    ] {
        assert!(
            spans
                .iter()
                .any(|span| span.name == operation && descendant(span, command)),
            "missing {operation} under command"
        );
    }
    assert!(
        spans
            .iter()
            .any(|span| span.name == "accounts.progression.plan_progression"
                && descendant(span, command)
                && has(span, "input.operation", "research"))
    );
    assert!(spans.iter().any(|span| span.name == "mercury.response"
        && descendant(span, command)
        && has(span, "rpc.method", "onCmdResponseExt")
        && has(span, "output.response_code", "0")
        && has(span, "output.delivery", "sent")));

    let command_root = |request: &str, id: &str| {
        spans
            .iter()
            .find(|root| {
                root.name == "game.command"
                    && has(root, "user.id", &account.to_string())
                    && has(root, "input.request_id", request)
                    && has(root, "input.command_id", id)
            })
            .unwrap_or_else(|| panic!("missing command {id} request {request}"))
    };
    let stats = command_root("2000", "501");
    let online_players = online_players.to_string();
    let read = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.server_stats"
                && descendant(span, stats)
                && has(span, "output.players.online", &online_players)
        })
        .expect("online count operation");
    assert!(spans.iter().any(|span| span.name == "mercury.response"
        && span.parent_span_id == read.span_context.span_id()
        && has(span, "rpc.method", "receiveServerStats")
        && has(span, "output.delivery", "sent")));
    assert!(
        !spans
            .iter()
            .any(|span| span.name == "postgresql.query" && descendant(span, stats))
    );

    let settings = command_root("225", "1600");
    let completion = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.settings.response"
                && descendant(span, settings)
                && has(span, "output.outcome", "committed")
        })
        .expect("settings completion");
    assert!(spans.iter().any(|span| {
        span.name == "postgresql.query"
            && descendant(span, settings)
            && has(span, "db.operation.name", "UPDATE")
            && span.attributes.iter().any(|attr| {
                attr.key.as_str() == "db.query.text" && attr.value.to_string().contains("$1")
            })
    }));
    assert!(spans.iter().any(|span| span.name == "mercury.response"
        && span.parent_span_id == completion.span_context.span_id()
        && has(span, "input.request_id", "225")
        && has(span, "output.delivery", "sent")));
    for (request, operation, code, outcome) in [
        (
            "229",
            "player_sessions.gateway.change_ammunition",
            "-2",
            "invalid_request",
        ),
        (
            "239",
            "player_sessions.gateway.ammunition_response",
            "0",
            "unchanged",
        ),
    ] {
        let root = command_root(request, "108");
        let action = spans
            .iter()
            .find(|span| {
                span.name == operation
                    && descendant(span, root)
                    && has(span, "output.outcome", outcome)
            })
            .expect(outcome);
        assert!(spans.iter().any(|span| span.name == "mercury.response"
            && span.parent_span_id == action.span_context.span_id()
            && has(span, "output.response_code", code)
            && has(span, "output.delivery", "sent")));
        assert!(!spans.iter().any(|span| {
            span.name == "accounts.postgres.save_ammunition" && descendant(span, root)
        }));
        assert_eq!(
            spans.iter().any(|span| {
                span.name == "accounts.postgres.snapshot" && descendant(span, root)
            }),
            outcome == "unchanged"
        );
    }

    let replacement = spans
        .iter()
        .find(|admission| {
            admission.name == "player_sessions.admit"
                && has(admission, "user.id", &account.to_string())
                && has(admission, "output.outcome", "replaced")
                && spans.iter().any(|root| {
                    root.name == "game.authenticate"
                        && has(root, "user.id", &account.to_string())
                        && admission.parent_span_id == root.span_context.span_id()
                })
        })
        .expect("Expected a replacement admission");
    assert!(
        spans
            .iter()
            .any(|span| span.name == "mercury.login.response"
                && span.parent_span_id == replacement.span_context.span_id()
                && has(span, "output.response_code", "1")
                && has(span, "output.delivery", "sent"))
    );
    assert!(
        spans.iter().all(|span| {
            !span.name.split('.').any(|segment| {
                matches!(
                    segment,
                    "application" | "domain" | "infrastructure" | "presentation"
                )
            })
        }),
        "exported span names must omit architecture layer segments"
    );
    let message = spans
        .iter()
        .find(|root| {
            root.name == "xmpp.request"
                && has(root, "ddd.bounded_context.name", "player_sessions")
                && spans.iter().any(|span| {
                    span.name == "social.send" && descendant(span, root)
                })
                && spans.iter().any(|enqueue| {
                    enqueue.name == "player_sessions.enqueue"
                        && has(enqueue, "output.delivery", "queued")
                        && descendant(enqueue, root)
                        && spans.iter().any(|response| {
                            response.name == "xmpp.response"
                                && response.parent_span_id == enqueue.span_context.span_id()
                        })
                })
                && ["player_sessions.xmpp.handle", "xmpp.response"]
                    .iter()
                    .all(|operation| {
                        spans.iter().any(|span| {
                            span.name == *operation
                                && has(span, "ddd.bounded_context.name", "player_sessions")
                                && descendant(span, root)
                        })
                    })
        })
        .expect("XMPP message must retain its parent through database persistence and outgoing delivery");
    assert_eq!(
        message.parent_span_id,
        opentelemetry::trace::SpanId::INVALID
    );
    assert!(spans.iter().any(|span| {
        span.name == "postgresql.query"
            && descendant(span, message)
            && has(span, "ddd.bounded_context.name", "social")
            && has(
                span,
                "db.query.text",
                "INSERT INTO social.private_messages(conversation_id,sender_id,body) VALUES($1,$2,$3)",
            )
            && has(span, "db.system.name", "postgresql")
    }));
    let send = spans
        .iter()
        .find(|span| span.name == "social.conversations.send_message" && descendant(span, message))
        .expect("Social must persist the message under the incoming request");
    let directory = spans
        .iter()
        .find(|span| {
            span.name == "accounts.directory.find"
                && span.parent_span_id == send.span_context.span_id()
                && has(span, "ddd.bounded_context.name", "accounts")
                && has(span, "input.ids.count", "2")
        })
        .expect("Social must resolve both participants through Accounts");
    assert!(spans.iter().any(|span| {
        span.name == "postgresql.query"
            && span.parent_span_id == directory.span_context.span_id()
            && has(span, "ddd.bounded_context.name", "accounts")
            && has(
                span,
                "db.query.text",
                "SELECT id, display_name FROM accounts.accounts WHERE id = ANY($1)",
            )
    }));
    assert!(
        spans
            .iter()
            .filter(|span| descendant(span, message))
            .all(|span| {
                !span.attributes.iter().any(|attribute| {
                    attribute.key.as_str().contains("password")
                        || attribute.value.to_string().contains("Online reply")
                })
            })
    );
}

pub async fn assert_database_failure(url: &str) {
    use wot_server::accounts::test_support::{AccountRepository, PostgresAccounts};
    let repository = PostgresAccounts::new(
        wot_server::server::test_support::connect_database(url, 1)
            .await
            .unwrap(),
    );
    repository.connection_pool().close().await;
    let before = exporter().get_finished_spans().unwrap().len();
    assert!(repository.snapshot(-1).await.is_err());
    let spans = exporter().get_finished_spans().unwrap();
    assert!(
        spans.iter().skip(before).any(|span| {
            span.name == "postgresql.query"
                && matches!(span.status, opentelemetry::trace::Status::Error { .. })
                && span.attributes.iter().any(|attribute| {
                    attribute.key.as_str() == "db.query.text"
                        && attribute.value.to_string() == "BEGIN"
                })
                && span
                    .attributes
                    .iter()
                    .any(|attribute| attribute.key.as_str() == "error.type")
        }),
        "database failure must retain SQL text and error status"
    );
}
