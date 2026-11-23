use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePrivateKey, LineEnding},
    rand_core::OsRng,
};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct WebFixture {
    directory: PathBuf,
    server: Option<Child>,
}

impl Drop for WebFixture {
    fn drop(&mut self) {
        if let Some(server) = &mut self.server {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
#[ignore = "Requires WG_TEST_DATABASE_URL and WG_RESOURCE_DIR"]
fn admin_and_shop_share_one_listener_with_separate_authorization() {
    let resources = fs::canonicalize(std::env::var_os("WG_RESOURCE_DIR").unwrap()).unwrap();
    let database_url = std::env::var("WG_TEST_DATABASE_URL").unwrap();
    let suffix = std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos();
    let mut fixture = WebFixture {
        directory: std::env::temp_dir().join(format!("wot-web-{}-{suffix}", std::process::id())),
        server: None,
    };
    let executable = fixture
        .directory
        .join(format!("wot-server{}", std::env::consts::EXE_SUFFIX));
    fs::create_dir_all(fixture.directory.join("web/assets")).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_wot-server"), &executable).unwrap();
    fs::write(
        fixture.directory.join("web/index.html"),
        "<main>Shared UI entry</main>",
    )
    .unwrap();
    fs::write(fixture.directory.join("web/assets/test.js"), "export {};").unwrap();
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let admin_cookie = "wot_admin_session=not-a-session";
    let key = RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
    let key_path = fixture.directory.join("web-test-login.pem");
    let log_path = fixture.directory.join("server.log");
    fs::write(
        &key_path,
        key.to_pkcs8_pem(LineEnding::LF).unwrap().as_bytes(),
    )
    .unwrap();
    let debug_log = fs::File::create(&log_path).unwrap();
    fixture.server = Some(
        Command::new(executable)
            .current_dir(&fixture.directory)
            .arg("--resource-dir")
            .arg(resources)
            .arg("--private-key")
            .arg(key_path)
            .args(["--login-bind", "127.0.0.1:0", "--base-bind", "127.0.0.1:0"])
            .args(["--web-bind", &address.to_string(), "--web-ui-dir", "web"])
            .args([
                "--store-origin",
                "http://192.168.1.3:8080",
                "--log-level",
                "info",
            ])
            .env("WOT_DATABASE_URL", database_url)
            .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
            .stdout(Stdio::null())
            .stderr(debug_log)
            .spawn()
            .unwrap(),
    );

    let started = Instant::now();
    loop {
        let status = fixture.server.as_mut().unwrap().try_wait().unwrap();
        if status.is_some() || started.elapsed() >= Duration::from_secs(60) {
            let log = fs::read_to_string(&log_path).unwrap();
            panic!(
                "Web listener at {address} failed to start after {:.1?} (process status {status:?})\n{log}",
                started.elapsed()
            );
        }
        if TcpStream::connect(address).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for route in ["/dashboard", "/dashboard/", "/shop", "/shop/"] {
        let page = get(address, route, None);
        assert!(page.starts_with("HTTP/1.1 200"), "{route}: {page}");
        assert!(page.contains("Shared UI entry"));
        assert!(page.contains("cache-control: no-store"));
    }
    let shop = get(address, "/shop/", None);
    assert!(shop.contains("script-src 'self';"));
    assert!(shop.contains("style-src 'self' 'unsafe-inline'; font-src 'self';"));
    assert!(!shop.contains("script-src 'self' 'unsafe-inline'"));
    assert!(shop.contains("form-action 'none'"));
    for route in ["/dashboard", "/dashboard/"] {
        let dashboard = get(address, route, None);
        assert!(dashboard.contains("form-action 'self'"));
    }
    for route in ["/admin", "/admin/"] {
        assert!(get(address, route, None).starts_with("HTTP/1.1 404"));
    }
    let root = get(address, "/", None);
    assert!(root.starts_with("HTTP/1.1 307") && root.contains("location: /dashboard/"));
    assert!(get(address, "/assets/test.js", None).starts_with("HTTP/1.1 200"));
    assert!(get(address, "/api/admin/session", None).starts_with("HTTP/1.1 200"));
    assert!(get(address, "/api/admin/players", None).starts_with("HTTP/1.1 401"));
    assert!(get(address, "/api/admin/me/shop/catalog", None).starts_with("HTTP/1.1 401"));
    assert!(get(address, "/api/shop/catalog", Some(admin_cookie)).starts_with("HTTP/1.1 401"));
    assert!(get(address, "/api/shop/missing", None).starts_with("HTTP/1.1 404"));
}

fn get(address: SocketAddr, path: &str, cookie: Option<&str>) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n"
    )
    .unwrap();
    if let Some(cookie) = cookie {
        write!(stream, "Cookie: {cookie}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
