use std::time::{Duration, Instant};

use opentelemetry::{
    KeyValue,
    metrics::{Counter, Histogram, Meter},
};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const REJECT_BURST_SIZE: u64 = 8;

#[derive(Debug, Default)]
struct DiagnosticWindow {
    started: Option<Instant>,
    count: u64,
}

impl DiagnosticWindow {
    fn sample(&mut self, now: Instant, threshold: u64) -> bool {
        if self
            .started
            .is_none_or(|start| now.saturating_duration_since(start) >= SAMPLE_INTERVAL)
        {
            self.started = Some(now);
            self.count = 0;
        }
        self.count = self.count.saturating_add(1);
        self.count == threshold
    }
}

#[derive(Debug)]
pub(super) struct TransportTelemetry {
    listener: &'static str,
    pub(super) sequence_drops: Counter<u64>,
    ack_rtt: Histogram<f64>,
    retransmissions: Counter<u64>,
    retry_exhaustion: Counter<u64>,
    queue_age: Histogram<f64>,
    rejections: Counter<u64>,
    reject_windows: [DiagnosticWindow; 2],
    exhaustion_windows: [DiagnosticWindow; 2],
}

impl TransportTelemetry {
    pub(super) fn new(meter: &Meter, listener: &'static str) -> Self {
        let counter = |name, unit| meter.u64_counter(name).with_unit(unit).build();
        let histogram = |name| {
            meter
                .f64_histogram(name)
                .with_unit("s")
                .with_boundaries(vec![
                    0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1, 0.2, 0.4, 0.75, 1.0, 2.0, 5.0,
                    10.0, 20.0,
                ])
                .build()
        };
        let sequence_drops = meter
            .u64_counter("wot.mercury.reliable_sequence.drops")
            .with_unit("{packet}")
            .with_description("Mercury packets dropped for a mismatched last reliable sequence")
            .build();
        for direction in ["behind", "ahead"] {
            sequence_drops.add(0, &[KeyValue::new("direction", direction)]);
        }
        let telemetry = Self {
            listener,
            sequence_drops,
            ack_rtt: histogram("wot.mercury.ack.rtt"),
            retransmissions: counter("wot.mercury.retransmissions", "{packet}"),
            retry_exhaustion: counter("wot.mercury.retry.exhaustion", "{packet}"),
            queue_age: histogram("wot.mercury.outbox.age"),
            rejections: counter("wot.mercury.packet.rejections", "{packet}"),
            reject_windows: Default::default(),
            exhaustion_windows: Default::default(),
        };
        if listener == "base" {
            for outcome in ["sent", "socket_error"] {
                telemetry.retransmission(0, outcome);
            }
            telemetry
                .retry_exhaustion
                .add(0, &telemetry.labels(Some(0)));
            telemetry.retry_exhaustion.add(0, &telemetry.labels(None));
        }
        telemetry
    }

    fn labels(&self, session: Option<i64>) -> [KeyValue; 2] {
        [
            KeyValue::new("listener", self.listener),
            KeyValue::new("session.stage", session_stage(session)),
        ]
    }

    pub(super) fn acknowledge(&self, rtt: Duration, kind: &'static str) {
        self.ack_rtt
            .record(rtt.as_secs_f64(), &[KeyValue::new("ack.kind", kind)]);
    }

    pub(super) fn retransmission(&self, count: u64, outcome: &'static str) {
        self.retransmissions
            .add(count, &[KeyValue::new("outcome", outcome)]);
    }

    pub(super) fn flush(&self, age: Duration, outcome: &'static str) {
        self.queue_age
            .record(age.as_secs_f64(), &[KeyValue::new("outcome", outcome)]);
    }

    pub(super) fn reject(
        &mut self,
        now: Instant,
        session: Option<i64>,
        reason: &'static str,
        bytes: usize,
    ) {
        let [listener, stage] = self.labels(session);
        self.rejections
            .add(1, &[listener, stage, KeyValue::new("reason", reason)]);
        if !self.reject_windows[usize::from(session.is_some())].sample(now, REJECT_BURST_SIZE) {
            return;
        }
        let span = tracing::info_span!(
            parent: None, "mercury.packet_reject_burst",
            ddd.bounded_context.name = "player_sessions",
            mercury.listener = self.listener,
            mercury.session.stage = session_stage(session),
            mercury.session.id = session,
            input.bytes = bytes as i64,
            output.reason = reason,
            output.rejections.count = REJECT_BURST_SIZE as i64,
            output.outcome = "rejected",
            otel.status_code = "ERROR",
        );
        span.in_scope(|| tracing::warn!("Repeated Mercury packet rejections"));
    }

    pub(super) fn exhausted(
        &mut self,
        now: Instant,
        session: Option<i64>,
        sequence: u32,
        attempts: u32,
        age: Duration,
    ) {
        self.retry_exhaustion.add(1, &self.labels(session));
        if !self.exhaustion_windows[usize::from(session.is_some())].sample(now, 1) {
            return;
        }
        let span = tracing::info_span!(
            parent: None, "mercury.retry_exhausted",
            ddd.bounded_context.name = "player_sessions",
            mercury.listener = self.listener,
            mercury.session.stage = session_stage(session),
            mercury.session.id = session,
            input.packet.sequence = i64::from(sequence),
            output.retries.scheduled_count = i64::from(attempts),
            output.packet.age_ms = i64::try_from(age.as_millis()).unwrap_or(i64::MAX),
            output.outcome = "retry_exhausted",
            otel.status_code = "ERROR",
        );
        span.in_scope(|| tracing::warn!("Mercury reliable packet acknowledgement timed out"));
    }
}

fn session_stage(session: Option<i64>) -> &'static str {
    if session.is_some() {
        "established"
    } else {
        "unestablished"
    }
}
