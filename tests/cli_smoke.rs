use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn lock_binary() -> &'static str {
    env!("CARGO_BIN_EXE_lock")
}

fn write_manifest(dir: &TempDir, name: &str, jsonl: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, jsonl).expect("write manifest");
    path
}

fn run_lock(args: &[&str], ledger_path: Option<&Path>) -> Output {
    let mut cmd = Command::new(lock_binary());
    cmd.args(args);
    if let Some(path) = ledger_path {
        cmd.env("EPISTEMIC_WITNESS", path);
    }
    cmd.output().expect("run lock binary")
}

#[test]
fn smoke_main_lock_created_exit_0_and_json_contract() {
    let dir = tempfile::tempdir().unwrap();
    let input = write_manifest(
        &dir,
        "created.jsonl",
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10,"tool_versions":{"hash":"0.1.0"}}
"#,
    );

    let output = run_lock(&[input.to_str().unwrap(), "--no-witness"], None);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["version"], "lock.v0");
    assert_eq!(parsed["member_count"], 1);
    assert_eq!(parsed["skipped_count"], 0);
    assert!(parsed.get("refusal").is_none());
}

#[test]
fn smoke_main_lock_partial_exit_1_and_json_contract() {
    let dir = tempfile::tempdir().unwrap();
    let input = write_manifest(
        &dir,
        "partial.jsonl",
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
{"version":"vacuum.v0","relative_path":"b.csv","size":20,"_skipped":true,"_warnings":[{"tool":"vacuum","code":"W_SKIPPED","message":"fixture skip","detail":{}}]}
"#,
    );

    let output = run_lock(&[input.to_str().unwrap(), "--no-witness"], None);
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["version"], "lock.v0");
    assert_eq!(parsed["member_count"], 1);
    assert_eq!(parsed["skipped_count"], 1);
    assert_eq!(parsed["skipped"][0]["path"], "b.csv");
}

#[test]
fn smoke_main_refusal_exit_2_and_refusal_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let input = write_manifest(
        &dir,
        "refusal.jsonl",
        r#"{"version":"hash.v0","relative_path":"a.csv","size":10}
"#,
    );

    let output = run_lock(&[input.to_str().unwrap(), "--no-witness"], None);
    assert_eq!(output.status.code(), Some(2));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_MISSING_HASH");
}

#[test]
fn smoke_witness_query_empty_ledger_json_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("witness.jsonl");

    let output = run_lock(&["witness", "query", "--json"], Some(&ledger));
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed, serde_json::json!([]));
}

#[test]
fn smoke_witness_last_empty_ledger_json_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("witness.jsonl");

    let output = run_lock(&["witness", "last", "--json"], Some(&ledger));
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed.is_null());
}

#[test]
fn smoke_witness_count_json_returns_total() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("witness.jsonl");
    fs::write(
        &ledger,
        r#"{"tool":"lock","outcome":"LOCK_CREATED","ts":"2026-01-01T00:00:00Z"}
{"tool":"lock","outcome":"REFUSAL","ts":"2026-01-02T00:00:00Z"}
"#,
    )
    .unwrap();

    let output = run_lock(&["witness", "count", "--json"], Some(&ledger));
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["count"], 2);
}

#[test]
fn smoke_witness_query_outcome_filter_works() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("witness.jsonl");
    fs::write(
        &ledger,
        r#"{"tool":"lock","outcome":"LOCK_CREATED","ts":"2026-01-01T00:00:00Z"}
{"tool":"lock","outcome":"REFUSAL","ts":"2026-01-02T00:00:00Z"}
"#,
    )
    .unwrap();

    let output = run_lock(
        &["witness", "query", "--outcome", "REFUSAL", "--json"],
        Some(&ledger),
    );
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let items = parsed.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["outcome"], "REFUSAL");
}
