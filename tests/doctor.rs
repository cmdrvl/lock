use std::{path::Path, process::Command};

use serde_json::Value;
use tempfile::TempDir;

fn isolated_command(home: &Path, witness_path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lock"));
    command.env("HOME", home);
    command.env("USERPROFILE", home);
    command.env("EPISTEMIC_WITNESS", witness_path);
    command
}

#[test]
fn doctor_health_json_exits_zero_without_writing_witness() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .args(["doctor", "health", "--json"])
        .output()
        .expect("lock doctor should run");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "doctor health should not emit stderr"
    );
    assert!(
        !witness_path.exists(),
        "doctor health must not append or create the witness ledger"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("doctor health should emit JSON");
    assert_eq!(report["schema_version"], "lock.doctor.health.v1");
    assert_eq!(report["tool"], "lock");
    assert_eq!(report["read_only"], true);
    assert_eq!(report["ok"], true);
    assert_eq!(report["fixers"], serde_json::json!([]));
}

#[test]
fn doctor_capabilities_json_advertises_no_fixers() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .args(["doctor", "capabilities", "--json"])
        .output()
        .expect("lock doctor should run");

    assert_eq!(output.status.code(), Some(0));
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("capabilities should emit JSON");

    assert_eq!(report["schema_version"], "lock.doctor.capabilities.v1");
    assert_eq!(report["read_only"], true);
    assert_eq!(report["side_effects"]["writes_witness_ledger"], false);
    assert_eq!(report["side_effects"]["reads_stdin"], false);
    assert_eq!(report["side_effects"]["creates_lockfiles"], false);
    assert_eq!(report["fix_mode"]["status"], "not_available");
    assert_eq!(report["fixers"], serde_json::json!([]));
}

#[test]
fn doctor_robot_triage_json_is_machine_readable() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .args(["doctor", "--robot-triage"])
        .output()
        .expect("lock doctor should run");

    assert_eq!(output.status.code(), Some(0));
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("robot triage should emit JSON");

    assert_eq!(report["schema_version"], "lock.doctor.triage.v1");
    assert_eq!(report["ok"], true);
    assert_eq!(report["health"]["schema_version"], "lock.doctor.health.v1");
    assert_eq!(
        report["capabilities"]["schema_version"],
        "lock.doctor.capabilities.v1"
    );
}

#[test]
fn doctor_fix_is_not_available() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .args(["doctor", "--fix"])
        .output()
        .expect("lock doctor should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "unknown doctor flags should not emit stdout"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(
        stderr.contains("unexpected argument '--fix'"),
        "stderr should explain that --fix is unavailable: {stderr}"
    );
    assert!(
        !witness_path.exists(),
        "unavailable fix mode must not create witness state"
    );
}
