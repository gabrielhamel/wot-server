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
