//! End-to-end spine compatibility fixtures.
//!
//! bd-7ot: Verify lock integrates with realistic upstream record formats
//! from the vacuum | hash | fingerprint pipeline.
//!
//! These tests use representative JSONL that mirrors real tool output to
//! confirm lock handles the full field set correctly.

use std::io::Cursor;

use serde_json::{Value, json};

use lock::input::{ReadResult, read_jsonl_reader, validate_records};
use lock::lockfile::self_hash::{compute_lock_hash, verify_lock_hash_from_json};
use lock::lockfile::{self, Lockfile};
use lock::output::{self, DomainOutcome};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

struct SpineResult {
    outcome: DomainOutcome,
    json: String,
    parsed: Value,
}

fn run_spine(
    jsonl: &str,
    dataset_id: Option<&str>,
    as_of: Option<&str>,
    note: Option<&str>,
) -> SpineResult {
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
        created: "2026-01-15T10:30:00Z".to_owned(),
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

    SpineResult {
        outcome: artifact.outcome,
        json: artifact.json,
        parsed,
    }
}

// ---------------------------------------------------------------------------
// Fixture: vacuum.v0 records (raw scan, no hash yet → lock refuses)
// ---------------------------------------------------------------------------

#[test]
fn vacuum_only_records_refuse_missing_hash() {
    let jsonl = concat!(
        r#"{"version":"vacuum.v0","path":"/data/file1.csv","relative_path":"file1.csv","root":"/data","size":1024,"mtime":"2026-01-15T10:30:00Z","extension":".csv","mime_guess":"text/csv","tool_versions":{"vacuum":"0.1.0"}}"#,
        "\n",
        r#"{"version":"vacuum.v0","path":"/data/file2.csv","relative_path":"file2.csv","root":"/data","size":2048,"mtime":"2026-01-15T11:00:00Z","extension":".csv","mime_guess":"text/csv","tool_versions":{"vacuum":"0.1.0"}}"#,
        "\n",
    );

    let result = read_jsonl_reader(Cursor::new(jsonl)).expect("should parse");
    let ReadResult::Records(records) = result else {
        panic!("expected records");
    };

    let err = validate_records(&records).expect_err("vacuum-only records lack bytes_hash");
    let lock::input::ValidationError::MissingHash(detail) = err else {
        panic!("expected MissingHash");
    };
    assert_eq!(detail.count, 2);
}

// ---------------------------------------------------------------------------
// Fixture: hash.v0 records (full pipeline without fingerprint)
// ---------------------------------------------------------------------------

#[test]
fn hash_v0_pipeline_creates_lock() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","path":"/data/tape.csv","relative_path":"tape.csv","size":847201,"bytes_hash":"sha256:7d865e959b2466918c9863afca942d0fb95d52c8f5d8c5e3f4a1b2c3d4e5f6a7","hash_algorithm":"sha256","extension":".csv","mime_guess":"text/csv","mtime":"2026-01-10T08:00:00Z","tool_versions":{"vacuum":"0.1.0","hash":"0.1.0"}}"#,
        "\n",
        r#"{"version":"hash.v0","path":"/data/model.xlsx","relative_path":"model.xlsx","size":2481920,"bytes_hash":"sha256:e3b0c44298fc1c149afbf4c8996fb924427ae41e4649b934ca495991b7852b85","hash_algorithm":"sha256","extension":".xlsx","mime_guess":"application/vnd.ms-excel","mtime":"2026-01-12T14:30:00Z","tool_versions":{"vacuum":"0.1.0","hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(
        jsonl,
        Some("raw-dec-2025"),
        Some("2025-12-31T23:59:59Z"),
        Some("Q4 delivery"),
    );

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["member_count"], 2);
    assert_eq!(result.parsed["skipped_count"], 0);

    // Members sorted by path.
    assert_eq!(result.parsed["members"][0]["path"], "model.xlsx");
    assert_eq!(result.parsed["members"][1]["path"], "tape.csv");

    // Metadata propagated.
    assert_eq!(result.parsed["dataset_id"], "raw-dec-2025");
    assert_eq!(result.parsed["as_of"], "2025-12-31T23:59:59Z");
    assert_eq!(result.parsed["note"], "Q4 delivery");

    // Tool versions merged.
    let tv = result.parsed["tool_versions"].as_object().unwrap();
    assert_eq!(tv.get("vacuum").and_then(Value::as_str), Some("0.1.0"));
    assert_eq!(tv.get("hash").and_then(Value::as_str), Some("0.1.0"));
    assert_eq!(tv.get("lock").and_then(Value::as_str), Some("0.1.0"));

    // No fingerprint for hash-only pipeline.
    assert!(result.parsed["members"][0]["fingerprint"].is_null());
    assert!(result.parsed["members"][1]["fingerprint"].is_null());

    // Self-hash valid.
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}

// ---------------------------------------------------------------------------
// Fixture: fingerprint.v0 records (full 3-tool pipeline)
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_vacuum_hash_fingerprint_creates_lock() {
    let jsonl = concat!(
        r#"{"version":"fingerprint.v0","path":"/data/argus/model_q4.xlsx","relative_path":"argus/model_q4.xlsx","size":2481920,"bytes_hash":"sha256:e3b0c44298fc1c149afbf4c8996fb924427ae41e4649b934ca495991b7852b85","extension":".xlsx","mime_guess":"application/vnd.ms-excel","fingerprint":{"fingerprint_id":"argus-model.v1","fingerprint_version":"0.3.2","matched":true,"content_hash":"blake3:9f2a3d5c8e1b9a7f4c6e2d8b5a9f3c1e"},"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.3.2"}}"#,
        "\n",
        r#"{"version":"fingerprint.v0","path":"/data/argus/tape_q4.csv","relative_path":"argus/tape_q4.csv","size":847201,"bytes_hash":"sha256:7d865e959b2466918c9863afca942d0fb95d52c8f5d8c5e3f4a1b2c3d4e5f6a7","extension":".csv","mime_guess":"text/csv","fingerprint":{"fingerprint_id":"csv_header_v1","fingerprint_version":"0.3.2","matched":false,"content_hash":null},"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.3.2"}}"#,
        "\n",
        r#"{"version":"fingerprint.v0","path":"/data/argus/readme.txt","relative_path":"argus/readme.txt","size":512,"bytes_hash":"sha256:abc123def456789012345678901234567890123456789012345678901234abcd","extension":".txt","mime_guess":"text/plain","fingerprint":{"fingerprint_id":"csv_header_v1","fingerprint_version":"0.3.2","matched":false,"content_hash":null},"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.3.2"}}"#,
        "\n",
    );

    let result = run_spine(
        jsonl,
        Some("argus-models-2025-12"),
        Some("2025-12-31T23:59:59Z"),
        Some("Q4 2025 final delivery"),
    );

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["member_count"], 3);
    assert_eq!(result.parsed["skipped_count"], 0);

    // Members sorted by path (lexicographic).
    let paths: Vec<&str> = result.parsed["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec![
            "argus/model_q4.xlsx",
            "argus/readme.txt",
            "argus/tape_q4.csv"
        ]
    );

    // First member has matched fingerprint.
    let fp0 = &result.parsed["members"][0]["fingerprint"];
    assert_eq!(fp0["fingerprint_id"], "argus-model.v1");
    assert_eq!(fp0["fingerprint_version"], "0.3.2");
    assert_eq!(fp0["matched"], true);
    assert!(fp0["content_hash"].is_string());

    // Second member has non-matching fingerprint.
    let fp1 = &result.parsed["members"][1]["fingerprint"];
    assert_eq!(fp1["matched"], false);
    assert!(fp1["content_hash"].is_null());

    // All three tool versions plus lock.
    let tv = result.parsed["tool_versions"].as_object().unwrap();
    assert_eq!(tv.len(), 4);
    assert_eq!(tv.get("vacuum").and_then(Value::as_str), Some("0.1.0"));
    assert_eq!(tv.get("hash").and_then(Value::as_str), Some("0.1.0"));
    assert_eq!(tv.get("fingerprint").and_then(Value::as_str), Some("0.3.2"));
    assert_eq!(tv.get("lock").and_then(Value::as_str), Some("0.1.0"));

    // Self-hash valid.
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}

// ---------------------------------------------------------------------------
// Fixture: mixed pipeline with skipped records (LOCK_PARTIAL)
// ---------------------------------------------------------------------------

#[test]
fn mixed_pipeline_with_skips_creates_partial_lock() {
    let jsonl = concat!(
        // Normal record from full pipeline.
        r#"{"version":"fingerprint.v0","relative_path":"data/model.xlsx","bytes_hash":"sha256:e3b0c44298fc1c149afbf4c8996fb924427ae41e4649b934ca495991b7852b85","size":2481920,"fingerprint":{"fingerprint_id":"argus-model.v1","fingerprint_version":"0.3.2","matched":true,"content_hash":"blake3:9f2a3d5c"},"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.3.2"}}"#,
        "\n",
        // Skipped record: hash failed (permission denied).
        r#"{"version":"hash.v0","relative_path":"data/secret.key","_skipped":true,"_warnings":[{"tool":"hash","code":"E_IO","message":"Cannot read file: permission denied","detail":{"errno":"13","path":"/data/secret.key"}}],"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0"}}"#,
        "\n",
        // Skipped record: fingerprint timeout.
        r#"{"version":"fingerprint.v0","relative_path":"data/huge.bin","_skipped":true,"_warnings":[{"tool":"fingerprint","code":"E_TIMEOUT","message":"Fingerprint scan timed out after 30s","detail":{"timeout_ms":"30000"}}],"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0","fingerprint":"0.3.2"}}"#,
        "\n",
        // Normal record from hash-only pipeline.
        r#"{"version":"hash.v0","relative_path":"data/tape.csv","bytes_hash":"sha256:7d865e959b2466918c9863afca942d0fb95d52c8f5d8c5e3f4a1b2c3d4e5f6a7","size":847201,"tool_versions":{"vacuum":"0.1.0","hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, Some("mixed-dataset"), None, None);

    assert_eq!(result.outcome, DomainOutcome::LockPartial);
    assert_eq!(result.parsed["member_count"], 2);
    assert_eq!(result.parsed["skipped_count"], 2);

    // Members sorted by path.
    assert_eq!(result.parsed["members"][0]["path"], "data/model.xlsx");
    assert_eq!(result.parsed["members"][1]["path"], "data/tape.csv");

    // Skipped sorted by path.
    assert_eq!(result.parsed["skipped"][0]["path"], "data/huge.bin");
    assert_eq!(result.parsed["skipped"][1]["path"], "data/secret.key");

    // Warnings preserved.
    let warn0 = &result.parsed["skipped"][0]["warnings"][0];
    assert_eq!(warn0["tool"], "fingerprint");
    assert_eq!(warn0["code"], "E_TIMEOUT");
    assert_eq!(warn0["detail"]["timeout_ms"], "30000");

    let warn1 = &result.parsed["skipped"][1]["warnings"][0];
    assert_eq!(warn1["tool"], "hash");
    assert_eq!(warn1["code"], "E_IO");
    assert_eq!(warn1["detail"]["errno"], "13");

    // Tool versions merged from all records (including skipped).
    let tv = result.parsed["tool_versions"].as_object().unwrap();
    assert_eq!(tv.get("fingerprint").and_then(Value::as_str), Some("0.3.2"));

    // Self-hash valid.
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}

// ---------------------------------------------------------------------------
// Fixture: mixed record versions from different pipeline stages
// ---------------------------------------------------------------------------

#[test]
fn records_with_mixed_versions_accepted() {
    // Real scenario: some files only went through hash, others through fingerprint.
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"fast.csv","bytes_hash":"sha256:aaaa","size":100,"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
        r#"{"version":"fingerprint.v0","relative_path":"slow.xlsx","bytes_hash":"sha256:bbbb","size":200,"fingerprint":{"fingerprint_id":"fp1","fingerprint_version":"0.2.0","matched":true,"content_hash":"blake3:cccc"},"tool_versions":{"hash":"0.1.0","fingerprint":"0.2.0"}}"#,
        "\n",
        r#"{"version":"vacuum.v0","relative_path":"raw.txt","bytes_hash":"sha256:dddd","size":50,"tool_versions":{"vacuum":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["member_count"], 3);

    // All versions accepted.
    let paths: Vec<&str> = result.parsed["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["fast.csv", "raw.txt", "slow.xlsx"]);

    // Tool versions from all records merged.
    let tv = result.parsed["tool_versions"].as_object().unwrap();
    assert!(tv.contains_key("hash"));
    assert!(tv.contains_key("fingerprint"));
    assert!(tv.contains_key("vacuum"));
    assert!(tv.contains_key("lock"));
}

// ---------------------------------------------------------------------------
// Fixture: large dataset simulation (many records)
// ---------------------------------------------------------------------------

#[test]
fn large_dataset_deterministic_output() {
    // Simulate 50 records from a full pipeline.
    let mut lines = Vec::new();
    for i in 0..50 {
        let record = json!({
            "version": "hash.v0",
            "relative_path": format!("data/file_{:03}.csv", i),
            "bytes_hash": format!("sha256:{:064x}", i),
            "size": 1000 + i,
            "tool_versions": {"vacuum": "0.1.0", "hash": "0.1.0"}
        });
        lines.push(serde_json::to_string(&record).unwrap());
    }
    let jsonl = lines.join("\n") + "\n";

    let result1 = run_spine(&jsonl, Some("bulk"), None, None);
    let result2 = run_spine(&jsonl, Some("bulk"), None, None);

    assert_eq!(result1.outcome, DomainOutcome::LockCreated);
    assert_eq!(result1.parsed["member_count"], 50);
    assert_eq!(
        result1.json, result2.json,
        "large dataset must be deterministic"
    );
    assert!(verify_lock_hash_from_json(&result1.json).expect("should parse"));

    // Verify sorted order.
    let paths: Vec<&str> = result1.parsed["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    let mut sorted_paths = paths.clone();
    sorted_paths.sort();
    assert_eq!(paths, sorted_paths, "members must be sorted by path");
}

// ---------------------------------------------------------------------------
// Fixture: extra upstream fields silently ignored
// ---------------------------------------------------------------------------

#[test]
fn extra_upstream_fields_do_not_break_lock() {
    // Records contain fields lock doesn't use (root, mtime, extension, etc.).
    let jsonl = concat!(
        r#"{"version":"hash.v0","path":"/scan/root/data.csv","relative_path":"data.csv","root":"/scan/root","size":1024,"bytes_hash":"sha256:abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234","hash_algorithm":"sha256","extension":".csv","mime_guess":"text/csv","mtime":"2026-01-15T10:30:00Z","custom_field":"extra_value","tool_versions":{"vacuum":"0.1.0","hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["member_count"], 1);
    assert_eq!(result.parsed["members"][0]["path"], "data.csv");
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}

// ---------------------------------------------------------------------------
// Fixture: relative_path preferred over path for member key
// ---------------------------------------------------------------------------

#[test]
fn relative_path_preferred_over_absolute_path() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","path":"/absolute/long/path/file.csv","relative_path":"file.csv","bytes_hash":"sha256:1234","size":10,"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(
        result.parsed["members"][0]["path"], "file.csv",
        "relative_path should be used as member path, not absolute path"
    );
}

#[test]
fn falls_back_to_path_when_relative_path_absent() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","path":"/data/fallback.csv","bytes_hash":"sha256:5678","size":20,"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(
        result.parsed["members"][0]["path"], "/data/fallback.csv",
        "should fall back to path when relative_path is absent"
    );
}

// ---------------------------------------------------------------------------
// Fixture: Windows-style paths from upstream
// ---------------------------------------------------------------------------

#[test]
fn windows_paths_normalized_in_spine_output() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"data\\sub\\file.csv","bytes_hash":"sha256:1234","size":10,"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(
        result.parsed["members"][0]["path"], "data/sub/file.csv",
        "backslash paths must be normalized to forward slash"
    );
}

// ---------------------------------------------------------------------------
// Fixture: warning detail with nested values
// ---------------------------------------------------------------------------

#[test]
fn warning_detail_non_string_values_rendered() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"ok.csv","bytes_hash":"sha256:1234","size":1,"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"weird.bin","_warnings":[{"tool":"hash","code":"E_SIZE","message":"File too large","detail":{"max_bytes":104857600,"actual_bytes":209715200}}],"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockPartial);
    let detail = &result.parsed["skipped"][0]["warnings"][0]["detail"];
    // Non-string values in detail should be rendered (via to_string for non-str).
    assert!(detail["max_bytes"].is_string() || detail["max_bytes"].is_number());
    assert!(detail["actual_bytes"].is_string() || detail["actual_bytes"].is_number());
}

// ---------------------------------------------------------------------------
// Fixture: all-skipped records (every record has _skipped:true)
// ---------------------------------------------------------------------------

#[test]
fn all_skipped_records_creates_partial_with_zero_members() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"a.csv","_warnings":[{"tool":"hash","code":"E_IO","message":"fail"}],"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"b.csv","_warnings":[],"tool_versions":{"hash":"0.1.0"}}"#,
        "\n",
    );

    let result = run_spine(jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockPartial);
    assert_eq!(result.parsed["member_count"], 0);
    assert_eq!(result.parsed["skipped_count"], 2);
    assert!(result.parsed["members"].as_array().unwrap().is_empty());
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}

// ---------------------------------------------------------------------------
// Fixture: single-file pipeline (minimal)
// ---------------------------------------------------------------------------

#[test]
fn single_file_pipeline() {
    let jsonl = r#"{"version":"hash.v0","relative_path":"solo.csv","bytes_hash":"sha256:deadbeef00000000000000000000000000000000000000000000000000000000","size":42,"tool_versions":{"hash":"0.1.0"}}"#;
    let jsonl = format!("{jsonl}\n");

    let result = run_spine(&jsonl, None, None, None);

    assert_eq!(result.outcome, DomainOutcome::LockCreated);
    assert_eq!(result.parsed["member_count"], 1);
    assert_eq!(result.parsed["members"][0]["path"], "solo.csv");
    assert_eq!(result.parsed["members"][0]["size"], 42);
    assert!(verify_lock_hash_from_json(&result.json).expect("should parse"));
}
