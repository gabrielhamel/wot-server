use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    net::SocketAddrV4,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Default, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogLevel {
    Error,
    #[default]
    Warn,
    Info,
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdminConfiguration {
    #[serde(default = "default_web_bind")]
    pub bind: SocketAddrV4,
    #[serde(default = "secure_admin_cookies")]
    pub secure_cookies: bool,
    pub dashboard_dir: Option<PathBuf>,
}

fn secure_admin_cookies() -> bool {
    true
}

fn default_web_bind() -> SocketAddrV4 {
    SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 8080)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebConfiguration {
    #[serde(default = "default_web_bind")]
    pub bind: SocketAddrV4,
    pub ui_dir: Option<PathBuf>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoreConfiguration {
    bind: Option<SocketAddrV4>,
    pub origin: String,
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

#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Configuration {
    pub resource_dir: PathBuf,
    pub database_url: String,
    pub login_bind: SocketAddrV4,
    pub base_bind: SocketAddrV4,
    pub advertised_base: Option<SocketAddrV4>,
    pub private_key: PathBuf,
    pub db_connections: u32,
    pub max_clients: usize,
    pub max_pending: usize,
    pub training_room_visible: bool,
    pub periphery_id: u32,
    pub log_level: LogLevel,
    pub web: Option<WebConfiguration>,
    pub admin: Option<AdminConfiguration>,
    pub store: Option<StoreConfiguration>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            resource_dir: "resources".into(),
            database_url: "postgresql://wot_server:local-development@127.0.0.1:54329/wot_server"
                .into(),
            login_bind: "0.0.0.0:20016".parse().unwrap(),
            base_bind: "0.0.0.0:20017".parse().unwrap(),
            advertised_base: None,
            private_key: "login.pem".into(),
            db_connections: 16,
            max_clients: 256,
            max_pending: 32,
            training_room_visible: true,
            periphery_id: 250,
            log_level: LogLevel::default(),
            web: None,
            admin: None,
            store: None,
        }
    }
}

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
                let mut defaults = serde_json::to_string_pretty(&Self::default())?;
                defaults.push('\n');
                file.write_all(defaults.as_bytes())
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
                .map(|admin| admin.bind)
                .or_else(|| config.store.as_ref().and_then(|store| store.bind))
                .unwrap_or_else(default_web_bind);
            let ui_dir = config
                .admin
                .as_ref()
                .and_then(|admin| admin.dashboard_dir.clone());
            config.web = Some(WebConfiguration { bind, ui_dir });
        }
        if let Some(web) = &mut config.web {
            if config.store.is_some() && web.ui_dir.is_none() {
                web.ui_dir = Some(PathBuf::from("web"));
            }
            if let Some(ui_dir) = &mut web.ui_dir {
                ensure!(
                    !ui_dir.as_os_str().is_empty(),
                    "Web UI directory must be nonempty"
                );
                if ui_dir.is_relative() {
                    *ui_dir = directory.join(&*ui_dir);
                }
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
            tracing::level_filters::LevelFilter::WARN
        );
        let mut settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(settings.get("log_file").is_none());
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
