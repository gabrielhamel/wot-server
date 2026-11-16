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

pub(crate) async fn serve(listener: TcpListener, router: Router, mut stop: oneshot::Receiver<()>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = result {
                    tracing::error!(%error, "Admin HTTP connection task failed");
                }
            }
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                match accepted {
                    Ok((stream, peer)) => {
                        let service = TowerToHyperService::new(router.clone());
                        connections.spawn(async move {
                            let connection = http1::Builder::new()
                                .timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(5))
                                .max_buf_size(32 * 1024)
                                .serve_connection(TokioIo::new(stream), service);
                            match tokio::time::timeout(CONNECTION_LIFETIME, connection).await {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => tracing::debug!(%peer, %error, "Admin HTTP connection closed"),
                                Err(_) => tracing::debug!(%peer, "Admin HTTP connection expired"),
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, "Accept admin HTTP connection failed");
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
