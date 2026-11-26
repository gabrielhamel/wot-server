use serde_json::Value;
use std::{path::PathBuf, process::Command};

#[test]
#[ignore = "requires the external supplied WorldOfTanks.exe; set WOT_GAME_DIR"]
fn supplied_client_regression_and_determinism() {
    let game = PathBuf::from(std::env::var_os("WOT_GAME_DIR").expect("set WOT_GAME_DIR"));
    let root = std::env::temp_dir().join(format!("wg-map-live-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let result = std::panic::catch_unwind(|| {
        for name in ["first", "second"] {
            let output = Command::new(env!("CARGO_BIN_EXE_wg-binary-map"))
                .arg(&game)
                .arg("--output")
                .arg(root.join(name))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        for file in ["structures.rs", "mapping.json"] {
            assert_eq!(
                std::fs::read(root.join("first").join(file)).unwrap(),
                std::fs::read(root.join("second").join(file)).unwrap()
            );
        }
        let report: Value =
            serde_json::from_slice(&std::fs::read(root.join("first/mapping.json")).unwrap())
                .unwrap();
        assert_eq!(
            report["sha256"],
            "58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11"
        );
        assert_eq!(report["native_types"].as_array().unwrap().len(), 20904);
        let interfaces = report["interfaces"].as_array().unwrap();
        for (name, count, last) in [
            ("ClientInterface", 80, 254),
            ("BaseAppExtInterface", 18, 254),
            ("CellAppExtInterface", 8, 7),
            ("LoginInterface", 5, 4),
        ] {
            let messages = interfaces.iter().find(|i| i["name"] == name).unwrap()["messages"]
                .as_array()
                .unwrap();
            assert_eq!(messages.len(), count);
            assert_eq!(messages.last().unwrap()["last_id"], last);
        }
        let client = interfaces
            .iter()
            .find(|i| i["name"] == "ClientInterface")
            .unwrap()["messages"]
            .as_array()
            .unwrap();
        let expected = [
            15, 13, 12, 11, 12, 10, 9, 8, 9, 7, 6, 5, 12, 10, 9, 8, 9, 7, 6, 5, 6, 4, 3, 2,
        ];
        for (message, length) in client[0x29..=0x40].iter().zip(expected) {
            assert_eq!(message["callback_length"]["bytes"], length);
        }
        assert_eq!(client[0x4e]["last_id"], 0xa6);
        assert_eq!(client[0x4f]["first_id"], 0xa7);
        let compiled = Command::new("rustc")
            .args(["--edition=2024", "--crate-type=lib"])
            .arg(root.join("first/structures.rs"))
            .arg("--out-dir")
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
    });
    std::fs::remove_dir_all(&root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
