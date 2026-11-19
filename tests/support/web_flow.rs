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
    let debug_log = fs::File::create(fixture.directory.join("server.log")).unwrap();
    let configuration = serde_json::json!({
        "resource_dir": resources,
        "database_url": database_url,
        "login_bind": "127.0.0.1:0",
        "base_bind": "127.0.0.1:0",
        "web": {"bind": address.to_string(), "ui_dir": "web"},
        "admin": {},
        "store": {"origin": format!("http://{address}")},
    });
    fs::write(
        fixture.directory.join("server.json"),
        configuration.to_string(),
    )
    .unwrap();
    fixture.server = Some(
        Command::new(executable)
            .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
            .stdout(Stdio::null())
            .stderr(debug_log)
            .spawn()
            .unwrap(),
    );

    let deadline = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect(address).is_err() {
        if fixture
            .server
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_some()
            || Instant::now() >= deadline
        {
            panic!(
                "Web listener failed to start: {}",
                fs::read_to_string(fixture.directory.join("server.log")).unwrap()
            );
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
