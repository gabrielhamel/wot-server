use super::{configuration::LogLevel, telemetry::Telemetry};
use anyhow::{Context, Result};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{filter::Targets, prelude::*};

pub fn server_targets(level: LevelFilter) -> Targets {
    Targets::new().with_targets(
        [
            "wot_server_runtime",
            "wot_accounts",
            "wot_battles",
            "wot_platform",
            "wot_player_administration",
            "wot_player_sessions",
            "wot_shared_kernel",
            "wot_social",
        ]
        .map(|target| (target, level)),
    )
}

pub(crate) fn initialize(
    level: LogLevel,
    resource_dir: &std::path::Path,
) -> Result<Option<Telemetry>> {
    let (telemetry, export_error) = match Telemetry::from_environment(resource_dir) {
        Ok(telemetry) => (telemetry, None),
        Err(error) => (None, Some(error)),
    };
    let console = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .with_filter(
            server_targets(level.filter()).with_default(level.filter().min(LevelFilter::WARN)),
        );

    let export_filter = || server_targets(LevelFilter::INFO);
    let traces = telemetry.as_ref().map(|telemetry| {
        tracing_opentelemetry::layer()
            .with_tracer(telemetry.tracer())
            .with_filter(export_filter())
    });
    let logs = telemetry.as_ref().map(|telemetry| {
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(telemetry.logger())
            .with_filter(export_filter())
    });

    tracing_subscriber::registry()
        .with(console)
        .with(traces)
        .with(logs)
        .try_init()
        .context("Initialize logging")?;
    if let Some(error) = export_error {
        tracing::warn!(%error, "OpenTelemetry export disabled");
    }
    Ok(telemetry)
}
