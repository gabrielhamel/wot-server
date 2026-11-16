mod support;
use serde_json::Value;
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Work(PathBuf);
impl Work {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wg-map-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, bytes: &[u8]) -> std::process::Output {
        let executable = self.0.join("WorldOfTanks.exe");
        std::fs::write(&executable, bytes).unwrap();
        Command::new(env!("CARGO_BIN_EXE_wg-binary-map"))
            .arg(executable)
            .arg("--output")
            .arg(self.0.join("output"))
            .output()
            .unwrap()
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn maps_rebased_pe_initializer_order_ranges_rtti_and_callback_branches() {
    let work = Work::new();
    let fixture = support::image();
    let result = work.run(&fixture.bytes);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(work.0.join("output/mapping.json")).unwrap())
            .unwrap();
    assert_eq!(report["image_base"], 0x180000000u64);
    let messages = report["interfaces"][0]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["name"], "createBasePlayer");
    assert_eq!(messages[1]["framing"]["bytes"], 7);
    assert_eq!(messages[2]["first_id"], 2);
    assert_eq!(messages[2]["last_id"], 254);
    assert_eq!(messages[2]["callback_length"]["bytes"], 15);
    assert_eq!(report["native_types"][0]["name"], "class BW::Vehicle");
    assert_eq!(
        report["native_types"][0]["bases"][1]["name"],
        "class BW::Entity"
    );
    assert_eq!(
        report["native_types"][0]["bases"][1]["member_displacement"],
        16
    );
    assert_eq!(
        report["native_types"][0]["bases"][1]["vbtable_displacement"],
        -1
    );
    assert_eq!(report["native_types"][0]["vtables"][0]["object_offset"], 0);
    let compiled = Command::new("rustc")
        .args(["--edition=2024", "--crate-type=lib"])
        .arg(work.0.join("output/structures.rs"))
        .arg("--out-dir")
        .arg(&work.0)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let second = work.run(&fixture.bytes);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already exists"));
}

#[test]
fn rejects_truncated_wrong_architecture_and_unknown_registration_without_output() {
    let fixture = support::image();
    let mut wrong = fixture.bytes.clone();
    wrong[0x84] = 0x4c;
    wrong[0x85] = 1;
    let mut unknown = fixture.bytes.clone();
    unknown[fixture.registration_argument] = 0x90;
    for bytes in [&b"MZ"[..], &wrong, &unknown] {
        let work = Work::new();
        let result = work.run(bytes);
        assert!(!result.status.success());
        assert!(!work.0.join("output").exists());
    }
}

#[test]
fn unsupported_callback_is_left_unknown() {
    let mut fixture = support::image();
    fixture.bytes[0x2548] = 1;
    let work = Work::new();
    let result = work.run(&fixture.bytes);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(work.0.join("output/mapping.json")).unwrap())
            .unwrap();
    assert!(report["interfaces"][0]["messages"][2]["callback_length"].is_null());
}
