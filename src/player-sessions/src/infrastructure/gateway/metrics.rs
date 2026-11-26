use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
use std::time::Duration;
use wot_battles::contract::TICK;

pub(super) struct GatewayMetrics {
    online_players: Gauge<u64>,
    running_battles: Gauge<u64>,
    tick_duration: Histogram<f64>,
    tick_delay: Histogram<f64>,
    skipped_ticks: Counter<u64>,
    snapshot_age: Histogram<f64>,
    snapshot_queue_drops: Counter<u64>,
}

impl GatewayMetrics {
    pub(super) fn new(meter: &Meter) -> Self {
        let skipped_ticks = meter
            .u64_counter("wot.battle.tick.skipped")
            .with_unit("{tick}")
            .with_description("Scheduled battle ticks discarded by the battle worker")
            .build();
        skipped_ticks.add(0, &[]);

        let snapshot_queue_drops = meter
            .u64_counter("wot.battle.snapshot.queue_drops")
            .with_unit("{snapshot}")
            .with_description(
                "Arena position snapshots discarded because the battle output queue was full",
            )
            .build();
        snapshot_queue_drops.add(0, &[]);

        let seconds_histogram = |name, description| {
            meter
                .f64_histogram(name)
                .with_unit("s")
                .with_description(description)
                .with_boundaries(vec![
                    0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.15, 0.25, 0.5, 1.0,
                ])
                .build()
        };

        Self {
            online_players: meter
                .u64_gauge("wot.players.online")
                .with_unit("{player}")
                .with_description("Current authenticated game sessions")
                .build(),
            running_battles: meter
                .u64_gauge("wot.battles.running")
                .with_unit("{battle}")
                .with_description("Current activated arenas with participants")
                .build(),
            tick_duration: seconds_histogram(
                "wot.battle.tick.duration",
                "Time spent processing a shared battle tick",
            ),
            tick_delay: seconds_histogram(
                "wot.battle.tick.delay",
                "Time from a scheduled battle tick to the start of its processing",
            ),
            skipped_ticks,
            snapshot_age: seconds_histogram(
                "wot.battle.snapshot.age",
                "Time from arena snapshot enqueue to the last recipient's UDP send attempt",
            ),
            snapshot_queue_drops,
        }
    }

    pub(super) fn record_counts(&self, players: usize, battles: usize) {
        self.online_players.record(players as u64, &[]);
        self.running_battles.record(battles as u64, &[]);
    }

    pub(super) fn record_snapshot_age(&self, age: Duration) {
        self.snapshot_age.record(age.as_secs_f64(), &[]);
    }

    pub(super) fn record_snapshot_queue_drops(&self, drops: u64) {
        if drops > 0 {
            self.snapshot_queue_drops.add(drops, &[]);
        }
    }

    pub(super) fn record_battle_tick(&self, delay: Duration, duration: Duration) {
        self.tick_delay.record(delay.as_secs_f64(), &[]);
        self.tick_duration.record(duration.as_secs_f64(), &[]);
        let skipped = u64::try_from(delay.as_nanos() / TICK.as_nanos()).unwrap_or(u64::MAX);
        if skipped > 0 {
            self.skipped_ticks.add(skipped, &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::{
        InMemoryMetricExporter, SdkMeterProvider,
        data::{AggregatedMetrics, MetricData},
    };

    #[test]
    fn records_skipped_intervals_at_the_scheduler_boundary() {
        let exporter = InMemoryMetricExporter::default();
        let provider = SdkMeterProvider::builder()
            .with_periodic_exporter(exporter.clone())
            .build();
        let metrics = GatewayMetrics::new(&provider.meter("gateway-test"));
        for delay in [99, 100, 250] {
            metrics.record_battle_tick(Duration::from_millis(delay), Duration::from_millis(5));
        }

        provider.force_flush().unwrap();
        let exports = exporter.get_finished_metrics().unwrap();
        let skipped = exports
            .last()
            .unwrap()
            .scope_metrics()
            .flat_map(|scope| scope.metrics())
            .find(|metric| metric.name() == "wot.battle.tick.skipped")
            .unwrap();
        let AggregatedMetrics::U64(MetricData::Sum(sum)) = skipped.data() else {
            panic!("expected a counter");
        };
        assert!(sum.is_monotonic());
        assert_eq!(sum.data_points().next().unwrap().value(), 3);
    }
}
