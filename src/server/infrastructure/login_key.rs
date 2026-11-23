use anyhow::{Context, Result};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(in crate::server) fn load_login_key(path: &Path) -> Result<Arc<RsaPrivateKey>> {
    let key = match std::fs::read_to_string(path) {
        Ok(pem) => RsaPrivateKey::from_pkcs8_pem(&pem).context("Decode login private key")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            use std::io::Write;
            let key = RsaPrivateKey::new(&mut OsRng, 2048).context("Generate login key")?;
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).context("Create login key directory")?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(path) {
                Ok(mut file) => {
                    file.write_all(key.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
                    file.sync_all()?;
                    key
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    RsaPrivateKey::from_pkcs8_pem(&std::fs::read_to_string(path)?)?
                }
                Err(error) => return Err(error).context("Create login private key"),
            }
        }
        Err(error) => return Err(error).context("Read login private key"),
    };
    let public_key = RsaPublicKey::from(&key).to_public_key_pem(LineEnding::LF)?;
    let mut public_path = path.with_extension("pubkey");
    if public_path == path {
        let mut filename = path.as_os_str().to_owned();
        filename.push(".pubkey");
        public_path = PathBuf::from(filename);
    }
    if std::fs::read(&public_path).ok().as_deref() != Some(public_key.as_bytes()) {
        std::fs::write(&public_path, public_key).context("Export login public key")?;
    }
    Ok(Arc::new(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_reuses_existing_private_key_and_exports_matching_public_key() {
        use rsa::pkcs8::DecodePublicKey;
        let directory = std::env::temp_dir().join(format!(
            "wot-server-key-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("login.pem");
        let fixture = RsaPrivateKey::new(&mut OsRng, 512)
            .unwrap()
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap();
        std::fs::write(&path, fixture.as_bytes()).unwrap();
        let first = load_login_key(&path).unwrap();
        let second = load_login_key(&path).unwrap();
        assert_eq!(*first, *second);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), *fixture);
        let public = RsaPublicKey::from_public_key_pem(
            &std::fs::read_to_string(path.with_extension("pubkey")).unwrap(),
        )
        .unwrap();
        assert_eq!(public, RsaPublicKey::from(&*first));
        let alternate_path = directory.join("private.pubkey");
        std::fs::write(&alternate_path, fixture.as_bytes()).unwrap();
        load_login_key(&alternate_path).unwrap();
        assert_eq!(std::fs::read_to_string(&alternate_path).unwrap(), *fixture);
        assert!(directory.join("private.pubkey.pubkey").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
