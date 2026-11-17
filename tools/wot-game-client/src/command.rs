use crate::selection::{Selection, resolve_user_path, validate_client};
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "game-client",
    version,
    about = "Locate and provision the compiled research client"
)]
pub struct Arguments {
    #[command(subcommand)]
    pub operation: Operation,
}

#[derive(Subcommand)]
pub enum Operation {
    /// Print the validated absolute client directory (offline).
    Path,
    /// Check local files and hash the executable (offline).
    Status {
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Register an existing client without changing its files.
    Use { path: PathBuf },
    /// Download a client using wg-launcher.
    Install {
        /// Destination. Defaults to the selected path.
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "eu")]
        region: String,
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long)]
        client_type: Option<String>,
    },
    /// Ask wg-launcher for installed and latest versions (network).
    CheckUpdates,
    /// Update the selected client using wg-launcher (network).
    Update,
}

impl Operation {
    pub fn run(
        self,
        selection: &Selection,
        launch: impl FnOnce(&[OsString]) -> Result<()>,
    ) -> Result<Option<String>> {
        match self {
            Self::Use { path } => selection.register(resolve_user_path(&path)?).map(Some),
            Self::Install {
                output,
                region,
                language,
                client_type,
            } => {
                let path = match output {
                    Some(output) => resolve_user_path(&output)?,
                    None => selection.resolve()?,
                };
                ensure!(
                    !path.join("WorldOfTanks.exe").try_exists()?,
                    "Client already exists at {}; use 'use' or 'update' instead",
                    path.display()
                );
                let mut args = vec![
                    "install".into(),
                    "--game".into(),
                    "wot".into(),
                    "--region".into(),
                    region.into(),
                    "--language".into(),
                    language.into(),
                    "--output".into(),
                    path.clone().into_os_string(),
                ];
                if let Some(client_type) = client_type {
                    args.extend([OsString::from("--client-type"), client_type.into()]);
                }
                launch(&args)?;
                selection.register(path).map(Some)
            }
            Self::Path | Self::Status { .. } | Self::CheckUpdates | Self::Update => {
                let path = selection.resolve()?;
                let client = validate_client(path)?;
                match self {
                    Self::Path => Ok(Some(client.path.display().to_string())),
                    Self::Status { json } => client.report(json).map(Some),
                    _ => {
                        let subcommand = if matches!(self, Self::CheckUpdates) {
                            "status"
                        } else {
                            "update"
                        };
                        launch(&[
                            subcommand.into(),
                            "--path".into(),
                            client.path.into_os_string(),
                        ])?;
                        Ok(None)
                    }
                }
            }
        }
    }
}
