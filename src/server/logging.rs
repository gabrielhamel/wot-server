use super::configuration::LogLevel;
use anyhow::{Context, Result};
use std::{path::Path, sync::Mutex};
use tracing_subscriber::{filter::Targets, fmt::writer::MakeWriterExt};

pub(super) fn initialize(level: LogLevel, path: &Path) -> Result<()> {
    use tracing_subscriber::util::SubscriberInitExt;
    subscriber(level, path)?
        .try_init()
        .map_err(|error| anyhow::anyhow!("Initialize logging: {error}"))
}

fn subscriber(level: LogLevel, path: &Path) -> Result<impl tracing::Subscriber + Send + Sync> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Create log directory")?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context("Open server log")?;
    use tracing_subscriber::layer::SubscriberExt;
    Ok(tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(std::io::stdout.and(Mutex::new(file)))
        .finish()
        .with(
            Targets::new()
                .with_default(tracing::Level::ERROR)
                .with_target("wot_server", level.filter()),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_levels_filter_events_and_preserve_previous_log_contents() {
        for level in [LogLevel::Error, LogLevel::Info, LogLevel::Debug] {
            let path = std::env::temp_dir().join(format!(
                "wg-log-{}-{:?}.log",
                std::process::id(),
                level.filter()
            ));
            std::fs::write(&path, "previous run\n").unwrap();
            tracing::subscriber::with_default(subscriber(level, &path).unwrap(), || {
                tracing::error!("error marker");
                tracing::info!("info marker");
                tracing::debug!("debug marker");
                tracing::trace!("trace marker");
            });
            let contents = std::fs::read_to_string(&path).unwrap();
            assert!(contents.starts_with("previous run\n"));
            assert!(contents.contains("ERROR") && contents.contains("error marker"));
            assert_eq!(
                contents.contains("info marker"),
                !matches!(level, LogLevel::Error)
            );
            assert_eq!(
                contents.contains("debug marker"),
                matches!(level, LogLevel::Debug)
            );
            assert!(!contents.contains("trace marker"));
            std::fs::remove_file(path).unwrap();
        }
    }
}
