use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(about = "Extract a standalone resource bundle for wot-server")]
struct Command {
    /// World of Tanks installation directory.
    game_dir: PathBuf,
    /// New directory to write the resource bundle into.
    #[arg(short, long)]
    output: PathBuf,
}

pub fn run() -> Result<()> {
    let command = Command::parse();
    let summary = crate::application::extract(&command.game_dir, &command.output)?;
    println!(
        "Extracted {} resources ({} bytes) into {}",
        summary.files,
        summary.bytes,
        command.output.display()
    );
    Ok(())
}
