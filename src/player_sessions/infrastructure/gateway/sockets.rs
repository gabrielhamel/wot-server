use crate::platform::mercury::net::socket::PacketSocket;
use anyhow::{Context, Result};
use std::{
    io,
    net::{SocketAddr, TcpListener},
};

const MAX_EPHEMERAL_PORT_RETRIES: u8 = 7;

pub(super) fn bind_base(address: SocketAddr) -> Result<(PacketSocket, TcpListener)> {
    bind_sockets(address, PacketSocket::bind)
}

#[tracing::instrument(
    name = "player_sessions.bind_base_sockets",
    skip_all,
    fields(
        ddd.bounded_context.name = "player_sessions",
        input.port = i64::from(address.port()),
        output.port = tracing::field::Empty,
        output.bind_retries.count = 0,
    ),
    err,
)]
fn bind_sockets(
    address: SocketAddr,
    mut bind_udp: impl FnMut(SocketAddr) -> io::Result<PacketSocket>,
) -> Result<(PacketSocket, TcpListener)> {
    let mut retries = 0;
    loop {
        let udp = bind_udp(address).context("Bind BaseApp UDP socket")?;
        let bound = udp.addr()?;
        match TcpListener::bind(bound) {
            Ok(tcp) => {
                tracing::Span::current().record("output.port", i64::from(bound.port()));
                return Ok((udp, tcp));
            }
            Err(error)
                if address.port() == 0
                    && error.kind() == io::ErrorKind::AddrInUse
                    && retries < MAX_EPHEMERAL_PORT_RETRIES =>
            {
                retries += 1;
                tracing::Span::current().record("output.bind_retries.count", i64::from(retries));
            }
            Err(error) => return Err(error).context("Bind native XMPP TCP socket"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{Status, TracerProvider};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
    use tracing_subscriber::prelude::*;

    #[test]
    fn retries_an_ephemeral_tcp_collision_and_keeps_fixed_port_failures() {
        let requested: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (occupied_udp, occupied) = bind_base(requested).unwrap();
        let occupied_address = occupied.local_addr().unwrap();
        let mut first_candidate = Some(occupied_udp);
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("socket-test")));

        let port = tracing::subscriber::with_default(subscriber, || {
            let _parent = tracing::info_span!("test.bind").entered();
            let (udp, tcp) = bind_sockets(requested, |address| {
                first_candidate
                    .take()
                    .map_or_else(|| PacketSocket::bind(address), Ok)
            })
            .unwrap();
            let bound = tcp.local_addr().unwrap();
            assert_eq!(udp.addr().unwrap(), bound);
            assert_ne!(bound, occupied_address);

            let error = bind_base(occupied_address).unwrap_err();
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                io::ErrorKind::AddrInUse
            );
            assert_eq!(error.to_string(), "Bind native XMPP TCP socket");
            bound.port()
        });
        let spans = exporter.get_finished_spans().unwrap();
        let root = spans.iter().find(|span| span.name == "test.bind").unwrap();
        let find = |port: u16| {
            spans
                .iter()
                .find(|span| {
                    span.name == "player_sessions.bind_base_sockets"
                        && span.parent_span_id == root.span_context.span_id()
                        && has_field(span, "input.port", port)
                })
                .unwrap_or_else(|| panic!("Missing bind span for port {port}: {spans:?}"))
        };
        let retry = find(0);
        assert!(has_field(retry, "output.port", port));
        assert!(retry.attributes.iter().any(|field| {
            field.key.as_str() == "output.bind_retries.count" && field.value.to_string() != "0"
        }));
        let failure = find(occupied_address.port());
        assert!(has_field(failure, "output.bind_retries.count", 0));
        assert!(matches!(failure.status, Status::Error { .. }));
    }

    fn has_field(span: &opentelemetry_sdk::trace::SpanData, key: &str, value: u16) -> bool {
        span.attributes.iter().any(|field| {
            field.key.as_str() == key && field.value == opentelemetry::Value::I64(i64::from(value))
        })
    }
}
