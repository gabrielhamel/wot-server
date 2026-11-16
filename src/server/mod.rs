use crate::{
    access::{Gateway, GatewayConfig},
    accounts::{Accounts, GarageCatalog, PlayerAccounts, PostgresAccounts, PostgresPlayers},
    admin,
};
use anyhow::{Context, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wot_resources::ResourceBundle;

mod configuration;
mod logging;
mod login_key;
use configuration::Configuration;

pub async fn run() -> Result<()> {
    let executable = std::env::current_exe().context("Locate server executable")?;
    let path = executable
        .parent()
        .context("Locate executable directory")?
        .join("server.json");
    anyhow::ensure!(
        std::env::args_os().len() == 1,
        "The server takes no arguments; edit {}",
        path.display()
    );
    let config = Configuration::load(&path).with_context(|| format!("Load {}", path.display()))?;
    logging::initialize(config.log_level, &config.log_file)?;
    tracing::info!(configuration = %path.display(), "Starting server");
    let result = serve(config).await;
    if let Err(error) = &result {
        tracing::error!(error = %format!("{error:#}"), "Server stopped with an error");
    }
    result
}

async fn serve(config: Configuration) -> Result<()> {
    let resources = ResourceBundle::open(&config.resource_dir)?;
    let repository = PostgresAccounts::connect(&config.database_url, config.db_connections).await?;
    let keys = repository.catalog_keys().await?;
    let catalog_resources = resources.clone();
    let catalog = Arc::new(
        tokio::task::spawn_blocking(move || GarageCatalog::load(catalog_resources, &keys))
            .await??,
    );
    let key_path = config.private_key.clone();
    let private_key =
        tokio::task::spawn_blocking(move || login_key::load_login_key(&key_path)).await??;
    let accounts = Accounts::new(repository.clone());
    let gateway = Gateway::bind(
        GatewayConfig {
            login_bind: config.login_bind,
            base_bind: config.base_bind,
            advertised_base: config.advertised_base,
            private_key: Some(private_key),
            max_clients: config.max_clients,
            max_pending: config.max_pending,
        },
        &resources,
        catalog.clone(),
        accounts.clone(),
        tokio::runtime::Handle::current(),
    )?;
    let (admin_stop, admin_shutdown) = tokio::sync::oneshot::channel();
    let admin_listener = if let Some(settings) = config.admin {
        if let Some(dashboard) = &settings.dashboard_dir {
            anyhow::ensure!(
                dashboard.join("index.html").is_file() && dashboard.join("assets").is_dir(),
                "Build the admin dashboard before using {}",
                dashboard.display()
            );
        }
        let token = admin::AdminToken::new(&settings.token);
        let listener = tokio::net::TcpListener::bind(settings.bind)
            .await
            .with_context(|| format!("Bind admin HTTP listener at {}", settings.bind))?;
        let player_catalog = catalog.clone();
        let player_repository = repository.clone();
        let players = tokio::task::spawn_blocking(move || {
            PostgresPlayers::new(&player_repository, player_catalog)
        })
        .await??;
        let router = admin::router(
            PlayerAccounts::new(players),
            accounts,
            catalog,
            gateway.player_grants(),
            token,
            settings.dashboard_dir.as_deref(),
        );
        Some((listener, router))
    } else {
        None
    };
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let admin_worker = if let Some((listener, router)) = admin_listener {
        tracing::info!(address = %listener.local_addr()?, "Admin HTTP listener started");
        Some(tokio::spawn(admin::serve(listener, router, admin_shutdown)))
    } else {
        None
    };
    let mut worker = tokio::task::spawn_blocking(move || gateway.run(worker_stop));
    let result = tokio::select! {
        result = &mut worker => result.context("Join game gateway").and_then(|result| result),
        signal = tokio::signal::ctrl_c() => {
            stop.store(true, Ordering::Relaxed);
            let result = worker.await.context("Join game gateway").and_then(|result| result);
            signal.context("Wait for shutdown signal").and(result)
        }
    };
    let _ = admin_stop.send(());
    if let Some(admin_worker) = admin_worker {
        admin_worker.await.context("Join admin HTTP listener")?;
    }
    result
}
