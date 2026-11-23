use axum::{Router, body::Bytes, extract::State, http::Uri};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
        trace::v1::ExportTraceServiceRequest,
    },
    common::v1::any_value::Value,
    resource::v1::Resource,
};
use prost::Message;
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
            r#"{"resource_dir":"resources","database_url":"invalid"}"#,
        )
        .unwrap();
        fs::create_dir(home.0.join("resources")).unwrap();
        fs::write(
            home.0.join("resources/manifest.json"),
            r#"{"format_version":1,"component_client_classes":[],"game_version":"synthetic-client-build"}"#,
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
            "fixture intentionally fails startup"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Server stopped with an error"),
            "{output:?}"
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
async fn console_filter_is_independent_of_otlp() {
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
    for (path, payload) in &requests {
        match path.as_str() {
            "/v1/logs" => {
                for logs in ExportLogsServiceRequest::decode(payload.clone())
                    .unwrap()
                    .resource_logs
                {
                    assert_resource(logs.resource);
                }
            }
            "/v1/traces" => {
                for traces in ExportTraceServiceRequest::decode(payload.clone())
                    .unwrap()
                    .resource_spans
                {
                    assert_resource(traces.resource);
                }
            }
            "/v1/metrics" => {
                for metrics in ExportMetricsServiceRequest::decode(payload.clone())
                    .unwrap()
                    .resource_metrics
                {
                    assert_resource(metrics.resource);
                }
            }
            _ => panic!("unexpected export {path}"),
        }
    }
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

#[test]
fn resource_bundle_preserves_source_version_and_reads_legacy_manifests() {
    let home = ServerHome::new();
    let root = home.0.join("resources");
    assert_eq!(
        wot_resources::ResourceBundle::open(&root)
            .unwrap()
            .game_version(),
        Some("synthetic-client-build")
    );
    fs::write(
        root.join("manifest.json"),
        r#"{"format_version":1,"component_client_classes":[]}"#,
    )
    .unwrap();
    assert_eq!(
        wot_resources::ResourceBundle::open(&root)
            .unwrap()
            .game_version(),
        None
    );
    let output = home.0.join("new-bundle");
    fs::create_dir(&output).unwrap();
    wot_resources::ResourceBundle::write_manifest_with_version(
        &output,
        Default::default(),
        Some("source-version".into()),
    )
    .unwrap();
    assert_eq!(
        wot_resources::ResourceBundle::open(&output)
            .unwrap()
            .game_version(),
        Some("source-version")
    );
}

fn assert_resource(resource: Option<Resource>) {
    let attributes = resource.unwrap().attributes;
    if let Some(version) = option_env!("WOT_GAME_VERSION").filter(|value| !value.is_empty()) {
        assert!(attributes.iter().any(|attribute| {
            attribute.key == "wot.game.build_version"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&Value::StringValue(version.into()))
        }));
    }
    for (key, expected) in [
        ("service.name", "wot-server"),
        (
            "service.version",
            option_env!("WOT_SERVER_VERSION")
                .filter(|version| !version.is_empty())
                .unwrap_or(env!("CARGO_PKG_VERSION")),
        ),
        ("wot.game.version", "synthetic-client-build"),
    ] {
        assert!(
            attributes.iter().any(|attribute| {
                attribute.key == key
                    && attribute
                        .value
                        .as_ref()
                        .and_then(|value| value.value.as_ref())
                        == Some(&Value::StringValue(expected.into()))
            }),
            "missing {key}={expected}"
        );
    }
}
