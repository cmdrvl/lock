//! Integration tests for witness dispatch functions (query, last, count)
//! and filter edge cases.
//!
//! bd-3c4: Validates dispatch exit codes, filter semantics, and subcommand
//! behavior against real ledger files.

use lock::cli::WitnessFilters;
use lock::witness::{WitnessRecord, apply_filters, read_ledger};

fn write_ledger(dir: &std::path::Path, records: &[&str]) -> std::path::PathBuf {
    let path = dir.join("witness.jsonl");
    let content = records.join("\n") + "\n";
    std::fs::write(&path, content).unwrap();
    path
}

fn make_ledger_record(tool: &str, outcome: &str, ts: &str) -> String {
    serde_json::json!({
        "id": format!("blake3:{tool}-{ts}"),
        "tool": tool,
        "version": "0.1.0",
        "outcome": outcome,
        "exit_code": match outcome {
            "LOCK_CREATED" => 0,
            "LOCK_PARTIAL" => 1,
            "REFUSAL" => 2,
            _ => -1,
        },
        "ts": ts,
        "output_hash": "blake3:0000",
        "inputs": [{ "path": "stdin", "hash": null, "bytes": null }],
        "params": { "dataset_id": null, "as_of": null, "note": null },
        "prev": null,
    })
    .to_string()
}

fn make_record_with_input_hash(tool: &str, ts: &str, hash: &str) -> String {
    serde_json::json!({
        "id": format!("blake3:{tool}-{ts}"),
        "tool": tool,
        "version": "0.1.0",
        "outcome": "LOCK_CREATED",
        "exit_code": 0,
        "ts": ts,
        "output_hash": "blake3:0000",
        "inputs": [{ "path": "data.jsonl", "hash": hash, "bytes": 1024 }],
        "params": {},
        "prev": null,
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// read_ledger edge cases
// ---------------------------------------------------------------------------

#[test]
fn read_ledger_handles_records_from_other_tools() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            &make_ledger_record("shape", "SHAPE_CREATED", "2026-01-02T00:00:00Z"),
            &make_ledger_record("lock", "REFUSAL", "2026-01-03T00:00:00Z"),
        ],
    );

    let records = read_ledger(&path).unwrap();
    assert_eq!(records.len(), 3);

    // Filter to lock-only
    let filters = WitnessFilters {
        tool: Some("lock".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 2);
}

#[test]
fn read_ledger_handles_extra_fields_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let record = r#"{"tool":"lock","outcome":"LOCK_CREATED","ts":"2026-01-01T00:00:00Z","extra_field":"should_be_captured","nested":{"deep":true}}"#;
    let path = write_ledger(dir.path(), &[record]);

    let records = read_ledger(&path).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tool.as_deref(), Some("lock"));
    // Extra fields captured via serde flatten.
    assert!(records[0].extra.contains_key("extra_field"));
}

#[test]
fn read_ledger_handles_minimal_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(dir.path(), &[r#"{}"#]);

    let records = read_ledger(&path).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].tool.is_none());
    assert!(records[0].outcome.is_none());
    assert!(records[0].ts.is_none());
}

// ---------------------------------------------------------------------------
// Input hash substring filter
// ---------------------------------------------------------------------------

#[test]
fn filter_by_input_hash_substring() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            &make_record_with_input_hash("lock", "2026-01-01T00:00:00Z", "sha256:abcdef1234567890"),
            &make_record_with_input_hash("lock", "2026-01-02T00:00:00Z", "sha256:9876fedcba543210"),
            &make_record_with_input_hash("lock", "2026-01-03T00:00:00Z", "sha256:abcdef9999999999"),
        ],
    );

    let records = read_ledger(&path).unwrap();

    // Match substring "abcdef" — should find records 1 and 3.
    let filters = WitnessFilters {
        input_hash: Some("abcdef".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 2);

    // Match exact full hash — should find exactly 1.
    let filters = WitnessFilters {
        input_hash: Some("9876fedcba543210".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].ts.as_deref(), Some("2026-01-02T00:00:00Z"));
}

#[test]
fn filter_by_input_hash_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[&make_record_with_input_hash(
            "lock",
            "2026-01-01T00:00:00Z",
            "sha256:abcdef",
        )],
    );

    let records = read_ledger(&path).unwrap();
    let filters = WitnessFilters {
        input_hash: Some("zzzzz".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert!(matched.is_empty());
}

#[test]
fn filter_by_input_hash_with_null_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[&make_ledger_record(
            "lock",
            "LOCK_CREATED",
            "2026-01-01T00:00:00Z",
        )],
    );

    let records = read_ledger(&path).unwrap();
    // Record has null hash in inputs — should not match.
    let filters = WitnessFilters {
        input_hash: Some("abc".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert!(matched.is_empty());
}

// ---------------------------------------------------------------------------
// Combined filter edge cases
// ---------------------------------------------------------------------------

#[test]
fn filter_since_and_until_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-15T00:00:00Z"),
            &make_ledger_record("lock", "LOCK_CREATED", "2026-02-01T00:00:00Z"),
            &make_ledger_record("lock", "LOCK_CREATED", "2026-03-01T00:00:00Z"),
        ],
    );

    let records = read_ledger(&path).unwrap();
    let filters = WitnessFilters {
        since: Some("2026-01-10T00:00:00Z".to_string()),
        until: Some("2026-02-15T00:00:00Z".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 2);
}

#[test]
fn filter_since_uses_rfc3339_instant_semantics_for_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            // Absolute time: 2025-12-31T22:00:00Z
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00+02:00"),
            // Absolute time: 2025-12-31T22:30:00Z
            &make_ledger_record("lock", "LOCK_CREATED", "2025-12-31T22:30:00Z"),
        ],
    );

    let records = read_ledger(&path).unwrap();
    let filters = WitnessFilters {
        since: Some("2025-12-31T22:15:00Z".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);

    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].ts.as_deref(), Some("2025-12-31T22:30:00Z"));
}

#[test]
fn malformed_timestamps_do_not_match_since_until_filters() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[r#"{"tool":"lock","outcome":"LOCK_CREATED","ts":"not-a-timestamp"}"#],
    );

    let records = read_ledger(&path).unwrap();

    let invalid_filter = WitnessFilters {
        since: Some("not-a-filter-ts".to_string()),
        ..Default::default()
    };
    assert!(apply_filters(&records, &invalid_filter).is_empty());

    let invalid_record_ts = WitnessFilters {
        until: Some("2026-12-31T00:00:00Z".to_string()),
        ..Default::default()
    };
    assert!(apply_filters(&records, &invalid_record_ts).is_empty());
}

#[test]
fn filter_all_criteria_combined() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            &make_ledger_record("hash", "LOCK_CREATED", "2026-01-15T00:00:00Z"),
            &make_ledger_record("lock", "REFUSAL", "2026-01-20T00:00:00Z"),
            &make_ledger_record("lock", "LOCK_CREATED", "2026-02-01T00:00:00Z"),
            &make_ledger_record("lock", "LOCK_CREATED", "2026-03-01T00:00:00Z"),
        ],
    );

    let records = read_ledger(&path).unwrap();
    let filters = WitnessFilters {
        tool: Some("lock".to_string()),
        outcome: Some("LOCK_CREATED".to_string()),
        since: Some("2026-01-10T00:00:00Z".to_string()),
        until: Some("2026-02-15T00:00:00Z".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    // Only "2026-02-01" matches all criteria.
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].ts.as_deref(), Some("2026-02-01T00:00:00Z"));
}

// ---------------------------------------------------------------------------
// WitnessRecord serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn witness_record_round_trip_through_serde() {
    let json = make_ledger_record("lock", "LOCK_CREATED", "2026-01-15T10:30:00Z");
    let record: WitnessRecord = serde_json::from_str(&json).unwrap();

    assert_eq!(record.tool.as_deref(), Some("lock"));
    assert_eq!(record.outcome.as_deref(), Some("LOCK_CREATED"));
    assert_eq!(record.exit_code, Some(0));
    assert_eq!(record.ts.as_deref(), Some("2026-01-15T10:30:00Z"));

    // Re-serialize and verify it's valid JSON.
    let reserialized = serde_json::to_string(&record).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(reparsed["tool"], "lock");
}

#[test]
fn witness_record_deserializes_with_missing_optional_fields() {
    let json = r#"{"tool":"lock","outcome":"LOCK_CREATED"}"#;
    let record: WitnessRecord = serde_json::from_str(json).unwrap();

    assert_eq!(record.tool.as_deref(), Some("lock"));
    assert!(record.id.is_none());
    assert!(record.version.is_none());
    assert!(record.exit_code.is_none());
    assert!(record.ts.is_none());
    assert!(record.output_hash.is_none());
    assert!(record.inputs.is_none());
    assert!(record.params.is_none());
    assert!(record.prev.is_none());
    assert!(record.binary_hash.is_none());
}

// ---------------------------------------------------------------------------
// Timestamp-based ordering
// ---------------------------------------------------------------------------

#[test]
fn records_without_timestamps_handled_in_filters() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ledger(
        dir.path(),
        &[
            r#"{"tool":"lock","outcome":"LOCK_CREATED"}"#,
            &make_ledger_record("lock", "LOCK_CREATED", "2026-01-15T00:00:00Z"),
        ],
    );

    let records = read_ledger(&path).unwrap();

    // Since filter should exclude records without timestamps.
    let filters = WitnessFilters {
        since: Some("2026-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].ts.as_deref(), Some("2026-01-15T00:00:00Z"));

    // Until filter should also exclude records without timestamps.
    let filters = WitnessFilters {
        until: Some("2026-12-31T00:00:00Z".to_string()),
        ..Default::default()
    };
    let matched = apply_filters(&records, &filters);
    assert_eq!(matched.len(), 1);
}
