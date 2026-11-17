use crate::binary_mapping::{application, infrastructure};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Map World of Tanks PE64 network registrations and native RTTI into Rust and JSON"
)]
struct Arguments {
    /// Installed game folder, or a specific AMD64 WorldOfTanks.exe.
    game_folder: PathBuf,
    /// Destination directory (must not already exist).
    #[arg(short, long)]
    output: PathBuf,
}

pub fn run() -> Result<()> {
    let args = Arguments::parse();
    let executable = if args.game_folder.is_file() {
        args.game_folder.clone()
    } else {
        args.game_folder.join("win64/WorldOfTanks.exe")
    };
    ensure!(
        !args.output.exists(),
        "output directory already exists: {}",
        args.output.display()
    );
    let report = application::investigate(&executable, &infrastructure::StaticInvestigation)?;
    let source = infrastructure::rust::render(&report);
    let json = serde_json::to_vec_pretty(&report)?;
    if let Some(parent) = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).context("create output parent directory")?;
    }
    std::fs::create_dir(&args.output).context("create new output directory")?;
    std::fs::write(args.output.join("structures.rs"), source)?;
    std::fs::write(args.output.join("mapping.json"), json)?;
    println!(
        "Mapped {} interfaces, {} message registrations and {} native types to {}",
        report.interfaces.len(),
        report
            .interfaces
            .iter()
            .map(|interface| interface.messages.len())
            .sum::<usize>(),
        report.native_types.len(),
        args.output.display()
    );
    Ok(())
}
