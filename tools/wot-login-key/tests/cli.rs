use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use rsa::{
    PublicKeyParts, RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey, DecodePublicKey},
};

fn run(directory: &Path, arguments: &[&str], answer: &str) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_wot-login-key"))
        .current_dir(directory)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(answer.as_bytes())
        .unwrap();
    process.wait_with_output().unwrap()
}

#[test]
fn requires_name_and_output_directory() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(directory.path(), &[], "");
    assert!(!output.status.success());
    let message = String::from_utf8(output.stderr).unwrap();
    assert!(message.contains("--name <NAME>"));
    assert!(message.contains("--output <DIRECTORY>"));
    let output = run(
        directory.path(),
        &["--name", "../training", "--output", ".local"],
        "",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected a base filename"));
    assert!(!directory.path().join(".local").exists());
}

#[test]
fn generates_matching_keys_and_requires_confirmation_even_if_only_one_exists() {
    let directory = tempfile::tempdir().unwrap();
    let arguments = [
        "--name",
        "training.local",
        "--output",
        ".local/Training Keys/",
    ];
    let output = run(directory.path(), &arguments, "");
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty());

    let private = directory
        .path()
        .join(".local/Training Keys/training.local.pem");
    let public = directory
        .path()
        .join(".local/Training Keys/training.local.pubkey");
    let old_private = fs::read_to_string(&private).unwrap();
    let old_public = fs::read_to_string(&public).unwrap();
    let private_key = RsaPrivateKey::from_pkcs8_pem(&old_private).unwrap();
    assert_eq!(private_key.n().bits(), 2048);
    assert_eq!(
        RsaPublicKey::from(&private_key),
        RsaPublicKey::from_public_key_pem(&old_public).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    fs::remove_file(&public).unwrap();
    for answer in ["", "n\n"] {
        let output = run(directory.path(), &arguments, answer);
        assert!(output.status.success(), "{:?}", output);
        assert!(String::from_utf8_lossy(&output.stderr).contains("[y/N]"));
        assert_eq!(fs::read_to_string(&private).unwrap(), old_private);
        assert!(!public.exists());
    }

    let output = run(directory.path(), &arguments, "yes\n");
    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("[y/N]"));
    let replacement_pem = fs::read_to_string(&private).unwrap();
    let replacement = RsaPrivateKey::from_pkcs8_pem(&replacement_pem).unwrap();
    let replacement_public =
        RsaPublicKey::from_public_key_pem(&fs::read_to_string(public).unwrap()).unwrap();
    assert_ne!(replacement_public, RsaPublicKey::from(&private_key));
    assert_eq!(replacement_public, RsaPublicKey::from(&replacement));
}
