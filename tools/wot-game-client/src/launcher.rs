use anyhow::{Context, Result, bail, ensure};
use std::{ffi::OsString, io::ErrorKind, process::Command};

pub fn run(arguments: &[OsString]) -> Result<()> {
    let status = match Command::new("wg-launcher").args(arguments).status() {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("wg-launcher is missing from PATH. See docs/game-client.md for setup.");
        }
        result => result.context("Run wg-launcher")?,
    };
    ensure!(status.success(), "wg-launcher failed: {status}");
    Ok(())
}
