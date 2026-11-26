#![forbid(unsafe_code)]

//! Server startup, adapter composition, and shutdown.

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wot_platform::http;
use wot_player_administration::contract as player_administration;
use wot_player_sessions::{Gateway, GatewayAccounts, GatewayConfig};
use wot_resources::ResourceBundle;

mod infrastructure;

use infrastructure::{
    browser_telemetry,
    configuration::{self, Configuration},
    database::connect_database,
    logging, login_key, telemetry, web,
};

#[cfg(any(test, feature = "integration-test-support"))]
#[doc(hidden)]
pub mod test_support {
    pub use super::infrastructure::{database::connect_database, logging::server_targets};

    pub fn configure_gateway_metrics(
        gateway: &mut wot_player_sessions::Gateway,
        meter: &opentelemetry::metrics::Meter,
    ) {
        gateway.configure_metrics(meter);
    }

    pub fn configure_database_metrics(meter: &opentelemetry::metrics::Meter, pool: &sqlx::PgPool) {
        wot_platform::observability::configure_database_metrics(meter, pool);
    }
}

pub async fn run() -> Result<()> {
    let mut config = Configuration::parse();
    config.validate()?;
    let level = config.log_level;
    let resource_dir = config.resource_dir.clone();
    let telemetry =
        tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
            logging::initialize(level, &resource_dir)
        }))
        .await
        .context("Join logging initialization")??;
    tracing::info!("Starting server");
    let result = serve(config, telemetry.as_ref().map(telemetry::Telemetry::meter)).await;
    if let Err(error) = &result {
        tracing::error!(error = %format!("{error:#}"), "Server stopped with an error");
    }
    if let Some(telemetry) = telemetry
        && let Err(error) =
            tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
                telemetry.shutdown()
            }))
            .await
    {
        tracing::warn!(%error, "OpenTelemetry shutdown task failed");
    }
    result
}

async fn serve(config: Configuration, meter: Option<opentelemetry::metrics::Meter>) -> Result<()> {
    let shutdown = shutdown_signal().context("Listen for shutdown signals")?;
    let resources = tracing::info_span!("server.load_resources", "ddd.bounded_context.name" = "server", "input.resource_dir" = %config.resource_dir.display())
        .in_scope(|| ResourceBundle::open(&config.resource_dir))?;
    let pool = connect_database(&config.database_url, config.db_connections).await?;
    if let Some(meter) = &meter {
        wot_platform::observability::configure_database_metrics(meter, &pool);
    }
    let account_services = wot_accounts::postgres::service(pool.clone(), resources.clone()).await?;
    let key_path = config.private_key.clone();
    let private_key =
        tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
            login_key::load_login_key(&key_path)
        }))
        .await??;
    let social =
        wot_social::postgres::service(pool.clone(), wot_accounts::postgres::directory(pool));
    let mut gateway = Gateway::bind(
        wot_battles::mercury::bind,
        GatewayConfig {
            login_bind: config.login_bind,
            base_bind: config.base_bind,
            advertised_base: config.advertised_base,
            private_key: Some(private_key),
            max_clients: config.max_clients,
            max_pending: config.max_pending,
            training_room_visible: !config.disable_training_room,
            periphery_id: config.periphery_id,
        },
        &resources,
        GatewayAccounts {
            accounts: account_services.accounts(),
            catalog: account_services.garage_queries(),
            garage: account_services.garage_commands(),
        },
        social,
        tokio::runtime::Handle::current(),
    )?;
    if let Some(meter) = &meter {
        gateway.configure_metrics(meter);
    }
    let (web_stop, web_shutdown) = tokio::sync::oneshot::channel();
    let admin_enabled = !config.disable_admin;
    let store_enabled = !config.disable_store;
    let web_worker = if admin_enabled || store_enabled || config.web_ui_dir.is_some() {
        let trusted_proxies = if admin_enabled {
            configuration::admin_trusted_proxy_ips()?
        } else {
            Vec::new()
        };
        let mut router = web::pages(
            config.web_ui_dir.as_deref(),
            admin_enabled,
            store_enabled,
            trusted_proxies.clone(),
        )?;
        match browser_telemetry::from_environment() {
            Ok(Some(ingress)) => router = router.merge(ingress),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Browser telemetry disabled"),
        }
        let listener = tokio::net::TcpListener::bind(config.web_bind)
            .await
            .with_context(|| format!("Bind web HTTP listener at {}", config.web_bind))?;
        let web_address = listener.local_addr()?;
        let store = if store_enabled {
            let garage = match config.store_origin {
                Some(origin) => gateway.configure_store(origin)?,
                None => gateway.configure_store_on_listener(web_address),
            };
            Some(wot_store::contract::Store::new(
                account_services.accounts(),
                account_services.garage_queries(),
                garage,
            ))
        } else {
            None
        };
        if let Some(store) = &store {
            router = router.merge(wot_store::contract::router(store.clone()));
        }
        if admin_enabled {
            let garage_accounts = account_services.accounts();
            let catalog = account_services.garage_queries();
            let players =
                tokio::task::spawn_blocking(wot_platform::observability::in_current_span(
                    move || account_services.player_access(),
                ))
                .await??;
            router = router.merge(player_administration::router(
                players,
                garage_accounts,
                catalog,
                gateway.player_grants(),
                store,
                player_administration::DashboardConfig {
                    secure_cookies: config.secure_cookies,
                    trusted_proxies,
                    battles: Some(gateway.training_battle_launches()),
                },
            ));
        }
        tracing::info!(address = %web_address, "Web HTTP listener started");
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
        tokio::task::spawn_blocking(wot_platform::observability::in_current_span(move || {
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
