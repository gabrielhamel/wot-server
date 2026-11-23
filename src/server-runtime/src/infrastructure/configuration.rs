use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    io::Write,
    net::SocketAddrV4,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    #[default]
    Debug,
    Trace,
}

impl LogLevel {
    pub(super) fn filter(self) -> tracing::level_filters::LevelFilter {
        match self {
            Self::Error => tracing::level_filters::LevelFilter::ERROR,
            Self::Warn => tracing::level_filters::LevelFilter::WARN,
            Self::Info => tracing::level_filters::LevelFilter::INFO,
            Self::Debug => tracing::level_filters::LevelFilter::DEBUG,
            Self::Trace => tracing::level_filters::LevelFilter::TRACE,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AdminConfiguration {
    bind: Option<SocketAddrV4>,
    pub secure_cookies: bool,
    pub dashboard_dir: Option<PathBuf>,
}

fn default_admin() -> Option<AdminConfiguration> {
    Some(AdminConfiguration::default())
}

fn default_web_bind() -> SocketAddrV4 {
    SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 8080)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebConfiguration {
    #[serde(default = "default_web_bind")]
    pub bind: SocketAddrV4,
    pub ui_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoreConfiguration {
    bind: Option<SocketAddrV4>,
    #[serde(default = "default_store_origin")]
    pub origin: String,
}

fn default_store_origin() -> String {
    "http://127.0.0.1:8080".into()
}

fn default_store() -> Option<StoreConfiguration> {
    Some(StoreConfiguration {
        bind: None,
        origin: default_store_origin(),
    })
}

impl StoreConfiguration {
    fn validate(&mut self) -> Result<()> {
        let uri = self
            .origin
            .parse::<axum::http::Uri>()
            .context("Invalid Store origin")?;
        let host = uri.host().context("Store origin requires a host")?;
        let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
        ensure!(
            uri.scheme_str() == Some("https") || (uri.scheme_str() == Some("http") && loopback),
            "Store origin requires HTTPS or loopback HTTP"
        );
        ensure!(
            uri.path() == "/" && uri.query().is_none() && !self.origin.contains(['@', '#']),
            "Store origin must contain only a scheme, host and optional port"
        );
        self.origin
            .truncate(self.origin.trim_end_matches('/').len());
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    pub resource_dir: PathBuf,
    pub database_url: String,
    #[serde(default = "default_login_bind")]
    pub login_bind: SocketAddrV4,
    #[serde(default = "default_base_bind")]
    pub base_bind: SocketAddrV4,
    pub advertised_base: Option<SocketAddrV4>,
    pub private_key: PathBuf,
    #[serde(default = "default_db_connections")]
    pub db_connections: u32,
    #[serde(default = "default_max_clients")]
    pub max_clients: usize,
    #[serde(default = "default_max_pending")]
    pub max_pending: usize,
    #[serde(default = "default_training_room_visible")]
    pub training_room_visible: bool,
    #[serde(default = "default_periphery_id")]
    pub periphery_id: u32,
    #[serde(default)]
    pub log_level: LogLevel,
    pub web: Option<WebConfiguration>,
    #[serde(default = "default_admin")]
    pub admin: Option<AdminConfiguration>,
    #[serde(default = "default_store")]
    pub store: Option<StoreConfiguration>,
}

fn default_login_bind() -> SocketAddrV4 {
    SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 20016)
}

fn default_base_bind() -> SocketAddrV4 {
    SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 20017)
}

fn default_db_connections() -> u32 {
    16
}

fn default_max_clients() -> usize {
    256
}

fn default_max_pending() -> usize {
    32
}

fn default_training_room_visible() -> bool {
    true
}

fn default_periphery_id() -> u32 {
    250
}

const INITIAL_CONFIGURATION: &str = r#"{
  "resource_dir": "resources",
  "database_url": "postgresql://wot_server:local-development@127.0.0.1:54329/wot_server",
  "private_key": "login.pem"
}
"#;

impl Configuration {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(mut file) => {
                file.write_all(INITIAL_CONFIGURATION.as_bytes())
                    .context("Write default configuration")?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("Create default configuration"),
        }
        // Serde diagnostics exclude source text, which can contain passwords.
        let mut config: Self = serde_json::from_reader(
            std::fs::File::open(path).context("Open server configuration")?,
        )
        .context("Invalid server configuration")?;
        ensure!(
            !config.database_url.is_empty(),
            "Database URL must be nonempty"
        );
        ensure!(
            (1..=256).contains(&config.db_connections),
            "db_connections must be between 1 and 256"
        );
        ensure!(
            (1..=256).contains(&config.max_clients) && (1..=256).contains(&config.max_pending),
            "max_clients and max_pending must be between 1 and 256"
        );
        ensure!(
            i32::try_from(config.periphery_id).is_ok(),
            "periphery_id must fit the client's signed 32-bit server-list field"
        );
        for value in [&config.resource_dir, &config.private_key] {
            ensure!(
                !value.as_os_str().is_empty(),
                "Configured paths must be nonempty"
            );
        }
        if let Some(address) = config.advertised_base {
            ensure!(
                !address.ip().is_unspecified() && address.port() != 0,
                "advertised_base must be a concrete address and nonzero port"
            );
        }
        let directory = path.parent().context("Configuration has no directory")?;
        for value in [&mut config.resource_dir, &mut config.private_key] {
            if value.is_relative() {
                *value = directory.join(&*value);
            }
        }
        if let Some(store) = &mut config.store {
            store.validate()?;
        }
        if config.web.is_none() && (config.admin.is_some() || config.store.is_some()) {
            let bind = config
                .admin
                .as_ref()
                .and_then(|admin| admin.bind)
                .or_else(|| config.store.as_ref().and_then(|store| store.bind))
                .unwrap_or_else(default_web_bind);
            let ui_dir = config
                .admin
                .as_ref()
                .and_then(|admin| admin.dashboard_dir.clone());
            config.web = Some(WebConfiguration { bind, ui_dir });
        }
        if let Some(ui_dir) = config.web.as_mut().and_then(|web| web.ui_dir.as_mut()) {
            ensure!(
                !ui_dir.as_os_str().is_empty(),
                "Web UI directory must be nonempty"
            );
            if ui_dir.is_relative() {
                *ui_dir = directory.join(&*ui_dir);
            }
        }
        Ok(config)
    }
}

pub(crate) fn admin_trusted_proxy_ips() -> Result<Vec<std::net::IpAddr>> {
    match std::env::var("WOT_ADMIN_TRUSTED_PROXY_IPS") {
        Ok(value) => parse_trusted_proxy_ips(&value),
        Err(std::env::VarError::NotPresent) => Ok(Vec::new()),
        Err(error) => Err(error).context("Read WOT_ADMIN_TRUSTED_PROXY_IPS"),
    }
}

fn parse_trusted_proxy_ips(value: &str) -> Result<Vec<std::net::IpAddr>> {
    ensure!(
        value.len() <= 1024,
        "WOT_ADMIN_TRUSTED_PROXY_IPS exceeds 1024 bytes"
    );
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|ip| {
            ip.trim()
                .parse()
                .context("WOT_ADMIN_TRUSTED_PROXY_IPS must contain comma-separated IP addresses")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_proxy_configuration_requires_explicit_ip_addresses() {
        assert!(parse_trusted_proxy_ips("").unwrap().is_empty());
        assert_eq!(parse_trusted_proxy_ips("192.0.2.1, ::1").unwrap().len(), 2);
        for invalid in [
            "*",
            "10.0.0.0/8",
            "proxy.local",
            "192.0.2.1,",
            &"x".repeat(1025),
        ] {
            assert!(parse_trusted_proxy_ips(invalid).is_err());
        }
    }

    #[test]
    fn creates_defaults_preserves_edits_and_rejects_invalid_settings() {
        let directory = std::env::temp_dir().join(format!(
            "wg-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("server.json");
        let defaults = Configuration::load(&path).unwrap();
        assert_eq!(defaults.login_bind, "0.0.0.0:20016".parse().unwrap());
        assert_eq!(defaults.base_bind, "0.0.0.0:20017".parse().unwrap());
        assert_eq!(defaults.resource_dir, directory.join("resources"));
        assert_eq!(defaults.private_key, directory.join("login.pem"));
        assert!(defaults.training_room_visible);
        assert_eq!(defaults.periphery_id, 250);
        assert_eq!(
            defaults.log_level.filter(),
            tracing::level_filters::LevelFilter::DEBUG
        );
        let mut settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(settings.as_object().unwrap().len(), 3);
        let web = defaults.web.unwrap();
        assert_eq!(web.bind, "0.0.0.0:8080".parse().unwrap());
        assert!(web.ui_dir.is_none());
        assert_eq!(defaults.store.unwrap().origin, "http://127.0.0.1:8080");
        assert!(!defaults.admin.unwrap().secure_cookies);

        let mut web_settings = settings.clone();
        web_settings["web"] = serde_json::json!({});
        web_settings["admin"] = serde_json::json!({});
        std::fs::write(&path, serde_json::to_vec(&web_settings).unwrap()).unwrap();
        let config = Configuration::load(&path).unwrap();
        assert_eq!(config.web.unwrap().bind, "0.0.0.0:8080".parse().unwrap());
        assert!(!config.admin.unwrap().secure_cookies);

        web_settings["web"]["bind"] = "127.0.0.1:9090".into();
        web_settings["admin"]["secure_cookies"] = true.into();
        std::fs::write(&path, serde_json::to_vec(&web_settings).unwrap()).unwrap();
        let config = Configuration::load(&path).unwrap();
        assert_eq!(config.web.unwrap().bind, "127.0.0.1:9090".parse().unwrap());
        assert!(config.admin.unwrap().secure_cookies);

        for field in ["resource_dir", "database_url", "private_key"] {
            let mut incomplete = settings.clone();
            incomplete.as_object_mut().unwrap().remove(field);
            std::fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
            let error = Configuration::load(&path).err().unwrap();
            assert!(format!("{error:#}").contains(&format!("missing field `{field}`")));
        }
        settings["log_level"] = "trace".into();
        settings["max_clients"] = 7.into();
        settings["periphery_id"] = 251.into();
        settings["training_room_visible"] = false.into();
        let edited = serde_json::to_vec_pretty(&settings).unwrap();
        std::fs::write(&path, &edited).unwrap();
        assert_eq!(Configuration::load(&path).unwrap().max_clients, 7);
        assert_eq!(Configuration::load(&path).unwrap().periphery_id, 251);
        assert!(!Configuration::load(&path).unwrap().training_room_visible);
        assert_eq!(std::fs::read(&path).unwrap(), edited);
        settings
            .as_object_mut()
            .unwrap()
            .remove("training_room_visible");
        std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(Configuration::load(&path).unwrap().training_room_visible);
        for (field, value) in [
            ("log_level", serde_json::json!("verbose")),
            ("db_connections", serde_json::json!(0)),
            ("max_pending", serde_json::json!(0)),
            ("log_file", serde_json::json!("old/server.log")),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut invalid = settings.clone();
            invalid[field] = value;
            std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert!(
                Configuration::load(&path).is_err(),
                "accepted invalid {field}"
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
