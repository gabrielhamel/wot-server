#![forbid(unsafe_code)]

mod replay;

use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;

use replay::{Metadata, POSITION, Replay};

#[derive(Parser)]
#[command(about = "Inspect a World of Tanks battle replay offline")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print capture metadata and counts by record type and payload size.
    Summary { file: PathBuf },
    /// Write decoded position records as JSON Lines.
    Positions {
        file: PathBuf,
        #[arg(long)]
        entity: Option<u32>,
    },
}

#[derive(Serialize)]
struct Summary<'a> {
    metadata: &'a Metadata,
    records: Vec<RecordCount>,
}

#[derive(Serialize)]
struct RecordCount {
    kind: String,
    payload_bytes: usize,
    count: usize,
}

fn main() -> Result<()> {
    let mut output = BufWriter::new(io::stdout().lock());
    match Args::parse().command {
        Command::Summary { file } => {
            let replay =
                Replay::open(&file).with_context(|| format!("reading {}", file.display()))?;
            let mut counts = BTreeMap::new();
            for record in replay.records() {
                let record = record?;
                *counts
                    .entry((record.kind, record.payload.len()))
                    .or_default() += 1;
            }
            let records = counts
                .into_iter()
                .map(|((kind, payload_bytes), count)| RecordCount {
                    kind: format!("0x{kind:02x}"),
                    payload_bytes,
                    count,
                })
                .collect();
            let summary = Summary {
                metadata: &replay.metadata,
                records,
            };
            serde_json::to_writer_pretty(&mut output, &summary)?;
            writeln!(output)?;
        }
        Command::Positions { file, entity } => {
            let replay =
                Replay::open(&file).with_context(|| format!("reading {}", file.display()))?;
            for record in replay.records() {
                let record = record?;
                if record.kind == POSITION {
                    let position = record.position().with_context(|| {
                        format!("decoding position at stream byte {}", record.offset)
                    })?;
                    if entity.is_none_or(|id| id == position.entity_id) {
                        serde_json::to_writer(&mut output, &position)?;
                        writeln!(output)?;
                    }
                }
            }
        }
    }
    output.flush()?;
    Ok(())
}
