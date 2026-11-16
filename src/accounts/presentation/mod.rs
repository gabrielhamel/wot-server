use crate::accounts::{GarageCatalog, PostgresAccounts};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;
use wot_resources::ResourceBundle;

#[derive(Parser)]
#[command(about = "Seed Player_1 and Player_2 with tier I tech tree garages; no server is started")]
struct SeedCommand {
    /// Existing server.json (defaults to beside this executable).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Override the extracted resource bundle directory.
    #[arg(long)]
    resource_dir: Option<PathBuf>,
    /// Override the PostgreSQL connection URL.
    #[arg(long)]
    database_url: Option<String>,
}

#[derive(Deserialize)]
struct SeedConfiguration {
    #[serde(default = "default_resource_dir")]
    resource_dir: PathBuf,
    database_url: String,
}

fn default_resource_dir() -> PathBuf {
    "resources".into()
}

pub async fn run_seed() -> Result<()> {
    let command = SeedCommand::parse();
    let explicit_config = command.config.is_some();
    let path = match command.config {
        Some(path) => path,
        None => std::env::current_exe()?.with_file_name("server.json"),
    };
    let configuration = if command.resource_dir.is_none() || command.database_url.is_none() {
        let mut config: SeedConfiguration = match std::fs::File::open(&path) {
            Ok(file) => serde_json::from_reader(file)
                .context("Read seed database and resource configuration")?,
            Err(error) if !explicit_config && error.kind() == std::io::ErrorKind::NotFound => {
                SeedConfiguration {
                    resource_dir: default_resource_dir(),
                    database_url:
                        "postgresql://wot_server:local-development@127.0.0.1:54329/wot_server"
                            .into(),
                }
            }
            Err(error) => return Err(error).context("Open seed configuration"),
        };
        if config.resource_dir.is_relative() {
            config.resource_dir = path
                .parent()
                .context("Configuration has no parent")?
                .join(config.resource_dir);
        }
        Some(config)
    } else {
        None
    };
    let resource_dir = command
        .resource_dir
        .or_else(|| configuration.as_ref().map(|c| c.resource_dir.clone()))
        .context("Missing resource directory")?;
    let database_url = command
        .database_url
        .or_else(|| configuration.map(|c| c.database_url))
        .context("Missing database URL")?;
    ensure!(!database_url.is_empty(), "Database URL must be nonempty");
    let catalog = GarageCatalog::load(ResourceBundle::open(&resource_dir)?, &[])
        .context("Load seed resource bundle")?;
    let repository = PostgresAccounts::connect(&database_url, 2).await?;
    let reports = repository.seed_demo_players(&catalog).await?;
    for report in reports {
        let status = if report.created {
            "created"
        } else if report.changed {
            "updated"
        } else {
            "unchanged"
        };
        println!(
            "{} (account {}): {status}, {} tier I vehicles",
            report.username, report.account_id, report.tier_one_vehicles
        );
    }
    Ok(())
}
