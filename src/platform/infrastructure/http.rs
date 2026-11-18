use axum::{Router, http::HeaderMap};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinSet};

const MAX_CONNECTIONS: usize = 64;
const CONNECTION_LIFETIME: Duration = Duration::from_secs(20);

pub(crate) async fn serve(
    listener: TcpListener,
    router: Router,
    service_name: &'static str,
    mut stop: oneshot::Receiver<()>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = result {
                    tracing::error!(%error, service_name, "HTTP connection task failed");
                }
            }
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                match accepted {
                    Ok((stream, peer)) => {
                        let service = TowerToHyperService::new(router.clone().layer(axum::Extension(peer)));
                        connections.spawn(async move {
                            let connection = http1::Builder::new()
                                .timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(5))
                                .max_buf_size(32 * 1024)
                                .serve_connection(TokioIo::new(stream), service);
                            match tokio::time::timeout(CONNECTION_LIFETIME, connection).await {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => tracing::debug!(%peer, %error, service_name, "HTTP connection closed"),
                                Err(_) => tracing::debug!(%peer, service_name, "HTTP connection expired"),
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, service_name, "Accept HTTP connection failed");
                        tokio::select! {
                            _ = &mut stop => break,
                            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                        }
                    }
                }
            }
        }
    }
    drop(listener);
    connections.shutdown().await;
}

fn forwarded_client(
    peer: IpAddr,
    headers: &HeaderMap,
    trusted_proxies: &[IpAddr],
) -> Option<IpAddr> {
    if !trusted_proxies.contains(&peer) {
        return None;
    }
    let mut values = headers.get_all("x-forwarded-for").iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() || value.len() > 45 {
        return None;
    }
    value.trim().parse().ok()
}

pub(crate) fn record_client_address(
    peer: SocketAddr,
    headers: &HeaderMap,
    trusted_proxies: &[IpAddr],
) {
    let forwarded = forwarded_client(peer.ip(), headers, trusted_proxies);
    let span = tracing::Span::current();
    span.record("network.peer.address", peer.ip().to_string());
    span.record("network.peer.port", peer.port());
    span.record("client.address", forwarded.unwrap_or(peer.ip()).to_string());
    if forwarded.is_none() {
        span.record("client.port", peer.port());
    }
}

pub(crate) fn set_remote_trace_parent(span: &tracing::Span, headers: &HeaderMap) {
    use opentelemetry::{propagation::TextMapPropagator, trace::TraceContextExt};
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let mut values = headers.get_all("traceparent").iter();
    let Some(value) = values.next().and_then(|value| value.to_str().ok()) else {
        return;
    };
    if value.len() != 55 || values.next().is_some() {
        return;
    }
    let parent = TraceContextPropagator::new().extract(&std::collections::HashMap::from([(
        "traceparent".to_owned(),
        value.to_owned(),
    )]));
    let context = parent.span();
    if context.span_context().is_valid() && context.span_context().is_sampled() {
        let _ = span.set_parent(parent.clone());
    }
}
