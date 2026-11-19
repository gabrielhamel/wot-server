mod database;
use crate::{
    accounts::{Accounts, GarageCatalog, PostgresAccounts, PostgresPlayers},
    platform::http,
    player_administration,
    player_sessions::{Gateway, GatewayConfig},
};
use anyhow::{Context, Result};
use clap::Parser;
pub use database::connect_database;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wot_resources::ResourceBundle;

mod browser_telemetry;
mod configuration;
mod logging;
mod login_key;
mod telemetry;
mod web;
use configuration::{Configuration, LogLevel};

#[derive(Parser)]
#[command(version, about = "World of Tanks game server")]
struct Options {
    /// Console log level (defaults to server.json, then warn)
    #[arg(long, value_enum)]
    log_level: Option<LogLevel>,
}

pub async fn run() -> Result<()> {
    let options = Options::parse();
    let executable = std::env::current_exe().context("Locate server executable")?;
    let path = executable
        .parent()
        .context("Locate executable directory")?
        .join("server.json");
    let config = Configuration::load(&path).with_context(|| format!("Load {}", path.display()))?;
    let level = options.log_level.unwrap_or(config.log_level);
    let resource_dir = config.resource_dir.clone();
    let telemetry =
        tokio::task::spawn_blocking(crate::platform::observability::in_current_span(move || {
            logging::initialize(level, &resource_dir)
        }))
        .await
        .context("Join logging initialization")??;
    tracing::info!(configuration = %path.display(), "Starting server");
    let result = serve(config).await;
    if let Err(error) = &result {
        tracing::error!(error = %format!("{error:#}"), "Server stopped with an error");
    }
    if let Some(telemetry) = telemetry
        && let Err(error) = tokio::task::spawn_blocking(
            crate::platform::observability::in_current_span(move || telemetry.shutdown()),
        )
        .await
    {
        tracing::warn!(%error, "OpenTelemetry shutdown task failed");
    }
    result
}

async fn serve(config: Configuration) -> Result<()> {
    let shutdown = shutdown_signal().context("Listen for shutdown signals")?;
    let resources = tracing::info_span!("server.load_resources", "ddd.bounded_context.name" = "server", "input.resource_dir" = %config.resource_dir.display())
        .in_scope(|| ResourceBundle::open(&config.resource_dir))?;
    let pool = connect_database(&config.database_url, config.db_connections).await?;
    let repository = PostgresAccounts::new(pool.clone());
    let keys = repository.catalog_keys().await?;
    let catalog_resources = resources.clone();
    let catalog = Arc::new(
        tokio::task::spawn_blocking(crate::platform::observability::in_current_span(move || {
            GarageCatalog::load(catalog_resources, &keys)
        }))
        .await??,
    );
    let key_path = config.private_key.clone();
    let private_key =
        tokio::task::spawn_blocking(crate::platform::observability::in_current_span(move || {
            login_key::load_login_key(&key_path)
        }))
        .await??;
    let accounts = Accounts::new(repository.clone());
    let social =
        crate::social::postgres::service(pool.clone(), crate::accounts::postgres::directory(pool));
    let mut gateway = Gateway::bind(
        GatewayConfig {
            login_bind: config.login_bind,
            base_bind: config.base_bind,
            advertised_base: config.advertised_base,
            private_key: Some(private_key),
            max_clients: config.max_clients,
            max_pending: config.max_pending,
            training_room_visible: config.training_room_visible,
        },
        &resources,
        catalog.clone(),
        accounts.clone().into(),
        crate::accounts::GarageWrites::new(accounts.clone(), catalog.clone()),
        social,
        tokio::runtime::Handle::current(),
    )?;
    let (web_stop, web_shutdown) = tokio::sync::oneshot::channel();
    let web_worker = if let Some(settings) = config.web {
        let trusted_proxies = if config.admin.is_some() {
            configuration::admin_trusted_proxy_ips()?
        } else {
            Vec::new()
        };
        let mut router = axum::Router::new();
        if let Some(directory) = &settings.ui_dir {
            router = web::pages(
                directory,
                config.admin.is_some(),
                config.store.is_some(),
                trusted_proxies.clone(),
            )?;
            match browser_telemetry::from_environment() {
                Ok(Some(ingress)) => router = router.merge(ingress),
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "Browser telemetry disabled"),
            }
        }
        let store = config
            .store
            .map(|store| gateway.configure_store(store.origin))
            .transpose()?;
        if let Some(store) = &store {
            router = router.merge(crate::player_sessions::store_router(store.clone()));
        }
        if let Some(admin) = config.admin {
            let (players, catalog) = tokio::task::spawn_blocking(
                crate::platform::observability::in_current_span(move || {
                    PostgresPlayers::new(&repository, catalog.clone())
                        .map(|players| (crate::accounts::PlayerAccounts::new(players), catalog))
                }),
            )
            .await??;
            router = router.merge(player_administration::router(
                players,
                accounts.into(),
                catalog,
                gateway.player_grants(),
                store,
                admin.secure_cookies,
                trusted_proxies,
            ));
        }
        let listener = tokio::net::TcpListener::bind(settings.bind)
            .await
            .with_context(|| format!("Bind web HTTP listener at {}", settings.bind))?;
        tracing::info!(address = %listener.local_addr()?, "Web HTTP listener started");
        Some(tokio::spawn(http::serve(
            listener,
            router,
            "web",
            web_shutdown,
        )))
    } else {
        None
    };
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let mut worker =
        tokio::task::spawn_blocking(crate::platform::observability::in_current_span(move || {
            gateway.run(worker_stop)
        }));
    let result = tokio::select! {
        result = &mut worker => result.context("Join game gateway").and_then(|result| result),
        signal = shutdown => {
            stop.store(true, Ordering::Relaxed);
            let result = worker.await.context("Join game gateway").and_then(|result| result);
            signal.context("Wait for shutdown signal").and(result)
        }
    };
    let _ = web_stop.send(());
    if let Some(web_worker) = web_worker {
        web_worker.await.context("Join web HTTP listener")?;
    }
    result
}

#[cfg(unix)]
fn shutdown_signal() -> std::io::Result<impl Future<Output = std::io::Result<()>>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => Ok(()),
            _ = terminate.recv() => Ok(()),
        }
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> std::io::Result<impl Future<Output = std::io::Result<()>>> {
    Ok(tokio::signal::ctrl_c())
}
