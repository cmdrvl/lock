use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

const MISSING_HASH_SAMPLE_LIMIT: usize = 5;

const ACCEPTED_RECORD_VERSIONS: [&str; 3] = ["vacuum.v0", "hash.v0", "fingerprint.v0"];

#[derive(Debug, Clone, PartialEq)]
pub struct InputRecord {
    pub line_number: usize,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReadResult {
    Empty,
    Records(Vec<InputRecord>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErrorDetail {
    pub line: usize,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionErrorDetail {
    pub line: usize,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingHashDetail {
    pub count: usize,
    pub sample_paths: Vec<String>,
}

#[derive(Debug)]
pub enum InputError {
    Io(io::Error),
    Parse(ParseErrorDetail),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    BadVersion(VersionErrorDetail),
    MissingHash(MissingHashDetail),
}

impl InputError {
    pub fn parse_detail(&self) -> Option<&ParseErrorDetail> {
        match self {
            Self::Parse(detail) => Some(detail),
            Self::Io(_) => None,
        }
    }
}

impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error while reading input: {error}"),
            Self::Parse(detail) => {
                write!(
                    formatter,
                    "JSON parse error at input line {}: {}",
                    detail.line, detail.error
                )
            }
        }
    }
}

impl std::error::Error for InputError {}

pub fn read_jsonl(input: Option<&Path>) -> Result<ReadResult, InputError> {
    match input {
        Some(path) => {
            let file = File::open(path).map_err(InputError::Io)?;
            read_jsonl_reader(BufReader::new(file))
        }
        None => {
            let stdin = io::stdin();
            read_jsonl_reader(stdin.lock())
        }
    }
}

pub fn read_jsonl_reader<R>(reader: R) -> Result<ReadResult, InputError>
where
    R: BufRead,
{
    let mut records = Vec::new();

    for (index, line_result) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line_result.map_err(InputError::Io)?;

        if line.trim().is_empty() {
            return Err(InputError::Parse(ParseErrorDetail {
                line: line_number,
                error: "line is empty; expected one JSON value per line".to_owned(),
            }));
        }

        let value = serde_json::from_str::<Value>(&line).map_err(|error| {
            InputError::Parse(ParseErrorDetail {
                line: line_number,
                error: error.to_string(),
            })
        })?;

        records.push(InputRecord { line_number, value });
    }

    if records.is_empty() {
        Ok(ReadResult::Empty)
    } else {
        Ok(ReadResult::Records(records))
    }
}

pub fn validate_records(records: &[InputRecord]) -> Result<(), ValidationError> {
    let mut missing_hash_paths = Vec::new();

    for record in records {
        validate_version(record)?;

        if is_skipped(record) {
            continue;
        }

        if !has_non_empty_string_field(&record.value, "bytes_hash") {
            missing_hash_paths.push(path_for_missing_hash(record));
        }
    }

    if missing_hash_paths.is_empty() {
        Ok(())
    } else {
        let sample_paths = missing_hash_paths
            .iter()
            .take(MISSING_HASH_SAMPLE_LIMIT)
            .cloned()
            .collect();
        Err(ValidationError::MissingHash(MissingHashDetail {
            count: missing_hash_paths.len(),
            sample_paths,
        }))
    }
}

fn validate_version(record: &InputRecord) -> Result<(), ValidationError> {
    let version = record
        .value
        .get("version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    match version.as_deref() {
        Some(version) if ACCEPTED_RECORD_VERSIONS.contains(&version) => Ok(()),
        _ => Err(ValidationError::BadVersion(VersionErrorDetail {
            line: record.line_number,
            version,
        })),
    }
}

fn is_skipped(record: &InputRecord) -> bool {
    record
        .value
        .get("_skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn has_non_empty_string_field(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|field| !field.trim().is_empty())
}

fn path_for_missing_hash(record: &InputRecord) -> String {
    record
        .value
        .get("relative_path")
        .or_else(|| record.value.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("<unknown>")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        InputError, InputRecord, ReadResult, ValidationError, read_jsonl, read_jsonl_reader,
        validate_records,
    };

    #[test]
    fn returns_empty_for_empty_stream() {
        let reader = Cursor::new("");

        let result = read_jsonl_reader(reader).expect("empty stream should parse");

        assert_eq!(result, ReadResult::Empty);
    }

    #[test]
    fn parses_multiple_jsonl_records_with_line_numbers() {
        let reader = Cursor::new("{\"path\":\"a\"}\n{\"path\":\"b\"}\n");

        let result = read_jsonl_reader(reader).expect("valid JSONL should parse");

        let ReadResult::Records(records) = result else {
            panic!("expected records");
        };

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].line_number, 1);
        assert_eq!(records[1].line_number, 2);
        assert_eq!(records[0].value["path"], "a");
        assert_eq!(records[1].value["path"], "b");
    }

    #[test]
    fn parse_error_contains_failing_line_number() {
        let reader = Cursor::new("{\"path\":\"ok\"}\nnot-json\n");

        let error = read_jsonl_reader(reader).expect_err("invalid JSON must error");

        let InputError::Parse(detail) = error else {
            panic!("expected parse error");
        };

        assert_eq!(detail.line, 2);
        assert!(detail.error.contains("expected"));
    }

    #[test]
    fn empty_line_is_parse_error() {
        let reader = Cursor::new("{\"path\":\"ok\"}\n\n");

        let error = read_jsonl_reader(reader).expect_err("blank lines must error");

        let InputError::Parse(detail) = error else {
            panic!("expected parse error");
        };

        assert_eq!(detail.line, 2);
        assert_eq!(
            detail.error,
            "line is empty; expected one JSON value per line"
        );
    }

    #[test]
    fn missing_input_file_returns_io_error() {
        let missing_path = PathBuf::from("this-path-does-not-exist-for-lock-tests.jsonl");

        let error = read_jsonl(Some(&missing_path)).expect_err("missing file should fail");

        assert!(matches!(error, InputError::Io(_)));
    }

    #[test]
    fn validate_records_rejects_missing_version() {
        let records = vec![InputRecord {
            line_number: 4,
            value: json!({
                "path": "/tmp/a.csv",
                "relative_path": "a.csv",
                "bytes_hash": "sha256:1234",
            }),
        }];

        let error = validate_records(&records).expect_err("missing version must fail");

        let ValidationError::BadVersion(detail) = error else {
            panic!("expected bad version");
        };
        assert_eq!(detail.line, 4);
        assert_eq!(detail.version, None);
    }

    #[test]
    fn validate_records_rejects_unknown_version() {
        let records = vec![InputRecord {
            line_number: 2,
            value: json!({
                "version": "hash.v2",
                "path": "/tmp/a.csv",
                "relative_path": "a.csv",
                "bytes_hash": "sha256:1234",
            }),
        }];

        let error = validate_records(&records).expect_err("unknown version must fail");

        let ValidationError::BadVersion(detail) = error else {
            panic!("expected bad version");
        };
        assert_eq!(detail.line, 2);
        assert_eq!(detail.version.as_deref(), Some("hash.v2"));
    }

    #[test]
    fn validate_records_ignores_missing_hash_for_skipped_record() {
        let records = vec![InputRecord {
            line_number: 8,
            value: json!({
                "version": "hash.v0",
                "path": "/tmp/a.csv",
                "relative_path": "a.csv",
                "_skipped": true,
            }),
        }];

        validate_records(&records).expect("skipped records should not require bytes_hash");
    }

    #[test]
    fn validate_records_reports_missing_hash_for_non_skipped_records() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "version": "hash.v0",
                    "path": "/tmp/a.csv",
                    "relative_path": "a.csv",
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "version": "hash.v0",
                    "path": "/tmp/b.csv",
                    "relative_path": "b.csv",
                    "_skipped": true,
                }),
            },
            InputRecord {
                line_number: 3,
                value: json!({
                    "version": "fingerprint.v0",
                    "path": "/tmp/c.csv",
                    "relative_path": "c.csv",
                }),
            },
        ];

        let error = validate_records(&records).expect_err("missing hash must fail");

        let ValidationError::MissingHash(detail) = error else {
            panic!("expected missing-hash validation error");
        };
        assert_eq!(detail.count, 2);
        assert_eq!(
            detail.sample_paths,
            vec!["a.csv".to_string(), "c.csv".to_string()]
        );
    }

    #[test]
    fn validate_records_accepts_known_versions_and_hashes() {
        let records = vec![
            InputRecord {
                line_number: 1,
                value: json!({
                    "version": "vacuum.v0",
                    "path": "/tmp/a.csv",
                    "relative_path": "a.csv",
                    "bytes_hash": "sha256:aaaa",
                }),
            },
            InputRecord {
                line_number: 2,
                value: json!({
                    "version": "hash.v0",
                    "path": "/tmp/b.csv",
                    "relative_path": "b.csv",
                    "bytes_hash": "sha256:bbbb",
                }),
            },
            InputRecord {
                line_number: 3,
                value: json!({
                    "version": "fingerprint.v0",
                    "path": "/tmp/c.csv",
                    "relative_path": "c.csv",
                    "bytes_hash": "sha256:cccc",
                }),
            },
        ];

        validate_records(&records).expect("known versions with bytes_hash should pass");
    }
}
