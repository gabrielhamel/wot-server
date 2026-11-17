use crate::{command::Operation, selection::Selection};
use anyhow::Result;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

struct Workspace(TempDir);

impl Workspace {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn absolute(&self, name: &str) -> PathBuf {
        dunce::canonicalize(self.0.path()).unwrap().join(name)
    }

    fn selection(&self) -> Selection {
        Selection::new(self.0.path(), None)
    }

    fn fixture(&self, name: &str) -> PathBuf {
        let path = self.absolute(name);
        fs::create_dir_all(path.join("res/packages")).unwrap();
        fs::write(path.join("WorldOfTanks.exe"), b"abc").unwrap();
        path
    }

    fn config(&self) -> PathBuf {
        self.absolute(".local/game-client.json")
    }
}

fn offline(_: &[OsString]) -> Result<()> {
    panic!("This command must stay offline");
}

fn install(path: Option<&Path>) -> Operation {
    Operation::Install {
        output: path.map(Path::to_path_buf),
        region: "eu".into(),
        language: "en".into(),
        client_type: None,
    }
}

#[test]
fn default_and_registered_clients_report_identity_offline() {
    let workspace = Workspace::new();
    let selection = workspace.selection();
    let error = Operation::Path.run(&selection, offline).unwrap_err();
    assert!(error.to_string().contains("cargo game-client use"));

    let default = workspace.fixture(".local/game-client");
    assert_eq!(selection.resolve().unwrap(), default);
    let registered = workspace.fixture("World of Tanks");
    Operation::Use {
        path: registered.clone(),
    }
    .run(&selection, offline)
    .unwrap();
    assert_eq!(
        Operation::Path.run(&selection, offline).unwrap().as_deref(),
        registered.to_str(),
    );
    let output = Operation::Status { json: true }
        .run(&selection, offline)
        .unwrap()
        .unwrap();
    let status: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(status["path"], registered.to_str().unwrap());
    assert_eq!(
        status["executable"],
        registered.join("WorldOfTanks.exe").to_str().unwrap()
    );
    assert_eq!(
        status["resources"],
        registered.join("res").to_str().unwrap()
    );
    assert_eq!(
        status["executable_sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(status["version_file"].is_null());
    let version = registered.join("version.xml");
    fs::write(&version, "<version/>").unwrap();
    let output = Operation::Status { json: true }
        .run(&selection, offline)
        .unwrap()
        .unwrap();
    let status: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(status["version_file"], version.to_str().unwrap());
}

#[test]
fn selection_precedence_and_invalid_inputs_do_not_fall_back() {
    let workspace = Workspace::new();
    workspace.fixture(".local/game-client");
    let registered = workspace.fixture("registered");
    let selection = workspace.selection();
    Operation::Use {
        path: registered.clone(),
    }
    .run(&selection, offline)
    .unwrap();
    let wrong_path = workspace.absolute("missing");
    assert!(
        Operation::Use {
            path: wrong_path.clone()
        }
        .run(&selection, offline)
        .is_err()
    );
    assert_eq!(selection.resolve().unwrap(), registered);

    let other = workspace.fixture("other");
    let selected = Selection::new(workspace.0.path(), Some(other.clone().into_os_string()));
    assert_eq!(selected.resolve().unwrap(), other);
    let invalid = Selection::new(workspace.0.path(), Some(wrong_path.into_os_string()));
    assert!(Operation::Path.run(&invalid, offline).is_err());
    for bad in ["{", "[]", r#"["/"]"#, r#"{"path":"relative"}"#] {
        fs::write(workspace.config(), bad).unwrap();
        assert!(selection.resolve().is_err());
    }
}

#[test]
fn launch_commands_pass_paths_as_single_arguments_and_save_after_validation() {
    let workspace = Workspace::new();
    let selection = workspace.selection();
    let previous = workspace.fixture("old");
    selection.register(previous.clone()).unwrap();
    let output = workspace.absolute("new client $name");
    let run = install(Some(&output));
    assert!(
        run.run(&selection, |_| anyhow::bail!("launcher failed"))
            .is_err()
    );
    assert_eq!(selection.resolve().unwrap(), previous);
    assert!(install(Some(&output)).run(&selection, |_| Ok(())).is_err());
    assert_eq!(selection.resolve().unwrap(), previous);
    assert!(install(None).run(&selection, offline).is_err());

    install(Some(&output))
        .run(&selection, |arguments| {
            let expected = [
                "install",
                "--game",
                "wot",
                "--region",
                "eu",
                "--language",
                "en",
                "--output",
            ]
            .map(OsString::from);
            assert_eq!(
                arguments.split_last(),
                Some((&output.clone().into_os_string(), expected.as_slice()))
            );
            workspace.fixture("new client $name");
            Ok(())
        })
        .unwrap();
    assert_eq!(selection.resolve().unwrap(), output);
    for (operation, subcommand) in [
        (Operation::CheckUpdates, "status"),
        (Operation::Update, "update"),
    ] {
        operation
            .run(&selection, |arguments| {
                assert_eq!(
                    arguments,
                    [
                        subcommand.into(),
                        "--path".into(),
                        output.clone().into_os_string()
                    ]
                );
                Ok(())
            })
            .unwrap();
    }
}
