//! Integration tests for main command outcomes, refusal envelopes,
//! and deterministic ordering.
//!
//! bd-2ku: operator-facing confidence layer for lock.v0 behavior.
//! Tests exercise the full pipeline end-to-end using public APIs.

use std::collections::BTreeMap;
use std::io::Cursor;

use serde_json::{Value, json};

use lock::input::{ReadResult, read_jsonl_reader, validate_records};
use lock::lockfile::self_hash::{
    compute_lock_hash, to_canonical_json, verify_lock_hash, verify_lock_hash_from_json,
};
use lock::lockfile::{self, Lockfile};
use lock::output::{self, DomainOutcome};
use lock::refusal;

// ---------------------------------------------------------------------------
// Helper: run the full pipeline from JSONL string to rendered output
// ---------------------------------------------------------------------------

struct PipelineResult {
    outcome: DomainOutcome,
    json: String,
    parsed: Value,
}

fn run_pipeline(
    jsonl: &str,
    dataset_id: Option<&str>,
    as_of: Option<&str>,
    note: Option<&str>,
) -> PipelineResult {
    let result = read_jsonl_reader(Cursor::new(jsonl)).expect("JSONL should parse");
    let ReadResult::Records(records) = result else {
        panic!("expected records, got Empty");
    };

    validate_records(&records).expect("validation should pass");

    let classification =
        lockfile::classify_records(&records).expect("classification should succeed");
    let metadata = lockfile::hydrate_metadata(&records, "0.1.0", dataset_id, as_of, note);

    let mut lockfile = Lockfile {
        version: "lock.v0".to_owned(),
        lock_hash: String::new(),
        dataset_id: metadata.dataset_id,
        as_of: metadata.as_of,
        note: metadata.note,
        created: "2026-01-15T10:00:00Z".to_owned(),
        tool_versions: metadata.tool_versions,
        profiles: metadata.profiles,
        skipped: classification.skipped,
        members: classification.members,
        skipped_count: classification.skipped_count,
        member_count: classification.member_count,
    };

    lockfile.lock_hash = compute_lock_hash(&lockfile);
    let artifact = output::render_lockfile(&lockfile).expect("render should succeed");

    let parsed: Value =
        serde_json::from_str(&artifact.json).expect("rendered output should be valid JSON");

    PipelineResult {
        outcome: artifact.outcome,
        json: artifact.json,
        parsed,
    }
}

// ---------------------------------------------------------------------------
// LOCK_CREATED outcome (exit 0)
// ---------------------------------------------------------------------------

#[test]
fn lock_created_exit_code_is_zero() {
    assert_eq!(DomainOutcome::LockCreated.exit_code(), 0);
}

#[test]
fn lock_created_with_all_valid_records() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaa","size":100,"tool_versions":{"hash":"0.2.0"}}"#,
        "\n",
        r#"{"version":"hash.v0","relative_path":"b.csv","bytes_hash":"sha256:bbbb","size":200,"tool_versions":{"hash":"0.2.0"}}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, Some("test-dataset"), None, None);

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["version"], "lock.v0");
    assert_eq!(result.parsed["member_count"], 2);
    assert_eq!(result.parsed["skipped_count"], 0);
    assert_eq!(result.parsed["dataset_id"], "test-dataset");
    assert!(result.parsed["skipped"].as_array().unwrap().is_empty());
    assert_eq!(result.parsed["members"].as_array().unwrap().len(), 2);
}

#[test]
fn lock_created_self_hash_verifies() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"file.csv","bytes_hash":"sha256:aaaa","size":50}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let lock_hash = result.parsed["lock_hash"]
        .as_str()
        .expect("lock_hash must be string");
    assert!(
        lock_hash.starts_with("sha256:"),
        "lock_hash must start with sha256:"
    );

    // Verify from JSON
    assert!(
        verify_lock_hash_from_json(&result.json).expect("should parse"),
        "self-hash must verify from JSON output"
    );
}

#[test]
fn lock_created_includes_all_metadata_fields() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(
        &jsonl,
        Some("ds-alpha"),
        Some("2026-06-15T00:00:00Z"),
        Some("quarterly delivery"),
    );

    assert_eq!(result.parsed["dataset_id"], "ds-alpha");
    assert_eq!(result.parsed["as_of"], "2026-06-15T00:00:00Z");
    assert_eq!(result.parsed["note"], "quarterly delivery");
}

#[test]
fn lock_created_nullable_fields_are_null_when_omitted() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    assert!(result.parsed["dataset_id"].is_null());
    assert!(result.parsed["as_of"].is_null());
    assert!(result.parsed["note"].is_null());
}

#[test]
fn lock_created_profiles_always_empty_in_v0() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    assert!(result.parsed["profiles"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// LOCK_PARTIAL outcome (exit 1)
// ---------------------------------------------------------------------------

#[test]
fn lock_partial_exit_code_is_one() {
    assert_eq!(DomainOutcome::LockPartial.exit_code(), 1);
}

#[test]
fn lock_partial_with_skipped_records() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"good.csv","bytes_hash":"sha256:aaaa","size":100}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"bad.csv","_warnings":[{"tool":"hash","code":"E_IO","message":"cannot read"}]}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockPartial);
    assert_eq!(result.parsed["member_count"], 1);
    assert_eq!(result.parsed["skipped_count"], 1);
    assert_eq!(result.parsed["members"][0]["path"], "good.csv");
    assert_eq!(result.parsed["skipped"][0]["path"], "bad.csv");
}

#[test]
fn lock_partial_self_hash_verifies() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:aaaa","size":10}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"skip.csv","_warnings":[]}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    assert!(
        verify_lock_hash_from_json(&result.json).expect("should parse"),
        "partial lockfile self-hash must verify"
    );
}

#[test]
fn lock_partial_skipped_entry_carries_warnings() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:1234","size":1}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"warn.csv","_warnings":[{"tool":"hash","code":"E_IO","message":"denied","detail":{"errno":"13"}}]}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    let skipped = &result.parsed["skipped"][0];
    assert_eq!(skipped["path"], "warn.csv");
    let warning = &skipped["warnings"][0];
    assert_eq!(warning["tool"], "hash");
    assert_eq!(warning["code"], "E_IO");
    assert_eq!(warning["message"], "denied");
    assert_eq!(warning["detail"]["errno"], "13");
}

// ---------------------------------------------------------------------------
// REFUSAL outcome (exit 2)
// ---------------------------------------------------------------------------

#[test]
fn refusal_exit_code_is_two() {
    assert_eq!(DomainOutcome::Refusal.exit_code(), 2);
}

#[test]
fn refusal_envelope_e_empty() {
    let envelope = refusal::empty();
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(parsed["version"], "lock.v0");
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_EMPTY");
    assert!(
        parsed["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("no input")
    );
    assert!(parsed["refusal"]["detail"].as_object().unwrap().is_empty());
    assert!(parsed["refusal"]["next_command"].is_string());
}

#[test]
fn refusal_envelope_e_bad_input_parse() {
    let envelope = refusal::bad_input_parse(7, "unexpected token");
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(parsed["version"], "lock.v0");
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_BAD_INPUT");
    assert_eq!(parsed["refusal"]["detail"]["line"], 7);
    assert_eq!(parsed["refusal"]["detail"]["error"], "unexpected token");
    assert!(parsed["refusal"]["next_command"].is_null());
}

#[test]
fn refusal_envelope_e_bad_input_version() {
    let envelope = refusal::bad_input_version(3, "pack.v0");
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(parsed["refusal"]["code"], "E_BAD_INPUT");
    assert_eq!(parsed["refusal"]["detail"]["line"], 3);
    assert_eq!(parsed["refusal"]["detail"]["version"], "pack.v0");
    assert!(
        parsed["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("pack.v0")
    );
}

#[test]
fn refusal_envelope_e_missing_hash() {
    let envelope = refusal::missing_hash(
        3,
        vec!["a.csv".to_owned(), "b.csv".to_owned(), "c.csv".to_owned()],
    );
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(parsed["refusal"]["code"], "E_MISSING_HASH");
    assert_eq!(parsed["refusal"]["detail"]["count"], 3);
    let sample = parsed["refusal"]["detail"]["sample_paths"]
        .as_array()
        .expect("sample_paths must be array");
    assert_eq!(sample.len(), 3);
    assert!(parsed["refusal"]["next_command"].is_string());
}

#[test]
fn refusal_envelope_has_exactly_three_top_level_keys() {
    let envelope = refusal::empty();
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
    assert_eq!(keys, &["outcome", "refusal", "version"]);
}

#[test]
fn refusal_envelope_has_exactly_four_refusal_keys() {
    let envelope = refusal::empty();
    let json = envelope.to_json();
    let parsed: Value = serde_json::from_str(&json).expect("valid JSON");

    let keys: Vec<&String> = parsed["refusal"].as_object().unwrap().keys().collect();
    assert_eq!(keys, &["code", "detail", "message", "next_command"]);
}

// ---------------------------------------------------------------------------
// Deterministic ordering
// ---------------------------------------------------------------------------

#[test]
fn members_sorted_by_path_lexicographic() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"zebra.csv","bytes_hash":"sha256:zz","size":3}"#,
        "\n",
        r#"{"version":"hash.v0","relative_path":"alpha.csv","bytes_hash":"sha256:aa","size":1}"#,
        "\n",
        r#"{"version":"hash.v0","relative_path":"mango.csv","bytes_hash":"sha256:mm","size":2}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    let members = result.parsed["members"].as_array().unwrap();
    assert_eq!(members[0]["path"], "alpha.csv");
    assert_eq!(members[1]["path"], "mango.csv");
    assert_eq!(members[2]["path"], "zebra.csv");
}

#[test]
fn skipped_sorted_by_path_lexicographic() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:1234","size":1}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"z_skip.csv","_warnings":[]}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"a_skip.csv","_warnings":[]}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    let skipped = result.parsed["skipped"].as_array().unwrap();
    assert_eq!(skipped[0]["path"], "a_skip.csv");
    assert_eq!(skipped[1]["path"], "z_skip.csv");
}

#[test]
fn deterministic_output_for_same_input() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"b.csv","bytes_hash":"sha256:bb","size":2}"#,
        "\n",
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aa","size":1}"#,
        "\n",
    );

    let result1 = run_pipeline(jsonl, Some("ds"), None, None);
    let result2 = run_pipeline(jsonl, Some("ds"), None, None);

    assert_eq!(
        result1.json, result2.json,
        "same input must produce identical output"
    );
}

// ---------------------------------------------------------------------------
// Canonical JSON output properties
// ---------------------------------------------------------------------------

#[test]
fn output_has_sorted_top_level_keys() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let keys: Vec<&String> = result.parsed.as_object().unwrap().keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys, "top-level keys must be sorted");
}

#[test]
fn output_has_sorted_member_keys() {
    let jsonl = r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10,"fingerprint":{"fingerprint_id":"fp","fingerprint_version":"0.1.0","matched":true,"content_hash":"blake3:abcd"}}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let member_keys: Vec<&String> = result.parsed["members"][0]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    let mut sorted = member_keys.clone();
    sorted.sort();
    assert_eq!(member_keys, sorted, "member keys must be sorted");
}

#[test]
fn output_is_compact_json() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    assert!(
        !result.json.contains('\n'),
        "output must be compact (no newlines)"
    );
    assert!(
        !result.json.contains("  "),
        "output must be compact (no indentation)"
    );
    assert!(!result.json.ends_with('\n'), "no trailing newline");
}

// ---------------------------------------------------------------------------
// Self-hash integrity
// ---------------------------------------------------------------------------

#[test]
fn self_hash_round_trip_with_all_field_types() {
    let mut tool_versions = BTreeMap::new();
    tool_versions.insert("vacuum".to_owned(), "0.1.0".to_owned());
    tool_versions.insert("hash".to_owned(), "0.2.0".to_owned());
    tool_versions.insert("lock".to_owned(), "0.1.0".to_owned());

    let mut lockfile = Lockfile {
        version: "lock.v0".to_owned(),
        lock_hash: String::new(),
        dataset_id: Some("test-ds".to_owned()),
        as_of: Some("2026-01-01T00:00:00Z".to_owned()),
        note: Some("test note".to_owned()),
        created: "2026-01-15T10:00:00Z".to_owned(),
        tool_versions,
        profiles: vec![],
        skipped: vec![lockfile::SkippedEntry {
            path: "skipped.csv".to_owned(),
            warnings: vec![lockfile::Warning {
                tool: "hash".to_owned(),
                code: "E_IO".to_owned(),
                message: "cannot read".to_owned(),
                detail: BTreeMap::from([("reason".to_owned(), "permission denied".to_owned())]),
            }],
        }],
        members: vec![
            lockfile::Member {
                path: "alpha.csv".to_owned(),
                bytes_hash: "sha256:aaaa".to_owned(),
                size: 100,
                fingerprint: Some(lockfile::FingerprintResult {
                    fingerprint_id: "csv_v1".to_owned(),
                    fingerprint_version: "0.1.0".to_owned(),
                    matched: true,
                    content_hash: Some("blake3:cccc".to_owned()),
                }),
            },
            lockfile::Member {
                path: "beta.csv".to_owned(),
                bytes_hash: "sha256:bbbb".to_owned(),
                size: 200,
                fingerprint: None,
            },
        ],
        skipped_count: 1,
        member_count: 2,
    };

    lockfile.lock_hash = compute_lock_hash(&lockfile);

    // Verify via struct
    assert!(verify_lock_hash(&lockfile), "struct round-trip must verify");

    // Verify via JSON
    let json = to_canonical_json(&lockfile).expect("should serialize");
    assert!(
        verify_lock_hash_from_json(&json).expect("should parse"),
        "JSON round-trip must verify"
    );
}

#[test]
fn tampering_any_field_breaks_self_hash() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, Some("original"), None, None);

    // Tamper with dataset_id
    let mut tampered: Value = serde_json::from_str(&result.json).expect("valid JSON");
    tampered["dataset_id"] = json!("tampered");
    let tampered_json = serde_json::to_string(&tampered).expect("should serialize");
    assert!(
        !verify_lock_hash_from_json(&tampered_json).expect("should parse"),
        "tampering dataset_id must break verification"
    );

    // Tamper with member size
    let mut tampered2: Value = serde_json::from_str(&result.json).expect("valid JSON");
    tampered2["members"][0]["size"] = json!(999);
    let tampered_json2 = serde_json::to_string(&tampered2).expect("should serialize");
    assert!(
        !verify_lock_hash_from_json(&tampered_json2).expect("should parse"),
        "tampering member size must break verification"
    );
}

// ---------------------------------------------------------------------------
// Tool versions in output
// ---------------------------------------------------------------------------

#[test]
fn tool_versions_merged_from_input_records() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aa","size":1,"tool_versions":{"vacuum":"0.1.0","hash":"0.2.0"}}"#,
        "\n",
        r#"{"version":"fingerprint.v0","relative_path":"b.csv","bytes_hash":"sha256:bb","size":2,"tool_versions":{"fingerprint":"0.3.0"}}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    let tv = result.parsed["tool_versions"].as_object().unwrap();
    assert_eq!(tv.get("vacuum").and_then(Value::as_str), Some("0.1.0"));
    assert_eq!(tv.get("hash").and_then(Value::as_str), Some("0.2.0"));
    assert_eq!(tv.get("fingerprint").and_then(Value::as_str), Some("0.3.0"));
    assert_eq!(tv.get("lock").and_then(Value::as_str), Some("0.1.0"));
}

// ---------------------------------------------------------------------------
// run_lock exit codes via temp files
// ---------------------------------------------------------------------------

#[test]
fn run_lock_exit_0_for_valid_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.jsonl");
    std::fs::write(
        &path,
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#,
    )
    .unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(code, 0, "valid input should exit 0 (LOCK_CREATED)");
}

#[test]
fn run_lock_exit_1_for_partial_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:1234","size":10}"#,
            "\n",
            r#"{"version":"hash.v0","_skipped":true,"relative_path":"skip.csv","_warnings":[]}"#,
        ),
    )
    .unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(code, 1, "partial input should exit 1 (LOCK_PARTIAL)");
}

#[test]
fn run_lock_exit_2_for_missing_file() {
    let cli = lock::cli::Cli {
        command: None,
        input: Some("nonexistent-file.jsonl".into()),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(code, 2, "missing file should exit 2 (REFUSAL)");
}

#[test]
fn run_lock_exit_2_for_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.jsonl");
    std::fs::write(&path, "").unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(code, 2, "empty file should exit 2 (REFUSAL / E_EMPTY)");
}

#[test]
fn run_lock_exit_2_for_bad_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.jsonl");
    std::fs::write(
        &path,
        r#"{"version":"unknown.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#,
    )
    .unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(code, 2, "bad version should exit 2 (REFUSAL / E_BAD_INPUT)");
}

#[test]
fn run_lock_exit_2_for_missing_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-hash.jsonl");
    std::fs::write(
        &path,
        r#"{"version":"hash.v0","relative_path":"f.csv","size":10}"#,
    )
    .unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(
        code, 2,
        "missing hash should exit 2 (REFUSAL / E_MISSING_HASH)"
    );
}

#[test]
fn run_lock_exit_2_for_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.jsonl");
    std::fs::write(&path, "this is not json\n").unwrap();

    let cli = lock::cli::Cli {
        command: None,
        input: Some(path),
        dataset_id: None,
        as_of: None,
        note: None,
        no_witness: true,
        describe: false,
        schema: false,
    };

    let code = lock::run_lock(&cli);
    assert_eq!(
        code, 2,
        "invalid JSON should exit 2 (REFUSAL / E_BAD_INPUT)"
    );
}

// ---------------------------------------------------------------------------
// Schema-level structural assertions
// ---------------------------------------------------------------------------

#[test]
fn lockfile_output_has_all_required_schema_fields() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let required = [
        "version",
        "lock_hash",
        "created",
        "tool_versions",
        "profiles",
        "skipped",
        "members",
        "skipped_count",
        "member_count",
    ];

    let obj = result.parsed.as_object().expect("top level must be object");
    for field in &required {
        assert!(
            obj.contains_key(*field),
            "required field '{field}' must be present"
        );
    }
}

#[test]
fn member_has_required_fields() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":42}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let member = &result.parsed["members"][0];
    assert!(member["path"].is_string());
    assert!(member["bytes_hash"].is_string());
    assert!(member["size"].is_number());
}

#[test]
fn skipped_entry_has_required_fields() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:1234","size":1}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"s.csv","_warnings":[{"tool":"hash","code":"E_IO","message":"fail"}]}"#,
        "\n",
    );

    let result = run_pipeline(jsonl, None, None, None);

    let skipped = &result.parsed["skipped"][0];
    assert!(skipped["path"].is_string());
    assert!(skipped["warnings"].is_array());
    let warning = &skipped["warnings"][0];
    assert!(warning["tool"].is_string());
    assert!(warning["code"].is_string());
    assert!(warning["message"].is_string());
}

#[test]
fn lock_hash_matches_pattern() {
    let jsonl =
        r#"{"version":"hash.v0","relative_path":"f.csv","bytes_hash":"sha256:1234","size":10}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_pipeline(&jsonl, None, None, None);

    let hash = result.parsed["lock_hash"].as_str().unwrap();
    assert!(hash.starts_with("sha256:"), "must start with sha256:");
    let hex = &hash["sha256:".len()..];
    assert_eq!(hex.len(), 64, "SHA256 hex must be 64 chars");
    assert!(
        hex.chars().all(|c| c.is_ascii_hexdigit()),
        "must be valid hex"
    );
}
