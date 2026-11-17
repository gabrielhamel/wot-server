#![forbid(unsafe_code)]

mod command;
mod launcher;
mod selection;

#[cfg(test)]
mod tests;

use clap::Parser;
use command::{Arguments, Operation};
use selection::Selection;
use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let selection = Selection::new(&root, std::env::var_os("WOT_GAME_DIR"));
    let warn_override =
        matches!(arguments.operation, Operation::Use { .. }) && selection.has_override();
    match arguments.operation.run(&selection, launcher::run) {
        Ok(output) => {
            if let Some(output) = output {
                println!("{output}");
            }
            if warn_override {
                eprintln!("WOT_GAME_DIR still overrides this saved selection.");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("game-client: {error:#}");
            ExitCode::FAILURE
        }
    }
}
