#![forbid(unsafe_code)]

//! Shared dashboard and Store build for Cargo and the standalone web command.

use anyhow::{Context, Result, ensure};
use std::{
    collections::hash_map::DefaultHasher,
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

const WORKSPACE: &str = "wot-server-web";

pub fn build(watch_inputs: bool) -> Result<PathBuf> {
    let web = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = web
        .ancestors()
        .find(|path| path.join("package-lock.json").is_file())
        .context("Locate the npm workspace root")?;
    let local = root.join(".local");
    fs::create_dir_all(&local)?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(local.join("web-build.lock"))?;
    lock.lock().context("Lock the web build")?;

    let manifests = [
        root.join("package.json"),
        root.join("package-lock.json"),
        web.join("package.json"),
    ];
    let mut sources = manifests[..2].to_vec();
    collect_files(&root.join("src"), &mut sources, |path| {
        matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "tsx" | "html" | "css" | "json" | "svg" | "png" | "webp" | "woff2")
        )
    })?;

    let version = env::var_os("WOT_SERVER_VERSION");
    let inputs = fingerprint(root, &mut sources, &version)?;
    let dist = web.join("dist");
    let stamp = local.join("web-bundle.stamp");
    let previous = local.join("web-dist-previous");
    recover_bundle(&dist, &previous)?;
    if watch_inputs {
        for path in sources.iter().chain([&dist]) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    if bundle_stamp(&dist, inputs)?
        .is_some_and(|value| fs::read_to_string(&stamp).is_ok_and(|existing| existing == value))
    {
        return Ok(dist);
    }

    install_dependencies(root, &manifests)?;
    let staging = local.join("web-dist");
    run_npm(
        npm(root)
            .args(["run", "build", "--workspace", WORKSPACE, "--", "--outDir"])
            .arg(&staging)
            .arg("--emptyOutDir"),
        "Bundle dashboard and Store assets",
    )?;
    let value =
        bundle_stamp(&staging, inputs)?.context("Vite produced an incomplete web bundle")?;
    publish_bundle(&staging, &dist, &previous)?;
    fs::write(stamp, value)?;
    Ok(dist)
}

fn collect_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    include: impl Fn(&Path) -> bool + Copy,
) -> Result<()> {
    for entry in fs::read_dir(directory).with_context(|| format!("Read {}", directory.display()))? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !matches!(entry.file_name().to_str(), Some("node_modules" | "dist")) {
                collect_files(&path, files, include)?;
            }
        } else if kind.is_file() && include(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn fingerprint(base: &Path, files: &mut [PathBuf], extra: &impl Hash) -> Result<u64> {
    files.sort_unstable();
    let mut hash = DefaultHasher::new();
    extra.hash(&mut hash);
    for path in files.iter() {
        path.strip_prefix(base)?.hash(&mut hash);
        fs::read(path)
            .with_context(|| format!("Read {}", path.display()))?
            .hash(&mut hash);
    }
    Ok(hash.finish())
}

fn bundle_stamp(dist: &Path, inputs: u64) -> Result<Option<String>> {
    let index = dist.join("index.html");
    let assets = dist.join("assets");
    if !index.is_file() || !assets.is_dir() {
        return Ok(None);
    }
    let mut files = Vec::new();
    collect_files(&assets, &mut files, |_| true)?;
    if files.is_empty() {
        return Ok(None);
    }
    files.push(index);
    Ok(Some(format!(
        "{}\n",
        fingerprint(dist, &mut files, &inputs)?
    )))
}

fn install_dependencies(root: &Path, manifests: &[PathBuf; 3]) -> Result<()> {
    let version = Command::new("node")
        .arg("--version")
        .output()
        .context("Run Node.js for the web build. Install a supported Node.js version with npm")?;
    ensure!(version.status.success(), "node --version failed");
    let stamp = root.join("node_modules/.wot-web-dependencies");
    let value = format!(
        "{}\n",
        fingerprint(root, &mut manifests.clone(), &version.stdout)?
    );
    let cli = root.join(if cfg!(windows) {
        "node_modules/.bin/vp.cmd"
    } else {
        "node_modules/.bin/vp"
    });
    if !cli.is_file() || fs::read_to_string(&stamp).ok().as_deref() != Some(value.as_str()) {
        run_npm(
            npm(root).args(["ci", "--no-audit", "--no-fund", "--engine-strict"]),
            "Install locked web dependencies",
        )?;
        fs::write(stamp, value)?;
    }
    Ok(())
}

fn npm(root: &Path) -> Command {
    let mut command = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
    command.current_dir(root);
    command
}

fn run_npm(command: &mut Command, operation: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("{operation}: running npm requires Node.js and npm"))?;
    ensure!(status.success(), "{operation}: npm exited with {status}");
    Ok(())
}

fn recover_bundle(dist: &Path, previous: &Path) -> Result<()> {
    if previous.exists() {
        if dist.exists() {
            fs::remove_dir_all(previous)?;
        } else {
            fs::rename(previous, dist)?;
        }
    }
    Ok(())
}

fn publish_bundle(staging: &Path, dist: &Path, previous: &Path) -> Result<()> {
    if dist.exists() {
        fs::rename(dist, previous)?;
    }
    if let Err(error) = fs::rename(staging, dist) {
        if previous.exists() {
            fs::rename(previous, dist).context("Restore the previous web bundle")?;
        }
        return Err(error).context("Publish the web bundle");
    }
    if previous.exists() {
        fs::remove_dir_all(previous)?;
    }
    Ok(())
}
