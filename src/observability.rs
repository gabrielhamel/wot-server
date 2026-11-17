//! Shared diagnostic adapters for context propagation and PostgreSQL execution.

use std::{fmt::Display, future::Future};
use tracing::Instrument;

pub(crate) fn in_current_span<F, T>(work: F) -> impl FnOnce() -> T + Send + 'static
where
    F: FnOnce() -> T + Send + 'static,
{
    let span = tracing::Span::current();
    move || span.in_scope(work)
}

pub(crate) async fn database<T, E: Display>(
    context: &'static str,
    statement: &str,
    operation: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let span = tracing::info_span!(
        "postgresql.query",
        "wot.context" = context,
        "otel.kind" = "client",
        "db.system.name" = "postgresql",
        "db.operation.name" = statement.split_whitespace().next().unwrap_or("QUERY"),
        "db.query.text" = statement,
        "otel.status_code" = tracing::field::Empty,
        "error.type" = tracing::field::Empty,
    );
    async move {
        let result = operation.await;
        if let Err(error) = &result {
            tracing::Span::current().record("otel.status_code", "ERROR");
            tracing::Span::current().record("error.type", std::any::type_name::<E>());
            tracing::error!(%error, "Database operation failed");
        }
        result
    }
    .instrument(span)
    .await
}
