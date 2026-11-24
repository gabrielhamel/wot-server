use std::{fs, path::Path, process::Command};

use wot_platform::client_formats::pxml;

const CONFIG: &[u8] = include_bytes!("../../../docs/public-server/scripts_config.xml");

fn run(operation: &str, game: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wot-server-list"))
        .arg(operation)
        .arg("--game-dir")
        .arg(game)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn inserts_lists_and_deletes_an_entry_without_touching_key_files() {
    let game = tempfile::tempdir().unwrap();
    let res = game.path().join("res");
    fs::create_dir(&res).unwrap();
    let config = res.join("scripts_config.xml");
    fs::write(&config, CONFIG).unwrap();

    let args = [
        "--periphery-id",
        "251",
        "--address",
        "192.0.2.10:20016",
        "--name",
        "Training Server",
        "--short-name",
        "TRAINING",
        "--key-name",
        "training.pubkey",
    ];
    let output = run("insert", game.path(), &args);
    assert!(output.status.success(), "{:?}", output);
    let updated = fs::read(&config).unwrap();
    let original = pxml::from_bytes(CONFIG).unwrap();
    let edited = pxml::from_bytes(&updated).unwrap();
    let original_login = original.get_child_element("login").unwrap();
    let edited_login = edited.get_child_element("login").unwrap();
    assert_eq!(edited_login.len(), original_login.len() + 1);
    for (previous, current) in original_login
        .iter_children_all()
        .zip(edited_login.iter_children_all())
    {
        assert_eq!(format!("{previous:?}"), format!("{current:?}"));
    }
    let host = edited_login
        .iter_children("host")
        .next_back()
        .unwrap()
        .as_element()
        .unwrap();
    assert_eq!(host.get_child_string("name"), Some("Training Server"));
    assert_eq!(host.get_child_string("short_name"), Some("TRAINING"));
    assert_eq!(host.get_child_string("url"), Some("192.0.2.10:20016"));
    assert_eq!(host.get_child_string("url_token"), Some("192.0.2.10:20016"));
    assert_eq!(host.get_child_integer("periphery_id"), Some(251));
    assert_eq!(
        host.get_child_string("public_key_path"),
        Some("training.pubkey")
    );
    let key = res.join("training.pubkey");
    assert!(!key.exists());

    fs::write(&key, b"manually installed key").unwrap();
    let output = run("insert", game.path(), &args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Periphery ID 251 is already used"));
    assert_eq!(fs::read(&config).unwrap(), updated);

    let output = run("list", game.path(), &[]);
    assert!(output.status.success(), "{:?}", output);
    let listed = String::from_utf8(output.stdout).unwrap();
    assert!(listed.lines().any(|line| line == "251\tTRAINING\t192.0.2.10:20016\ttraining.pubkey\tTraining Server"));
    assert!(listed.contains("WOTSERVER"));
    assert_eq!(fs::read(&config).unwrap(), updated);

    let output = run("delete", game.path(), &["--periphery-id", "251"]);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(fs::read(key).unwrap(), b"manually installed key");
    let output = run("list", game.path(), &[]);
    assert!(output.status.success(), "{:?}", output);
    let listed = String::from_utf8(output.stdout).unwrap();
    assert!(!listed.contains("TRAINING"));
    assert!(listed.contains("WOTSERVER"));

    let deleted = fs::read(&config).unwrap();
    let edited = pxml::from_bytes(&deleted).unwrap();
    assert_eq!(format!("{edited:?}"), format!("{original:?}"));
    let output = run("delete", game.path(), &["--periphery-id", "251"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No server has periphery ID 251"));
    assert_eq!(fs::read(&config).unwrap(), deleted);
}

#[test]
fn rejects_conflicting_ids_and_malformed_configs_without_replacing_files() {
    let game = tempfile::tempdir().unwrap();
    let res = game.path().join("res");
    fs::create_dir(&res).unwrap();
    let config = res.join("scripts_config.xml");
    fs::write(&config, CONFIG).unwrap();
    let output = run("insert", game.path(), &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Periphery ID 250 is already used"));
    assert_eq!(fs::read(&config).unwrap(), CONFIG);

    let output = run(
        "insert",
        game.path(),
        &["--periphery-id", "251", "--key-name", "../outside.pubkey"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--key-name requires a filename"));
    assert_eq!(fs::read(&config).unwrap(), CONFIG);

    let mut corrupt = pxml::MAGIC.to_vec();
    let empty_string_descriptor = 0x1000_0000_u32.to_le_bytes();
    corrupt.extend_from_slice(&[0, 0]); // Header byte and empty name dictionary.
    corrupt.extend_from_slice(&1_u16.to_le_bytes());
    corrupt.extend_from_slice(&empty_string_descriptor);
    corrupt.extend_from_slice(&0_u16.to_le_bytes());
    corrupt.extend_from_slice(&empty_string_descriptor);
    fs::write(&config, &corrupt).unwrap();
    let output = run("insert", game.path(), &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid element name index"));
    assert_eq!(fs::read(&config).unwrap(), corrupt);
}
