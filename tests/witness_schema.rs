use std::fs;
use std::process::Command;

use serde_json::Value;

fn lock_binary() -> &'static str {
    env!("CARGO_BIN_EXE_lock")
}

fn schema() -> Value {
    serde_json::from_str(include_str!("../schemas/witness-v0.schema.json"))
        .expect("schema must be valid json")
}

fn validate(instance: &Value) -> Result<(), String> {
    let validator = jsonschema::validator_for(&schema()).expect("schema should compile");
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn run_and_read_single_witness_record(manifest_jsonl: &str) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("input.jsonl");
    let ledger_path = dir.path().join("witness.jsonl");

    fs::write(&manifest_path, manifest_jsonl).unwrap();

    let output = Command::new(lock_binary())
        .arg(manifest_path.to_str().unwrap())
        .env("EPISTEMIC_WITNESS", &ledger_path)
        .output()
        .expect("run lock");

    let code = output.status.code().expect("process exit code");

    let content = fs::read_to_string(&ledger_path).expect("witness ledger should exist");
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let line = lines
        .next()
        .expect("witness ledger should contain one record");
    assert!(
        lines.next().is_none(),
        "expected exactly one witness record"
    );

    let value: Value = serde_json::from_str(line).expect("witness line should parse as json");
    (code, value)
}

#[test]
fn schema_file_is_valid_and_compiles() {
    let _ = jsonschema::validator_for(&schema()).expect("schema should compile");
}

#[test]
fn schema_validates_lock_created_witness_record() {
    let (code, record) = run_and_read_single_witness_record(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
"#,
    );
    assert_eq!(code, 0);
    validate(&record).expect("LOCK_CREATED witness should validate");
}

#[test]
fn schema_validates_refusal_witness_record() {
    let (code, record) = run_and_read_single_witness_record(
        r#"{"version":"hash.v0","relative_path":"a.csv","size":10}
"#,
    );
    assert_eq!(code, 2);
    assert_eq!(record["outcome"], "REFUSAL");
    validate(&record).expect("REFUSAL witness should validate");
}

#[test]
fn schema_rejects_missing_required_id() {
    let (_code, mut record) = run_and_read_single_witness_record(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
"#,
    );
    record.as_object_mut().unwrap().remove("id");

    assert!(validate(&record).is_err(), "missing id must fail");
}

#[test]
fn schema_rejects_invalid_outcome_value() {
    let (_code, mut record) = run_and_read_single_witness_record(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
"#,
    );
    record["outcome"] = serde_json::json!("REAL_CHANGE");

    assert!(validate(&record).is_err(), "invalid outcome must fail");
}

#[test]
fn schema_rejects_invalid_hash_format() {
    let (_code, mut record) = run_and_read_single_witness_record(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10}
"#,
    );
    record["output_hash"] = serde_json::json!("sha256:not-blake3");

    assert!(validate(&record).is_err(), "invalid output_hash must fail");
}
