use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use std::sync::OnceLock;
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

pub fn assert_operation_traces(account: i64, research_account: i64) {
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
                && spans
                    .iter()
                    .any(|span| span.name == "postgresql.query" && descendant(span, root))
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
    let read = spans
        .iter()
        .find(|span| {
            span.name == "player_sessions.server_stats"
                && descendant(span, stats)
                && has(span, "output.players.online", "2")
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
        assert!(
            !spans
                .iter()
                .any(|span| span.name == "postgresql.query" && descendant(span, root))
        );
    }

    assert!(spans.iter().any(|root| {
        root.name == "game.authenticate"
            && has(root, "user.id", &account.to_string())
            && spans.iter().any(|span| {
                span.name == "mercury.login.response"
                    && span.parent_span_id == root.span_context.span_id()
                    && has(span, "output.response_code", "69")
                    && has(span, "output.delivery", "sent")
            })
    }));
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
                "INSERT INTO private_messages(conversation_id,sender_id,body) VALUES($1,$2,$3)",
            )
            && has(span, "db.system.name", "postgresql")
    }));
    assert!(
        spans
            .iter()
            .any(|span| span.name == "social.conversations.send_message"
                && descendant(span, message))
    );
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
    use wot_server::accounts::{AccountRepository, PostgresAccounts};
    let repository = PostgresAccounts::connect(url, 1).await.unwrap();
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
