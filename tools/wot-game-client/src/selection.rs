use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{ErrorKind, Read, Write},
    path::{self, Component, Path, PathBuf},
};

const SETUP: &str = "Run cargo game-client use /path/to/World_of_Tanks or install.";

pub struct Selection {
    config: PathBuf,
    default_client: PathBuf,
    override_path: Option<PathBuf>,
}

#[derive(Serialize)]
struct SavedClient {
    path: PathBuf,
}

impl Selection {
    pub fn new(workspace: &Path, override_path: Option<OsString>) -> Self {
        let local = workspace.join(".local");
        Self {
            config: local.join("game-client.json"),
            default_client: local.join("game-client"),
            override_path: override_path
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn has_override(&self) -> bool {
        self.override_path.is_some()
    }

    pub fn resolve(&self) -> Result<PathBuf> {
        if let Some(path) = &self.override_path {
            return resolve_user_path(path);
        }
        if self.config.try_exists()? {
            let file = File::open(&self.config)
                .with_context(|| format!("Read {}", self.config.display()))?;
            let data: serde_json::Value = serde_json::from_reader(file)
                .with_context(|| format!("Read {}", self.config.display()))?;
            let saved = data
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(Path::new)
                .filter(|path| path.is_absolute())
                .with_context(|| {
                    format!(
                        "{} must contain an absolute 'path' string",
                        self.config.display()
                    )
                })?;
            return resolve_path(saved);
        }
        resolve_path(&self.default_client)
    }

    pub fn register(&self, path: PathBuf) -> Result<String> {
        let path = validate_client(resolve_path(&path)?)?.path;
        let folder = self
            .config
            .parent()
            .context("Selection has no parent directory")?;
        fs::create_dir_all(folder)?;
        let mut pending = tempfile::NamedTempFile::new_in(folder)?;
        serde_json::to_writer_pretty(&mut pending, &SavedClient { path: path.clone() })?;
        pending.write_all(b"\n")?;
        pending
            .persist(&self.config)
            .with_context(|| format!("Save {}", self.config.display()))?;
        Ok(format!("Registered {}", path.display()))
    }
}

pub fn resolve_user_path(value: &Path) -> Result<PathBuf> {
    if let Ok(relative) = value.strip_prefix("~") {
        let home = home::home_dir().context("Cannot expand '~': no home directory found")?;
        return resolve_path(&home.join(relative));
    }
    resolve_path(value)
}

fn resolve_path(value: &Path) -> Result<PathBuf> {
    let absolute = path::absolute(value)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    let mut resolved = loop {
        match dunce::canonicalize(ancestor) {
            Ok(path) => break path,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let component = ancestor.components().next_back().context("Empty path")?;
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Cannot resolve {}", absolute.display()))?;
                suffix.push(component);
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Resolve {}", absolute.display()));
            }
        }
    };
    for component in suffix.into_iter().rev() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => resolved.push(part),
            _ => {}
        }
    }
    Ok(resolved)
}

pub struct Client {
    pub path: PathBuf,
    executable: PathBuf,
    resources: PathBuf,
}

pub fn validate_client(path: PathBuf) -> Result<Client> {
    let executable = path.join("WorldOfTanks.exe");
    let resources = path.join("res");
    let packages = resources.join("packages");
    let mut missing = Vec::new();
    for (name, item) in [
        ("WorldOfTanks.exe", &executable),
        ("res", &resources),
        ("res/packages", &packages),
    ] {
        if !item.try_exists()? {
            missing.push(name);
        }
    }
    ensure!(
        missing.is_empty(),
        "Client unavailable at {}: missing {}. {SETUP}",
        path.display(),
        missing.join(", ")
    );
    ensure!(
        executable.is_file() && packages.is_dir(),
        "Invalid client layout at {}. {SETUP}",
        path.display()
    );
    Ok(Client {
        path,
        executable,
        resources,
    })
}

#[derive(Serialize)]
struct Status<'a> {
    path: &'a Path,
    executable: &'a Path,
    resources: &'a Path,
    executable_sha256: String,
    version_file: Option<PathBuf>,
}

impl Client {
    pub fn report(&self, json: bool) -> Result<String> {
        let mut executable = File::open(&self.executable)
            .with_context(|| format!("Open {}", self.executable.display()))?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = executable.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let version = self.path.join("version.xml");
        let status = Status {
            path: &self.path,
            executable: &self.executable,
            resources: &self.resources,
            executable_sha256: digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            version_file: version.is_file().then_some(version),
        };
        if json {
            return Ok(serde_json::to_string_pretty(&status)?);
        }
        Ok(format!(
            "path: {}\nexecutable: {}\nresources: {}\nexecutable_sha256: {}\nversion_file: {}\n\
             Local layout checked; package completeness and server compatibility are unverified.\n\
             Run 'check-updates' to query available versions.",
            status.path.display(),
            status.executable.display(),
            status.resources.display(),
            status.executable_sha256,
            status
                .version_file
                .as_deref()
                .map_or_else(|| "None".into(), |path| path.display().to_string()),
        ))
    }
}
