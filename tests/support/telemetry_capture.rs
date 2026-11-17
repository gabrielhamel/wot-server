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

pub fn assert_player_spans(account: i64, login: &str) {
    let spans = exporter().get_finished_spans().unwrap();
    let has = |span: &opentelemetry_sdk::trace::SpanData, key: &str, expected: &str| {
        span.attributes.iter().any(|attribute| {
            attribute.key.as_str() == key && attribute.value.to_string() == expected
        })
    };
    assert!(spans.iter().any(|span| {
        span.name == "game.authenticate"
            && has(span, "user.id", &account.to_string())
            && has(span, "user.name", login)
            && has(span, "user.email", login)
            && has(span, "outcome", "authenticated")
            && has(span, "wot.client.language", "en")
            && has(span, "wot.client.protocol_version", "0")
    }));
    assert!(spans.iter().any(|span| {
        span.name == "game.command"
            && has(span, "user.id", &account.to_string())
            && span
                .attributes
                .iter()
                .any(|attribute| attribute.key.as_str() == "rpc.method")
    }));
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

pub fn assert_layered_traces(account: i64) {
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
    let command = spans
        .iter()
        .find(|root| {
            root.name == "game.command"
                && has(root, "user.id", &account.to_string())
                && spans.iter().any(|span| {
                    span.name == "access.infrastructure.gateway.progression_response"
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
    for layer in ["application", "domain", "infrastructure"] {
        assert!(
            spans
                .iter()
                .any(|span| has(span, "code.layer", layer) && descendant(span, command)),
            "missing {layer} under command"
        );
    }
    let message = spans.iter().find(|root| {
        root.name == "xmpp.request"
            && spans.iter().any(|span| span.name == "social.application.send" && descendant(span, root))
            && spans.iter().any(|span| span.name == "xmpp.response" && descendant(span, root))
    }).expect("XMPP message must retain its parent through database persistence and outgoing delivery");
    assert_eq!(
        message.parent_span_id,
        opentelemetry::trace::SpanId::INVALID
    );
    assert!(spans.iter().any(|span| {
        span.name == "postgresql.query"
            && descendant(span, message)
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
            .any(|span| span.name == "social.infrastructure.send" && descendant(span, message))
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
