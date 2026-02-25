use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::input::InputRecord;
use crate::output::DomainOutcome;

pub mod self_hash;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lockfile {
    pub version: String,
    pub lock_hash: String,
    pub dataset_id: Option<String>,
    pub as_of: Option<String>,
    pub note: Option<String>,
    pub created: String,
    pub tool_versions: BTreeMap<String, String>,
    pub profiles: Vec<String>,
    pub skipped: Vec<SkippedEntry>,
    pub members: Vec<Member>,
    pub skipped_count: u64,
    pub member_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Member {
    pub path: String,
    pub bytes_hash: String,
    pub size: u64,
    pub fingerprint: Option<FingerprintResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FingerprintResult {
    pub fingerprint_id: String,
    pub fingerprint_version: String,
    pub matched: bool,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedEntry {
    pub path: String,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    pub tool: String,
    pub code: String,
    pub message: String,
    pub detail: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub members: Vec<Member>,
    pub skipped: Vec<SkippedEntry>,
    pub skipped_count: u64,
    pub member_count: u64,
    pub outcome: DomainOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassificationError {
    MissingPath { line_number: usize },
    MissingBytesHash { line_number: usize },
    MissingSize { line_number: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataHydration {
    pub dataset_id: Option<String>,
    pub as_of: Option<String>,
    pub note: Option<String>,
    pub profiles: Vec<String>,
    pub tool_versions: BTreeMap<String, String>,
}

pub fn classify_records(records: &[InputRecord]) -> Result<Classification, ClassificationError> {
    let mut members = Vec::new();
    let mut skipped = Vec::new();

    for record in records {
        let path = extract_record_path(&record.value, record.line_number)?;
        let is_skipped = record
            .value
            .get("_skipped")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_skipped {
            skipped.push(SkippedEntry {
                path,
                warnings: extract_warnings(&record.value),
            });
            continue;
        }

        let bytes_hash = record
            .value
            .get("bytes_hash")
            .and_then(Value::as_str)
            .ok_or(ClassificationError::MissingBytesHash {
                line_number: record.line_number,
            })?
            .to_owned();

        let size = record.value.get("size").and_then(Value::as_u64).ok_or(
            ClassificationError::MissingSize {
                line_number: record.line_number,
            },
        )?;

        members.push(Member {
            path,
            bytes_hash,
            size,
            fingerprint: extract_fingerprint(&record.value),
        });
    }

    members.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    skipped.sort_unstable_by(|left, right| left.path.cmp(&right.path));

    let skipped_count = skipped.len() as u64;
    let member_count = members.len() as u64;
    let outcome = if skipped.is_empty() {
        DomainOutcome::LockCreated
    } else {
        DomainOutcome::LockPartial
    };

    Ok(Classification {
        members,
        skipped,
        skipped_count,
        member_count,
        outcome,
    })
}

pub fn hydrate_metadata(
    records: &[InputRecord],
    lock_version: &str,
    dataset_id: Option<&str>,
    as_of: Option<&str>,
    note: Option<&str>,
) -> MetadataHydration {
    MetadataHydration {
        dataset_id: dataset_id.map(str::to_owned),
        as_of: as_of.map(str::to_owned),
        note: note.map(str::to_owned),
        profiles: Vec::new(),
        tool_versions: merge_tool_versions(records, lock_version),
    }
}

pub fn merge_tool_versions(
    records: &[InputRecord],
    lock_version: &str,
) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();

    for record in records {
        let Some(tool_versions) = record.value.get("tool_versions").and_then(Value::as_object)
        else {
            continue;
        };

        for (tool, version_value) in tool_versions {
            let Some(version) = version_value.as_str() else {
                continue;
            };

            merged
                .entry(tool.clone())
                .or_insert_with(|| version.to_owned());
        }
    }

    merged
        .entry("lock".to_owned())
        .or_insert_with(|| lock_version.to_owned());
    merged
}

fn extract_record_path(value: &Value, line_number: usize) -> Result<String, ClassificationError> {
    let path = value
        .get("relative_path")
        .and_then(Value::as_str)
        .or_else(|| value.get("path").and_then(Value::as_str))
        .ok_or(ClassificationError::MissingPath { line_number })?;
    Ok(normalize_path(path))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn extract_fingerprint(value: &Value) -> Option<FingerprintResult> {
    let object = value.get("fingerprint")?.as_object()?;

    Some(FingerprintResult {
        fingerprint_id: object.get("fingerprint_id")?.as_str()?.to_owned(),
        fingerprint_version: object.get("fingerprint_version")?.as_str()?.to_owned(),
        matched: object.get("matched")?.as_bool()?,
        content_hash: object
            .get("content_hash")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn extract_warnings(value: &Value) -> Vec<Warning> {
    value
        .get("_warnings")
        .and_then(Value::as_array)
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(Value::as_object)
                .map(|warning| Warning {
                    tool: warning
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    code: warning
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    message: warning
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    detail: extract_warning_detail(warning.get("detail")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_warning_detail(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|detail| {
            detail
                .iter()
                .map(|(key, value)| {
                    let rendered = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    (key.clone(), rendered)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ClassificationError, classify_records, hydrate_metadata, merge_tool_versions};
    use crate::input::InputRecord;
    use crate::output::DomainOutcome;

    #[test]
    fn classify_records_computes_counts_and_partial_outcome() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "relative_path": "zeta.csv",
                    "path": "/data/zeta.csv",
                    "bytes_hash": "sha256:zeta",
                    "size": 22
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "relative_path": "alpha.csv",
                    "path": "/data/alpha.csv",
                    "bytes_hash": "sha256:alpha",
                    "size": 11
                }),
            },
            InputRecord {
                line_number: 3,
                value: json!({
                    "_skipped": true,
                    "relative_path": "skipped.csv",
                    "_warnings": [{
                        "tool": "hash",
                        "code": "E_IO",
                        "message": "Cannot read file",
                        "detail": {
                            "reason": "permission denied"
                        }
                    }]
                }),
            },
        ];

        let classification = classify_records(&records).expect("classification should succeed");

        assert_eq!(classification.member_count, 2);
        assert_eq!(classification.skipped_count, 1);
        assert_eq!(classification.outcome, DomainOutcome::LockPartial);
        assert_eq!(classification.members[0].path, "alpha.csv");
        assert_eq!(classification.members[1].path, "zeta.csv");
        assert_eq!(classification.skipped[0].path, "skipped.csv");
        assert_eq!(classification.skipped[0].warnings[0].tool, "hash");
        assert_eq!(
            classification.skipped[0].warnings[0]
                .detail
                .get("reason")
                .expect("reason should exist"),
            "permission denied"
        );
    }

    #[test]
    fn classify_records_sorts_paths_and_normalizes_separators() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "_skipped": true,
                    "relative_path": "skip\\b.txt",
                    "_warnings": []
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "_skipped": true,
                    "relative_path": "skip\\a.txt",
                    "_warnings": []
                }),
            },
            InputRecord {
                line_number: 3,
                value: json!({
                    "relative_path": "member\\b.txt",
                    "bytes_hash": "sha256:b",
                    "size": 2
                }),
            },
            InputRecord {
                line_number: 4,
                value: json!({
                    "relative_path": "member\\a.txt",
                    "bytes_hash": "sha256:a",
                    "size": 1
                }),
            },
        ];

        let classification = classify_records(&records).expect("classification should succeed");

        assert_eq!(
            classification
                .members
                .iter()
                .map(|member| member.path.as_str())
                .collect::<Vec<_>>(),
            vec!["member/a.txt", "member/b.txt"]
        );
        assert_eq!(
            classification
                .skipped
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["skip/a.txt", "skip/b.txt"]
        );
    }

    #[test]
    fn classify_records_requires_bytes_hash_for_non_skipped_records() {
        let records = vec![InputRecord {
            line_number: 11,
            value: json!({
                "relative_path": "missing-hash.csv",
                "size": 10
            }),
        }];

        let error = classify_records(&records).expect_err("missing hash must fail");
        assert_eq!(
            error,
            ClassificationError::MissingBytesHash { line_number: 11 }
        );
    }

    #[test]
    fn classify_records_uses_path_when_relative_path_is_missing() {
        let records = vec![InputRecord {
            line_number: 6,
            value: json!({
                "path": "folder\\file.txt",
                "bytes_hash": "sha256:abc",
                "size": 5
            }),
        }];

        let classification = classify_records(&records).expect("classification should succeed");
        assert_eq!(classification.member_count, 1);
        assert_eq!(classification.skipped_count, 0);
        assert_eq!(classification.outcome, DomainOutcome::LockCreated);
        assert_eq!(classification.members[0].path, "folder/file.txt");
    }

    #[test]
    fn classify_records_is_deterministic_for_same_input() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "relative_path": "b.csv",
                    "bytes_hash": "sha256:b",
                    "size": 2
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "_skipped": true,
                    "relative_path": "skip.csv",
                    "_warnings": []
                }),
            },
            InputRecord {
                line_number: 3,
                value: json!({
                    "relative_path": "a.csv",
                    "bytes_hash": "sha256:a",
                    "size": 1
                }),
            },
        ];

        let first = classify_records(&records).expect("first classification must succeed");
        let second = classify_records(&records).expect("second classification must succeed");

        assert_eq!(first, second);
        assert_eq!(
            first
                .members
                .iter()
                .map(|member| member.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.csv", "b.csv"]
        );
        assert_eq!(first.skipped[0].path, "skip.csv");
    }

    #[test]
    fn merge_tool_versions_includes_skipped_records_and_preserves_first_seen() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "tool_versions": {
                        "vacuum": "0.1.0",
                        "hash": "0.2.0"
                    }
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "_skipped": true,
                    "tool_versions": {
                        "fingerprint": "0.3.0",
                        "hash": "9.9.9"
                    }
                }),
            },
        ];

        let versions = merge_tool_versions(&records, "0.9.0");

        assert_eq!(versions.get("vacuum"), Some(&"0.1.0".to_owned()));
        assert_eq!(versions.get("hash"), Some(&"0.2.0".to_owned()));
        assert_eq!(versions.get("fingerprint"), Some(&"0.3.0".to_owned()));
        assert_eq!(versions.get("lock"), Some(&"0.9.0".to_owned()));
    }

    #[test]
    fn hydrate_metadata_sets_nullable_fields_and_defaults_profiles() {
        let records = vec![InputRecord {
            line_number: 1,
            value: json!({
                "tool_versions": {
                    "vacuum": "0.1.0"
                }
            }),
        }];

        let metadata = hydrate_metadata(
            &records,
            "0.1.0",
            Some("dataset-a"),
            Some("2026-02-24T00:00:00Z"),
            Some("note"),
        );

        assert_eq!(metadata.dataset_id.as_deref(), Some("dataset-a"));
        assert_eq!(metadata.as_of.as_deref(), Some("2026-02-24T00:00:00Z"));
        assert_eq!(metadata.note.as_deref(), Some("note"));
        assert!(metadata.profiles.is_empty());
        assert_eq!(
            metadata.tool_versions.get("vacuum"),
            Some(&"0.1.0".to_owned())
        );
        assert_eq!(
            metadata.tool_versions.get("lock"),
            Some(&"0.1.0".to_owned())
        );
    }

    #[test]
    fn hydrate_metadata_uses_nullables_when_flags_omitted() {
        let metadata = hydrate_metadata(&[], "0.1.0", None, None, None);

        assert_eq!(metadata.dataset_id, None);
        assert_eq!(metadata.as_of, None);
        assert_eq!(metadata.note, None);
        assert!(metadata.profiles.is_empty());
        assert_eq!(
            metadata.tool_versions.get("lock"),
            Some(&"0.1.0".to_owned())
        );
    }
}
