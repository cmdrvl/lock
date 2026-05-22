use std::{path::Path, process::Command};

use serde_json::Value;
use tempfile::TempDir;

mod support;

fn isolated_command(home: &Path, witness_path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lock"));
    command.env("HOME", home);
    command.env("USERPROFILE", home);
    command.env("EPISTEMIC_WITNESS", witness_path);
    command
}

fn parse_stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
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

#[test]
fn describe_runs_without_guard_hooks() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .arg("--describe")
        .output()
        .expect("lock --describe should run");

    assert_eq!(output.status.code(), Some(0));
    let report = parse_stdout_json(&output);
    assert_eq!(report["name"], "lock");
    assert!(
        !witness_path.exists(),
        "--describe must not append or create the witness ledger"
    );
}

#[test]
fn domain_lock_fails_closed_without_guard_hooks() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .arg("--no-witness")
        .output()
        .expect("lock domain command should run");

    assert_eq!(output.status.code(), Some(2));
    let refusal = parse_stdout_json(&output);
    assert_eq!(refusal["refusal"]["code"], "E_GUARD_PREFLIGHT");
    assert!(
        refusal["refusal"]["detail"]["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| finding
                .as_str()
                .is_some_and(|finding| finding.contains("dcg Bash hook is missing"))))
    );
    assert!(
        !witness_path.exists(),
        "guard refusal must not append or create the witness ledger"
    );
}

#[test]
fn domain_lock_fails_closed_with_invalid_dcg_hook() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    support::write_guard_hooks(home.path(), "/definitely/missing/dcg");

    let output = isolated_command(home.path(), &witness_path)
        .arg("--no-witness")
        .output()
        .expect("lock domain command should run");

    assert_eq!(output.status.code(), Some(2));
    let refusal = parse_stdout_json(&output);
    assert_eq!(refusal["refusal"]["code"], "E_GUARD_PREFLIGHT");
    assert!(
        refusal["refusal"]["detail"]["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| finding
                .as_str()
                .is_some_and(|finding| finding.contains("dcg Bash hook command"))))
    );
    assert!(
        !witness_path.exists(),
        "guard refusal must not append or create the witness ledger"
    );
}

#[test]
fn domain_lock_runs_when_guard_hooks_are_healthy() {
    let home = TempDir::new().expect("temp home should be created");
    let manifest_path = home.path().join("input.jsonl");
    let witness_path = home.path().join("witness.jsonl");
    support::write_healthy_guard_hooks(home.path());
    std::fs::write(
        &manifest_path,
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
"#,
    )
    .expect("manifest should be writable");

    let output = isolated_command(home.path(), &witness_path)
        .args([manifest_path.to_str().unwrap(), "--no-witness"])
        .output()
        .expect("lock domain command should run");

    assert_eq!(output.status.code(), Some(0));
    let lockfile = parse_stdout_json(&output);
    assert_eq!(lockfile["version"], "lock.v0");
    assert_eq!(lockfile["member_count"], 1);
    assert!(
        !witness_path.exists(),
        "--no-witness must suppress witness append"
    );
}

#[test]
fn verify_fails_closed_without_guard_hooks() {
    let home = TempDir::new().expect("temp home should be created");
    let witness_path = home.path().join("witness.jsonl");
    let output = isolated_command(home.path(), &witness_path)
        .args([
            "verify",
            "/nonexistent/file.lock.json",
            "--json",
            "--no-witness",
        ])
        .output()
        .expect("lock verify should run");

    assert_eq!(output.status.code(), Some(2));
    let refusal = parse_stdout_json(&output);
    assert_eq!(refusal["refusal"]["code"], "E_GUARD_PREFLIGHT");
    assert!(
        !witness_path.exists(),
        "guard refusal must not append or create the witness ledger"
    );
}
