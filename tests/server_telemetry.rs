use axum::{Router, body::Bytes, extract::State, http::Uri};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::mpsc};

struct ServerHome(PathBuf);

impl ServerHome {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = Self(std::env::temp_dir().join(format!(
            "wot-server-telemetry-{}-{suffix}",
            std::process::id()
        )));
        fs::create_dir(&home.0).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_wot-server"), home.executable()).unwrap();
        fs::write(
            home.0.join("server.json"),
            r#"{"resource_dir":"missing","log_file":"legacy/server.log"}"#,
        )
        .unwrap();
        home
    }

    fn executable(&self) -> PathBuf {
        self.0
            .join(format!("wot-server{}", std::env::consts::EXE_SUFFIX))
    }

    async fn run(&self, args: &[&str], endpoint: Option<String>) -> Output {
        let mut command = Command::new(self.executable());
        command.args(args).env("NO_PROXY", "127.0.0.1");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("OTEL_") {
                command.env_remove(name);
            }
        }
        if let Some(endpoint) = endpoint {
            command
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint)
                .env("OTEL_EXPORTER_OTLP_TIMEOUT", "2000");
        }
        let output = tokio::task::spawn_blocking(move || command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !output.status.success(),
            "startup requires a resource bundle"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Server stopped with an error"),
            "{output:?}"
        );
        assert!(
            !self.0.join("legacy").exists(),
            "opened the legacy log file"
        );
        output
    }
}

impl Drop for ServerHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn console_filter_is_independent_of_otlp_and_no_log_file_is_created() {
    let home = ServerHome::new();
    let info = home.run(&["--log-level", "info"], None).await;
    assert!(String::from_utf8_lossy(&info.stderr).contains("Starting server"));
    let invalid = home.run(&[], Some("invalid endpoint".into())).await;
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(stderr.contains("OpenTelemetry export disabled"), "{stderr}");
    assert!(!stderr.contains("Starting server"), "{stderr}");

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (send, mut receive) = mpsc::unbounded_channel();
    let app =
        Router::new()
            .fallback(
                |State(send): State<mpsc::UnboundedSender<(String, Bytes)>>,
                 uri: Uri,
                 bytes: Bytes| async move {
                    let _ = send.send((uri.path().into(), bytes));
                },
            )
            .with_state(send);
    let collector = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let exported = home.run(&[], Some(endpoint)).await;
    assert!(!String::from_utf8_lossy(&exported.stderr).contains("Starting server"));

    collector.abort();
    let requests: Vec<_> = std::iter::from_fn(|| receive.try_recv().ok()).collect();
    for (path, marker) in [
        ("/v1/logs", &b"Starting server"[..]),
        ("/v1/traces", &b"server.load_resources"[..]),
        ("/v1/metrics", &b"wot.server.uptime"[..]),
    ] {
        assert!(
            requests.iter().any(|(received_path, payload)| {
                received_path == path && payload.windows(marker.len()).any(|bytes| bytes == marker)
            }),
            "missing {path} export"
        );
    }
}
