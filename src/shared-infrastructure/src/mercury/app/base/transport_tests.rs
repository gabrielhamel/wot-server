use super::*;

#[test]
fn udp_transport_exports_ack_timing_and_sampled_rejections() {
    use blowfish::cipher::KeyInit;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry::{Value, metrics::MeterProvider};
    use opentelemetry_sdk::{
        metrics::{
            InMemoryMetricExporter, SdkMeterProvider,
            data::{AggregatedMetrics, MetricData},
        },
        trace::{InMemorySpanExporter, SdkTracerProvider},
    };
    use tracing_subscriber::prelude::*;

    use crate::mercury::net::packet::{Packet, PacketConfig};

    let metrics = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_periodic_exporter(metrics.clone())
        .build();
    let traces = InMemorySpanExporter::default();
    let tracers = SdkTracerProvider::builder()
        .with_simple_exporter(traces.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracers.tracer("mercury-test")));
    let _subscriber = tracing::subscriber::set_default(subscriber);

    let localhost = "127.0.0.1:0".parse().unwrap();
    let mut app = App::new(localhost, Script::default()).unwrap();
    app.configure_metrics(&meters.meter("mercury-test"));
    let mut peer = PacketSocket::bind(localhost).unwrap();
    peer.set_recv_timeout(Some(Duration::from_secs(1))).unwrap();
    let addr = peer.addr().unwrap();
    let server_addr = app.addr().unwrap();
    let cipher = Arc::new(Blowfish::new_from_slice(&[42; 56]).unwrap());
    peer.set_encryption(server_addr, cipher.clone());
    app.pending_clients.insert(addr, (1, Instant::now()));
    let client = app.answer_login_success(addr, cipher).unwrap();
    peer.recv().unwrap();

    for (cumulative, retransmit) in [(false, false), (true, false), (false, true)] {
        app.send_simple(client, client::element::TickSync { tick: 1 })
            .unwrap();
        assert!(matches!(app.poll_until(Instant::now()), Event::Timeout));
        let (sent, _) = peer.recv().unwrap();
        let mut config = PacketConfig::new();
        sent.read_config(&mut config).unwrap();
        let sequence = config.sequence_num();
        if retransmit {
            app.retry_reliable(Duration::ZERO, RESEND_GIVE_UP).unwrap();
            let (resent, _) = peer.recv().unwrap();
            assert_eq!(resent.slice(), sent.slice());
        }

        let mut ack = Packet::new();
        let mut config = PacketConfig::new();
        config.set_on_channel(true);
        if cumulative {
            config.set_cumulative_ack(sequence + 1);
        }
        config.single_acks_mut().push_back(sequence);
        ack.write_config(&mut config);
        peer.send(&ack, server_addr).unwrap();
        assert!(matches!(app.poll_until(Instant::now()), Event::Timeout));
    }

    app.send_simple(client, client::element::TickSync { tick: 2 })
        .unwrap();
    assert!(matches!(app.poll_until(Instant::now()), Event::Timeout));
    peer.recv().unwrap();
    app.retry_reliable(Duration::ZERO, RESEND_GIVE_UP).unwrap();
    peer.recv().unwrap();
    app.retry_reliable(RESEND_TIMEOUT, Duration::ZERO).unwrap();

    let mut invalid = Packet::new();
    invalid.write_flags(0x8000);
    for _ in 0..10 {
        peer.send(&invalid, server_addr).unwrap();
        assert!(matches!(app.poll_until(Instant::now()), Event::Timeout));
    }
    let unknown = std::net::UdpSocket::bind(localhost).unwrap();
    unknown.send_to(&[0], server_addr).unwrap();
    assert!(matches!(app.poll_until(Instant::now()), Event::IoError(_)));

    meters.force_flush().unwrap();
    let exports = metrics.get_finished_metrics().unwrap();
    let metrics: HashMap<_, _> = exports
        .last()
        .unwrap()
        .scope_metrics()
        .flat_map(|scope| scope.metrics())
        .map(|metric| (metric.name(), metric.data()))
        .collect();
    for (name, expected) in [
        (
            "wot.mercury.ack.rtt",
            HashMap::from([("single", 1), ("cumulative", 1)]),
        ),
        ("wot.mercury.outbox.age", HashMap::from([("sent", 4)])),
    ] {
        let AggregatedMetrics::F64(MetricData::Histogram(histogram)) = metrics[name] else {
            panic!("expected histogram for {name}");
        };
        let counts = histogram
            .data_points()
            .map(|point| {
                let label = point.attributes().next().unwrap();
                assert_eq!(point.attributes().count(), 1);
                (label.value.as_str().into_owned(), point.count())
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            counts,
            expected
                .into_iter()
                .map(|(label, count)| (label.to_owned(), count))
                .collect()
        );
    }

    let AggregatedMetrics::U64(MetricData::Sum(sum)) = metrics["wot.mercury.packet.rejections"]
    else {
        panic!("expected rejection counter");
    };
    let counts: HashMap<_, _> = sum
        .data_points()
        .map(|point| {
            let labels: HashMap<_, _> = point
                .attributes()
                .map(|kv| (kv.key.as_str(), kv.value.as_str()))
                .collect();
            assert_eq!(labels.len(), 3);
            assert_eq!(labels["listener"], "base");
            (
                (labels["session.stage"].clone(), labels["reason"].clone()),
                point.value(),
            )
        })
        .collect();
    assert_eq!(
        counts,
        HashMap::from([
            (("established".into(), "unknown_flags".into()), 10),
            (("unestablished".into(), "datagram_size".into()), 1),
        ])
    );

    let AggregatedMetrics::U64(MetricData::Sum(retransmits)) =
        metrics["wot.mercury.retransmissions"]
    else {
        panic!("expected retransmissions counter");
    };
    let sent = retransmits
        .data_points()
        .find(|point| {
            point
                .attributes()
                .any(|kv| kv.key.as_str() == "outcome" && kv.value.as_str() == "sent")
        })
        .unwrap();
    assert_eq!(sent.value(), 2);
    let AggregatedMetrics::U64(MetricData::Sum(exhaustions)) =
        metrics["wot.mercury.retry.exhaustion"]
    else {
        panic!("expected exhaustion counter");
    };
    let established = exhaustions
        .data_points()
        .find(|point| {
            point
                .attributes()
                .any(|kv| kv.key.as_str() == "session.stage" && kv.value.as_str() == "established")
        })
        .unwrap();
    assert_eq!(established.value(), 1);

    let spans = traces.get_finished_spans().unwrap();
    let [exhaustion, burst] = spans.as_slice() else {
        panic!("expected two sampled spans, got {spans:?}");
    };
    assert_eq!(exhaustion.name, "mercury.retry_exhausted");
    assert_eq!(
        exhaustion.parent_span_id,
        opentelemetry::trace::SpanId::INVALID
    );
    assert!(matches!(
        exhaustion.status,
        opentelemetry::trace::Status::Error { .. }
    ));
    assert_eq!(exhaustion.events.len(), 1);
    let attrs: HashMap<_, _> = exhaustion
        .attributes
        .iter()
        .map(|kv| (kv.key.as_str(), &kv.value))
        .collect();
    assert_eq!(
        attrs["mercury.session.id"],
        &Value::I64(client.slot.diagnostic_id())
    );
    assert_eq!(attrs["output.retries.scheduled_count"], &Value::I64(1));
    assert_eq!(burst.name, "mercury.packet_reject_burst");
    assert_eq!(burst.parent_span_id, opentelemetry::trace::SpanId::INVALID);
    assert!(matches!(
        burst.status,
        opentelemetry::trace::Status::Error { .. }
    ));
    assert_eq!(burst.events.len(), 1);
    let attrs: HashMap<_, _> = burst
        .attributes
        .iter()
        .map(|kv| (kv.key.as_str(), &kv.value))
        .collect();
    assert_eq!(
        attrs["mercury.session.id"],
        &Value::I64(client.slot.diagnostic_id())
    );
    assert_eq!(attrs["output.rejections.count"], &Value::I64(8));
    assert_eq!(attrs["output.reason"].as_str(), "unknown_flags");
}
