#![forbid(unsafe_code)]

mod generate;
mod mapper;
mod process;
mod render;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Validate CML models and render deterministic SVG context maps"
)]
struct Arguments {
    /// Directory containing CML files. Defaults to docs/context-maps in this checkout.
    #[arg(long)]
    models_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Arguments::parse();
    let models_dir = args
        .models_dir
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/context-maps"));
    let models_dir = dunce::canonicalize(&models_dir)
        .with_context(|| format!("Open model directory {}", models_dir.display()))?;
    generate::run(&models_dir)
}
