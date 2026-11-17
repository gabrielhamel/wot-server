use anyhow::{Context, Result};
use opentelemetry::{metrics::MeterProvider, trace::TracerProvider};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider,
};
use std::{env, time::Instant};

pub(super) struct Telemetry {
    logger: SdkLoggerProvider,
    tracer: SdkTracerProvider,
    meter: SdkMeterProvider,
}

impl Telemetry {
    pub(super) fn from_environment() -> Result<Option<Self>> {
        match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(endpoint) if !endpoint.is_empty() => Self::new().map(Some),
            Ok(_) | Err(env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error).context("Read OTEL_EXPORTER_OTLP_ENDPOINT"),
        }
    }

    fn new() -> Result<Self> {
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
            .unwrap_or_else(|| env!("CARGO_PKG_NAME").into());
        let resource = Resource::builder().with_service_name(name).build();
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
            .meter(env!("CARGO_PKG_NAME"))
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
        self.tracer.tracer(env!("CARGO_PKG_NAME"))
    }

    pub(super) fn logger(&self) -> &SdkLoggerProvider {
        &self.logger
    }

    pub(super) fn shutdown(self) {
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
