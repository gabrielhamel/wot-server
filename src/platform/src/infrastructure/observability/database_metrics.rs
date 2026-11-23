use opentelemetry::{
    KeyValue,
    metrics::{Histogram, Meter},
};
use sqlx::PgPool;
use std::{sync::OnceLock, time::Instant};

static OPERATION_DURATION: OnceLock<Histogram<f64>> = OnceLock::new();

pub fn configure_database_metrics(meter: &Meter, pool: &PgPool) {
    OPERATION_DURATION.get_or_init(|| {
        meter
            .f64_histogram("wot.db.operation.duration")
            .with_unit("s")
            .with_description("Elapsed time of a PostgreSQL operation")
            .with_boundaries(vec![
                0.0005, 0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ])
            .build()
    });

    let capacity = u64::from(pool.options().get_max_connections());
    meter
        .u64_observable_gauge("wot.db.pool.capacity")
        .with_unit("{connection}")
        .with_description("Configured maximum connections in the PostgreSQL pool")
        .with_callback(move |gauge| gauge.observe(capacity, &[]))
        .build();

    let pool = pool.clone();
    let idle_attributes = [KeyValue::new("state", "idle")];
    let non_idle_attributes = [KeyValue::new("state", "non_idle")];
    meter
        .u64_observable_gauge("wot.db.pool.connections")
        .with_unit("{connection}")
        .with_description("Current PostgreSQL pool connections by state")
        .with_callback(move |gauge| {
            let size = u64::from(pool.size());
            let idle = (pool.num_idle() as u64).min(size);
            gauge.observe(idle, &idle_attributes);
            gauge.observe(size - idle, &non_idle_attributes);
        })
        .build();
}

pub(super) struct OperationMeasurement {
    histogram: &'static Histogram<f64>,
    started: Instant,
}

impl OperationMeasurement {
    pub(super) fn start() -> Option<Self> {
        OPERATION_DURATION.get().map(|histogram| Self {
            histogram,
            started: Instant::now(),
        })
    }

    pub(super) fn record(self, context: &'static str, operation: &str, succeeded: bool) {
        let duration = self.started.elapsed().as_secs_f64();
        let operation = match operation {
            "SELECT" => "SELECT",
            "INSERT" => "INSERT",
            "UPDATE" => "UPDATE",
            "DELETE" => "DELETE",
            "WITH" => "WITH",
            "SET" => "SET",
            "BEGIN" => "BEGIN",
            "COMMIT" => "COMMIT",
            "ROLLBACK" => "ROLLBACK",
            _ => "OTHER",
        };
        self.histogram.record(
            duration,
            &[
                KeyValue::new("ddd.bounded_context.name", context),
                KeyValue::new("db.operation.name", operation),
                KeyValue::new("outcome", if succeeded { "success" } else { "error" }),
            ],
        );
    }
}
