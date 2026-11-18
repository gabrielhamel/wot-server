use axum::Router;
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::time::Duration;
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
