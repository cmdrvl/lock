//! Integration tests for the parse → validate → classify pipeline.
//!
//! bd-v08: parsing, version-gate, missing-hash, skipped handling.
//! These tests exercise the public API across module boundaries.

use std::io::Cursor;

use serde_json::json;

use lock::input::{InputRecord, ReadResult, ValidationError, read_jsonl_reader, validate_records};
use lock::lockfile::{self, ClassificationError};
use lock::output::DomainOutcome;

// ---------------------------------------------------------------------------
// End-to-end: parse JSONL → validate → classify
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_parse_validate_classify_lock_created() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"alpha.csv","bytes_hash":"sha256:aaaa","size":100}"#,
        "\n",
        r#"{"version":"hash.v0","relative_path":"beta.csv","bytes_hash":"sha256:bbbb","size":200}"#,
        "\n",
    );

    let result = read_jsonl_reader(Cursor::new(jsonl)).expect("parse should succeed");
    let ReadResult::Records(records) = result else {
        panic!("expected records");
    };
    assert_eq!(records.len(), 2);

    validate_records(&records).expect("validation should pass");

    let classification =
        lockfile::classify_records(&records).expect("classification should succeed");
    assert_eq!(classification.member_count, 2);
    assert_eq!(classification.skipped_count, 0);
    assert_eq!(classification.outcome, DomainOutcome::LockCreated);
    assert_eq!(classification.members[0].path, "alpha.csv");
    assert_eq!(classification.members[1].path, "beta.csv");
}

#[test]
fn end_to_end_parse_validate_classify_lock_partial() {
    let jsonl = concat!(
        r#"{"version":"hash.v0","relative_path":"good.csv","bytes_hash":"sha256:aaaa","size":100}"#,
        "\n",
        r#"{"version":"hash.v0","_skipped":true,"relative_path":"bad.csv","_warnings":[{"tool":"hash","code":"E_IO","message":"permission denied"}]}"#,
        "\n",
    );

    let result = read_jsonl_reader(Cursor::new(jsonl)).expect("parse should succeed");
    let ReadResult::Records(records) = result else {
        panic!("expected records");
    };

    validate_records(&records).expect("validation should pass (skipped records exempt from hash)");

    let classification =
        lockfile::classify_records(&records).expect("classification should succeed");
    assert_eq!(classification.member_count, 1);
    assert_eq!(classification.skipped_count, 1);
    assert_eq!(classification.outcome, DomainOutcome::LockPartial);
    assert_eq!(classification.members[0].path, "good.csv");
    assert_eq!(classification.skipped[0].path, "bad.csv");
    assert_eq!(classification.skipped[0].warnings[0].tool, "hash");
}

// ---------------------------------------------------------------------------
// Version gate
// ---------------------------------------------------------------------------

#[test]
fn version_gate_accepts_all_known_versions() {
    for version in ["vacuum.v0", "hash.v0", "fingerprint.v0"] {
        let records = vec![InputRecord {
            line_number: 1,
            value: json!({
                "version": version,
                "relative_path": "file.csv",
                "bytes_hash": "sha256:1234",
            }),
        }];
        validate_records(&records).unwrap_or_else(|_| panic!("{version} should be accepted"));
    }
}

#[test]
fn version_gate_rejects_future_version() {
    let records = vec![InputRecord {
        line_number: 5,
        value: json!({
            "version": "hash.v1",
            "relative_path": "file.csv",
            "bytes_hash": "sha256:1234",
        }),
    }];

    let err = validate_records(&records).expect_err("future version must fail");
    let ValidationError::BadVersion(detail) = err else {
        panic!("expected BadVersion");
    };
    assert_eq!(detail.line, 5);
    assert_eq!(detail.version.as_deref(), Some("hash.v1"));
}

#[test]
fn version_gate_rejects_empty_string_version() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "",
            "relative_path": "file.csv",
            "bytes_hash": "sha256:1234",
        }),
    }];

    let err = validate_records(&records).expect_err("empty version must fail");
    assert!(matches!(err, ValidationError::BadVersion(_)));
}

#[test]
fn version_gate_rejects_numeric_version() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": 1,
            "relative_path": "file.csv",
            "bytes_hash": "sha256:1234",
        }),
    }];

    let err = validate_records(&records).expect_err("numeric version must fail");
    let ValidationError::BadVersion(detail) = err else {
        panic!("expected BadVersion");
    };
    assert_eq!(detail.version, None); // non-string → None
}

#[test]
fn version_gate_stops_at_first_bad_version() {
    let records = vec![
        InputRecord {
            line_number: 1,
            value: json!({
                "version": "hash.v0",
                "relative_path": "ok.csv",
                "bytes_hash": "sha256:aaaa",
            }),
        },
        InputRecord {
            line_number: 2,
            value: json!({
                "version": "unknown.v0",
                "relative_path": "bad.csv",
                "bytes_hash": "sha256:bbbb",
            }),
        },
    ];

    let err = validate_records(&records).expect_err("should fail on second record");
    let ValidationError::BadVersion(detail) = err else {
        panic!("expected BadVersion");
    };
    assert_eq!(detail.line, 2);
}

// ---------------------------------------------------------------------------
// Missing hash detection
// ---------------------------------------------------------------------------

#[test]
fn missing_hash_detected_for_non_skipped_records() {
    let records = vec![
        InputRecord {
            line_number: 1,
            value: json!({
                "version": "hash.v0",
                "relative_path": "no-hash.csv",
            }),
        },
        InputRecord {
            line_number: 2,
            value: json!({
                "version": "hash.v0",
                "relative_path": "also-no-hash.csv",
            }),
        },
    ];

    let err = validate_records(&records).expect_err("missing hash must fail");
    let ValidationError::MissingHash(detail) = err else {
        panic!("expected MissingHash");
    };
    assert_eq!(detail.count, 2);
    assert_eq!(detail.sample_paths, vec!["no-hash.csv", "also-no-hash.csv"]);
}

#[test]
fn missing_hash_uses_path_fallback_when_relative_path_absent() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "path": "/absolute/path/file.csv",
        }),
    }];

    let err = validate_records(&records).expect_err("missing hash must fail");
    let ValidationError::MissingHash(detail) = err else {
        panic!("expected MissingHash");
    };
    assert_eq!(detail.sample_paths, vec!["/absolute/path/file.csv"]);
}

#[test]
fn missing_hash_reports_unknown_when_no_path_field() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
        }),
    }];

    let err = validate_records(&records).expect_err("missing hash must fail");
    let ValidationError::MissingHash(detail) = err else {
        panic!("expected MissingHash");
    };
    assert_eq!(detail.sample_paths, vec!["<unknown>"]);
}

#[test]
fn missing_hash_sample_paths_capped_at_five() {
    let records: Vec<InputRecord> = (0..10)
        .map(|i| InputRecord {
            line_number: i + 1,
            value: json!({
                "version": "hash.v0",
                "relative_path": format!("file_{i}.csv"),
            }),
        })
        .collect();

    let err = validate_records(&records).expect_err("missing hash must fail");
    let ValidationError::MissingHash(detail) = err else {
        panic!("expected MissingHash");
    };
    assert_eq!(detail.count, 10);
    assert_eq!(detail.sample_paths.len(), 5);
}

#[test]
fn empty_string_bytes_hash_treated_as_missing() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "relative_path": "empty-hash.csv",
            "bytes_hash": "",
        }),
    }];

    let err = validate_records(&records).expect_err("empty bytes_hash must fail");
    assert!(matches!(err, ValidationError::MissingHash(_)));
}

#[test]
fn whitespace_only_bytes_hash_treated_as_missing() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "relative_path": "ws-hash.csv",
            "bytes_hash": "   ",
        }),
    }];

    let err = validate_records(&records).expect_err("whitespace bytes_hash must fail");
    assert!(matches!(err, ValidationError::MissingHash(_)));
}

// ---------------------------------------------------------------------------
// Skipped handling
// ---------------------------------------------------------------------------

#[test]
fn skipped_true_exempts_from_missing_hash_check() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "relative_path": "skipped.csv",
            "_skipped": true,
        }),
    }];

    validate_records(&records).expect("skipped record should not require bytes_hash");
}

#[test]
fn skipped_false_requires_hash() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "relative_path": "not-skipped.csv",
            "_skipped": false,
        }),
    }];

    let err = validate_records(&records).expect_err("_skipped:false must require hash");
    assert!(matches!(err, ValidationError::MissingHash(_)));
}

#[test]
fn missing_skipped_field_requires_hash() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "version": "hash.v0",
            "relative_path": "no-flag.csv",
        }),
    }];

    let err = validate_records(&records).expect_err("absent _skipped must require hash");
    assert!(matches!(err, ValidationError::MissingHash(_)));
}

#[test]
fn skipped_records_carry_warnings_through_classification() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "bad.csv",
            "_skipped": true,
            "_warnings": [
                {"tool": "hash", "code": "E_IO", "message": "cannot read"},
                {"tool": "fingerprint", "code": "E_TIMEOUT", "message": "timed out"}
            ]
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert_eq!(classification.skipped.len(), 1);
    assert_eq!(classification.skipped[0].warnings.len(), 2);
    assert_eq!(classification.skipped[0].warnings[0].tool, "hash");
    assert_eq!(classification.skipped[0].warnings[1].tool, "fingerprint");
}

#[test]
fn skipped_with_empty_warnings_array() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "empty-warn.csv",
            "_skipped": true,
            "_warnings": []
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert_eq!(classification.skipped.len(), 1);
    assert!(classification.skipped[0].warnings.is_empty());
}

#[test]
fn skipped_with_no_warnings_field() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "no-warnings.csv",
            "_skipped": true
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert_eq!(classification.skipped.len(), 1);
    assert!(classification.skipped[0].warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Fingerprint passthrough
// ---------------------------------------------------------------------------

#[test]
fn fingerprint_present_and_matched() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "data.csv",
            "bytes_hash": "sha256:aaaa",
            "size": 100,
            "fingerprint": {
                "fingerprint_id": "csv_header_v1",
                "fingerprint_version": "0.1.0",
                "matched": true,
                "content_hash": "blake3:cccc"
            }
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    let fp = classification.members[0]
        .fingerprint
        .as_ref()
        .expect("fingerprint should be present");
    assert_eq!(fp.fingerprint_id, "csv_header_v1");
    assert_eq!(fp.fingerprint_version, "0.1.0");
    assert!(fp.matched);
    assert_eq!(fp.content_hash.as_deref(), Some("blake3:cccc"));
}

#[test]
fn fingerprint_present_but_not_matched() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "binary.bin",
            "bytes_hash": "sha256:dddd",
            "size": 50,
            "fingerprint": {
                "fingerprint_id": "csv_header_v1",
                "fingerprint_version": "0.1.0",
                "matched": false,
                "content_hash": null
            }
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    let fp = classification.members[0]
        .fingerprint
        .as_ref()
        .expect("fingerprint should be present");
    assert!(!fp.matched);
    assert_eq!(fp.content_hash, None);
}

#[test]
fn fingerprint_absent_yields_none() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "no-fp.csv",
            "bytes_hash": "sha256:eeee",
            "size": 10
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert!(classification.members[0].fingerprint.is_none());
}

#[test]
fn fingerprint_null_yields_none() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "null-fp.csv",
            "bytes_hash": "sha256:ffff",
            "size": 10,
            "fingerprint": null
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert!(classification.members[0].fingerprint.is_none());
}

// ---------------------------------------------------------------------------
// Classification edge cases
// ---------------------------------------------------------------------------

#[test]
fn classification_requires_path_field() {
    let records = vec![InputRecord {
        line_number: 7,
        value: json!({
            "bytes_hash": "sha256:1234",
            "size": 10
        }),
    }];

    let err = lockfile::classify_records(&records).expect_err("missing path must fail");
    assert_eq!(err, ClassificationError::MissingPath { line_number: 7 });
}

#[test]
fn classification_requires_size_field() {
    let records = vec![InputRecord {
        line_number: 3,
        value: json!({
            "relative_path": "no-size.csv",
            "bytes_hash": "sha256:1234"
        }),
    }];

    let err = lockfile::classify_records(&records).expect_err("missing size must fail");
    assert_eq!(err, ClassificationError::MissingSize { line_number: 3 });
}

#[test]
fn classification_sorts_members_lexicographically() {
    let records = vec![
        InputRecord {
            line_number: 1,
            value: json!({"relative_path": "zebra.csv", "bytes_hash": "sha256:z", "size": 1}),
        },
        InputRecord {
            line_number: 2,
            value: json!({"relative_path": "alpha.csv", "bytes_hash": "sha256:a", "size": 1}),
        },
        InputRecord {
            line_number: 3,
            value: json!({"relative_path": "mango.csv", "bytes_hash": "sha256:m", "size": 1}),
        },
    ];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    let paths: Vec<&str> = classification
        .members
        .iter()
        .map(|m| m.path.as_str())
        .collect();
    assert_eq!(paths, vec!["alpha.csv", "mango.csv", "zebra.csv"]);
}

#[test]
fn backslash_paths_normalized_to_forward_slash() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "data\\sub\\file.csv",
            "bytes_hash": "sha256:1234",
            "size": 10
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert_eq!(classification.members[0].path, "data/sub/file.csv");
}

#[test]
fn unicode_paths_preserved() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({
            "relative_path": "données/résultat.csv",
            "bytes_hash": "sha256:1234",
            "size": 10
        }),
    }];

    let classification = lockfile::classify_records(&records).expect("should succeed");
    assert_eq!(classification.members[0].path, "données/résultat.csv");
}

// ---------------------------------------------------------------------------
// Metadata hydration
// ---------------------------------------------------------------------------

#[test]
fn hydrate_merges_tool_versions_across_records() {
    let records = vec![
        InputRecord {
            line_number: 1,
            value: json!({"tool_versions": {"vacuum": "0.1.0", "hash": "0.2.0"}}),
        },
        InputRecord {
            line_number: 2,
            value: json!({"tool_versions": {"fingerprint": "0.3.0"}}),
        },
    ];

    let metadata = lockfile::hydrate_metadata(&records, "0.1.0", None, None, None);
    assert_eq!(
        metadata.tool_versions.get("vacuum").map(|s| s.as_str()),
        Some("0.1.0")
    );
    assert_eq!(
        metadata.tool_versions.get("hash").map(|s| s.as_str()),
        Some("0.2.0")
    );
    assert_eq!(
        metadata
            .tool_versions
            .get("fingerprint")
            .map(|s| s.as_str()),
        Some("0.3.0")
    );
    assert_eq!(
        metadata.tool_versions.get("lock").map(|s| s.as_str()),
        Some("0.1.0")
    );
}

#[test]
fn hydrate_first_seen_version_wins() {
    let records = vec![
        InputRecord {
            line_number: 1,
            value: json!({"tool_versions": {"hash": "0.2.0"}}),
        },
        InputRecord {
            line_number: 2,
            value: json!({"tool_versions": {"hash": "9.9.9"}}),
        },
    ];

    let metadata = lockfile::hydrate_metadata(&records, "0.1.0", None, None, None);
    assert_eq!(
        metadata.tool_versions.get("hash").map(|s| s.as_str()),
        Some("0.2.0"),
        "first-seen version should win"
    );
}

#[test]
fn hydrate_records_without_tool_versions_ignored() {
    let records = vec![InputRecord {
        line_number: 1,
        value: json!({"relative_path": "file.csv"}),
    }];

    let metadata = lockfile::hydrate_metadata(&records, "0.1.0", None, None, None);
    assert_eq!(metadata.tool_versions.len(), 1); // only "lock"
    assert_eq!(
        metadata.tool_versions.get("lock").map(|s| s.as_str()),
        Some("0.1.0")
    );
}

// ---------------------------------------------------------------------------
// JSONL parsing edge cases
// ---------------------------------------------------------------------------

#[test]
fn single_record_jsonl() {
    let result = read_jsonl_reader(Cursor::new(r#"{"a":1}"#)).expect("should parse");
    let ReadResult::Records(records) = result else {
        panic!("expected records");
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].line_number, 1);
}

#[test]
fn trailing_newline_does_not_create_extra_record() {
    let jsonl = "{\"a\":1}\n{\"b\":2}\n";
    let result = read_jsonl_reader(Cursor::new(jsonl)).expect("should parse");
    let ReadResult::Records(records) = result else {
        panic!("expected records");
    };
    assert_eq!(records.len(), 2);
}
