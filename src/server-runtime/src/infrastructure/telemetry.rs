use anyhow::{Context, Result};
use opentelemetry::{KeyValue, metrics::MeterProvider, trace::TracerProvider};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider,
};
use std::{env, path::Path, time::Instant};

const SERVICE_NAME: &str = "wot-server";

pub(crate) struct Telemetry {
    logger: SdkLoggerProvider,
    tracer: SdkTracerProvider,
    meter: SdkMeterProvider,
}

impl Telemetry {
    pub(super) fn from_environment(resource_dir: &Path) -> Result<Option<Self>> {
        match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(endpoint) if !endpoint.is_empty() => Self::new(resource_dir).map(Some),
            Ok(_) | Err(env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error).context("Read OTEL_EXPORTER_OTLP_ENDPOINT"),
        }
    }

    fn new(resource_dir: &Path) -> Result<Self> {
        // Let the SDK resolve the common endpoint, per-signal paths, headers, and timeouts.
        let logs = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .build()
            .context("Configure OTLP logs")?;
        let traces = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .context("Configure OTLP traces")?;
        let metrics = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .build()
            .context("Configure OTLP metrics")?;

        let name = env::var("OTEL_SERVICE_NAME")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| SERVICE_NAME.into());
        let version = option_env!("WOT_SERVER_VERSION")
            .filter(|value| !value.is_empty())
            .unwrap_or(env!("CARGO_PKG_VERSION"));
        let mut resource = Resource::builder()
            .with_service_name(name)
            .with_attribute(KeyValue::new("service.version", version));
        if let Some(version) = option_env!("WOT_GAME_VERSION").filter(|value| !value.is_empty()) {
            resource = resource.with_attribute(KeyValue::new("wot.game.build_version", version));
        }
        if let Some(version) = wot_resources::ResourceBundle::open(resource_dir)
            .ok()
            .and_then(|bundle| bundle.game_version().map(str::to_owned))
        {
            resource = resource.with_attribute(KeyValue::new("wot.game.version", version));
        }
        let resource = resource.build();
        let logger = SdkLoggerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(logs)
            .build();
        let tracer = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(traces)
            .build();
        let meter = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_periodic_exporter(metrics)
            .build();

        let started = Instant::now();
        meter
            .meter(SERVICE_NAME)
            .f64_observable_gauge("wot.server.uptime")
            .with_unit("s")
            .with_description("Seconds since the server started")
            .with_callback(move |gauge| gauge.observe(started.elapsed().as_secs_f64(), &[]))
            .build();

        Ok(Self {
            logger,
            tracer,
            meter,
        })
    }

    pub(super) fn tracer(&self) -> opentelemetry_sdk::trace::SdkTracer {
        self.tracer.tracer(SERVICE_NAME)
    }

    pub(super) fn logger(&self) -> &SdkLoggerProvider {
        &self.logger
    }

    pub(crate) fn meter(&self) -> opentelemetry::metrics::Meter {
        self.meter.meter(SERVICE_NAME)
    }

    pub(crate) fn shutdown(self) {
        for (signal, result) in [
            ("logs", self.logger.shutdown()),
            ("traces", self.tracer.shutdown()),
            ("metrics", self.meter.shutdown()),
        ] {
            if let Err(error) = result {
                tracing::warn!(signal, %error, "OpenTelemetry shutdown failed");
            }
        }
    }
}
