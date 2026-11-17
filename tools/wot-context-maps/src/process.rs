use anyhow::{Context, Result, bail};
use std::{
    io::{Read, Seek},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(120);

pub fn capture(command: &mut Command) -> Result<(ExitStatus, String, String)> {
    let description = format!("{command:?}");
    // Files let the child write both streams while we poll for the deadline.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .with_context(|| format!("Start {description}"))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "{description} timed out after {} seconds",
                TIMEOUT.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(25));
    };
    fn read(file: &mut std::fs::File) -> Result<String> {
        file.rewind()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    Ok((status, read(&mut stdout)?, read(&mut stderr)?))
}
