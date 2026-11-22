//! Shared diagnostic adapters for context propagation and PostgreSQL execution.

use std::{fmt::Display, future::Future};
use tracing::Instrument;

mod database_metrics;
pub(crate) use database_metrics::configure_database_metrics;

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
    let operation_name = statement.split_whitespace().next().unwrap_or("QUERY");
    let span = tracing::info_span!(
        "postgresql.query",
        "ddd.bounded_context.name" = context,
        "otel.kind" = "client",
        "db.system.name" = "postgresql",
        "db.operation.name" = operation_name,
        "db.query.text" = statement,
        "otel.status_code" = tracing::field::Empty,
        "error.type" = tracing::field::Empty,
    );
    async move {
        let measurement = database_metrics::OperationMeasurement::start();
        let result = operation.await;
        if let Some(measurement) = measurement {
            measurement.record(context, operation_name, result.is_ok());
        }
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
