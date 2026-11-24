#![forbid(unsafe_code)]

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
};
use tempfile::NamedTempFile;

const LOCK_NAME: &str = ".wot-login-key.lock";

#[derive(Parser)]
#[command(
    version,
    about = "Generate a 2048-bit RSA login key pair for wot-server"
)]
struct Command {
    /// Base filename for the key pair, such as login.
    #[arg(long, value_parser = key_name)]
    name: String,
    /// Directory where the pair will be saved.
    #[arg(long, value_name = "DIRECTORY")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let command = Command::parse();
    let directory = &command.output;
    fs::create_dir_all(directory).with_context(|| format!("Create {}", directory.display()))?;
    let private_path = directory.join(format!("{}.pem", command.name));
    let public_path = directory.join(format!("{}.pubkey", command.name));
    let _lock = lock_directory(directory)?;

    let private_exists = existing_file(&private_path)?.is_some();
    let public_metadata = existing_file(&public_path)?;
    let public_exists = public_metadata.is_some();
    if (private_exists || public_exists) && !confirm_replacement(&private_path, &public_path)? {
        println!("Key generation cancelled.");
        return Ok(());
    }

    let private_key = RsaPrivateKey::new(&mut OsRng, 2048).context("Generate login key")?;
    let private_pem = private_key.to_pkcs8_pem(LineEnding::LF)?;
    let public_pem = RsaPublicKey::from(&private_key).to_public_key_pem(LineEnding::LF)?;
    let pending_private = stage(directory, private_pem.as_bytes()).context("Stage private key")?;
    let pending_public = stage(directory, public_pem.as_bytes()).context("Stage public key")?;
    let previous_public = if let Some(metadata) = public_metadata {
        let previous =
            fs::read(&public_path).with_context(|| format!("Read {}", public_path.display()))?;
        let pending = stage(directory, &previous).context("Stage previous public key")?;
        pending.as_file().set_permissions(metadata.permissions())?;
        Some(pending)
    } else {
        None
    };

    publish(pending_public, &public_path, public_exists)?;
    if let Err(error) = publish(pending_private, &private_path, private_exists) {
        // The private key remains authoritative until its replacement succeeds.
        let restored = match previous_public {
            Some(previous) => publish(previous, &public_path, true),
            None => fs::remove_file(&public_path).map_err(Into::into),
        };
        restored
            .with_context(|| format!("{error:#}. Could not restore {}", public_path.display()))?;
        return Err(error);
    }

    println!("Private key: {}", private_path.display());
    println!("Public key:  {}", public_path.display());
    Ok(())
}

fn key_name(value: &str) -> Result<String, String> {
    if Path::new(value)
        .file_name()
        .is_some_and(|name| name == value)
        && !value.ends_with(['.', ' '])
        && !value.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && !value.chars().any(char::is_control)
    {
        Ok(value.into())
    } else {
        Err("expected a base filename without directory components".into())
    }
}

fn lock_directory(directory: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(LOCK_NAME))
        .with_context(|| format!("Open key generation lock in {}", directory.display()))?;
    lock.lock()
        .with_context(|| format!("Lock {}", directory.display()))?;
    Ok(lock)
}

fn existing_file(path: &Path) -> Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file(),
                "{} must be a regular file",
                path.display()
            );
            Ok(Some(metadata))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Inspect {}", path.display())),
    }
}

fn confirm_replacement(private: &Path, public: &Path) -> Result<bool> {
    eprint!(
        "A login key file already exists. Generate a new pair at {} and {}? [y/N] ",
        private.display(),
        public.display()
    );
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes"))
}

fn stage(directory: &Path, bytes: &[u8]) -> Result<NamedTempFile> {
    let mut pending = NamedTempFile::new_in(directory)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    Ok(pending)
}

fn publish(pending: NamedTempFile, path: &Path, replace: bool) -> Result<()> {
    let result = if replace {
        pending.persist(path)
    } else {
        pending.persist_noclobber(path)
    };
    result.with_context(|| format!("Save {}", path.display()))?;
    Ok(())
}
