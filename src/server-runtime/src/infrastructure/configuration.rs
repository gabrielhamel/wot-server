use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum LogLevel {
    Error,
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

#[derive(Parser)]
#[command(name = "wot-server", version, about = "World of Tanks game server")]
pub(crate) struct Configuration {
    /// Directory containing the extracted resource bundle
    #[arg(long)]
    pub resource_dir: PathBuf,

    /// PostgreSQL connection URL
    #[arg(long, env = "WOT_DATABASE_URL", hide_env_values = true)]
    pub database_url: String,

    /// RSA private key path. A key is generated there if absent
    #[arg(long)]
    pub private_key: PathBuf,

    /// IPv4 address and port for LoginApp UDP
    #[arg(long, default_value = "0.0.0.0:20016")]
    pub login_bind: SocketAddrV4,

    /// IPv4 address and port for BaseApp UDP and XMPP TCP
    #[arg(long, default_value = "0.0.0.0:20017")]
    pub base_bind: SocketAddrV4,

    /// IPv4 address and port advertised to remote game clients
    #[arg(long)]
    pub advertised_base: Option<SocketAddrV4>,

    /// Maximum PostgreSQL connections
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=256))]
    pub db_connections: u32,

    /// Maximum admitted players and pending session handoffs
    #[arg(long, default_value_t = 256)]
    pub max_clients: usize,

    /// Maximum pending login requests
    #[arg(long, default_value_t = 32)]
    pub max_pending: usize,

    /// Hide the Training Room card and disable room creation
    #[arg(long)]
    pub disable_training_room: bool,

    /// Local server-list ID used for Training Room invitations
    #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(0..=i64::from(i32::MAX)))]
    pub periphery_id: u32,

    /// Console log level
    #[arg(long, value_enum, default_value = "debug")]
    pub log_level: LogLevel,

    /// IPv4 address and port for the shared HTTP listener
    #[arg(long, default_value = "0.0.0.0:8080")]
    pub web_bind: SocketAddrV4,

    /// Serve the dashboard and Store UI from this directory
    #[arg(long)]
    pub web_ui_dir: Option<PathBuf>,

    /// Disable the dashboard and admin API
    #[arg(long)]
    pub disable_admin: bool,

    /// Mark admin session cookies Secure for HTTPS
    #[arg(long)]
    pub secure_cookies: bool,

    /// Disable the in-game Store and its HTTP routes
    #[arg(long)]
    pub disable_store: bool,

    /// Override the automatically selected local Store origin (HTTPS or local HTTP)
    #[arg(long)]
    pub store_origin: Option<String>,
}

impl Configuration {
    pub(crate) fn validate(&mut self) -> Result<()> {
        ensure!(
            !self.database_url.is_empty(),
            "Database URL must be nonempty"
        );
        ensure!(
            (1..=256).contains(&self.max_clients) && (1..=256).contains(&self.max_pending),
            "--max-clients and --max-pending must be between 1 and 256"
        );
        if let Some(address) = self.advertised_base {
            ensure!(
                !address.ip().is_unspecified() && address.port() != 0,
                "--advertised-base must be a concrete address and nonzero port"
            );
        }
        if !self.disable_store
            && let Some(origin) = &mut self.store_origin
        {
            validate_store_origin(origin)?;
        }
        Ok(())
    }
}

fn validate_store_origin(origin: &mut String) -> Result<()> {
    let uri = origin
        .parse::<axum::http::Uri>()
        .context("Invalid Store origin")?;
    let host = uri.host().context("Store origin requires a host")?;
    let local_http = matches!(host, "localhost" | "[::1]")
        || host
            .parse::<Ipv4Addr>()
            .is_ok_and(|address| address.is_loopback() || address.is_private());
    ensure!(
        uri.scheme_str() == Some("https") || (uri.scheme_str() == Some("http") && local_http),
        "Store origin requires HTTPS, or HTTP with a loopback or private IPv4 address"
    );
    ensure!(
        uri.path() == "/" && uri.query().is_none() && !origin.contains(['@', '#']),
        "Store origin must contain only a scheme, host and optional port"
    );
    origin.truncate(origin.trim_end_matches('/').len());
    Ok(())
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
    fn store_origin_allows_explicit_lan_http_and_requires_https_for_public_hosts() {
        for origin in [
            "http://192.168.1.3:8080",
            "http://10.0.0.3",
            "http://172.16.0.3",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
            "https://store.example.test",
        ] {
            let mut configured = format!("{origin}/");
            validate_store_origin(&mut configured).unwrap();
            assert_eq!(configured, origin);
        }

        for origin in [
            "http://198.51.100.3:8080",
            "http://172.32.0.3",
            "http://0.0.0.0:8080",
            "http://store.example.test",
            "http://192.168.1.3:8080/shop",
            "http://192.168.1.3:8080/#token",
            "http://user@192.168.1.3:8080",
        ] {
            assert!(
                validate_store_origin(&mut origin.to_owned()).is_err(),
                "{origin}"
            );
        }
    }

    #[test]
    fn command_line_defaults_and_overrides() {
        let args = [
            "wot-server",
            "--resource-dir",
            "resources",
            "--database-url",
            "postgresql://localhost/wot_server",
            "--private-key",
            "state/login.pem",
        ];
        let mut defaults = Configuration::try_parse_from(args).unwrap();
        defaults.validate().unwrap();
        assert_eq!(defaults.resource_dir, PathBuf::from("resources"));
        assert_eq!(defaults.private_key, PathBuf::from("state/login.pem"));
        assert_eq!(defaults.login_bind, "0.0.0.0:20016".parse().unwrap());
        assert_eq!(defaults.base_bind, "0.0.0.0:20017".parse().unwrap());
        assert_eq!(defaults.web_bind, "0.0.0.0:8080".parse().unwrap());
        assert!(defaults.store_origin.is_none());
        assert_eq!(
            defaults.log_level.filter(),
            tracing::level_filters::LevelFilter::DEBUG
        );
        assert_eq!(defaults.db_connections, 16);
        assert_eq!(defaults.max_clients, 256);
        assert_eq!(defaults.max_pending, 32);
        assert_eq!(defaults.periphery_id, 250);
        assert!(defaults.web_ui_dir.is_none());
        assert!(defaults.advertised_base.is_none());
        assert!(!defaults.secure_cookies);
        assert!(!defaults.disable_admin);
        assert!(!defaults.disable_store);
        assert!(!defaults.disable_training_room);

        let mut config = Configuration::try_parse_from(args.into_iter().chain([
            "--web-bind",
            "127.0.0.1:9090",
            "--web-ui-dir",
            "dist",
            "--secure-cookies",
            "--disable-admin",
            "--disable-store",
            "--disable-training-room",
        ]))
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.web_bind, "127.0.0.1:9090".parse().unwrap());
        assert_eq!(config.web_ui_dir, Some(PathBuf::from("dist")));
        assert!(config.secure_cookies);
        assert!(config.disable_admin);
        assert!(config.disable_store);
        assert!(config.disable_training_room);

        for invalid in [
            ["--log-level", "verbose"],
            ["--db-connections", "0"],
            ["--max-pending", "0"],
            ["--periphery-id", "2147483648"],
            ["--advertised-base", "0.0.0.0:20017"],
            ["--store-origin", "http://example.test"],
        ] {
            let rejected = Configuration::try_parse_from(args.into_iter().chain(invalid))
                .map_or(true, |mut config| config.validate().is_err());
            assert!(rejected, "accepted {invalid:?}");
        }
    }
}
