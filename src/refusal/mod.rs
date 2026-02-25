use serde::Serialize;
use serde_json::Value;

/// Lock schema version, shared across lockfile and refusal envelopes.
pub const LOCK_VERSION: &str = "lock.v0";

/// Maximum sample paths included in `E_MISSING_HASH` detail.
const MAX_SAMPLE_PATHS: usize = 5;

/// Refusal codes defined by the lock spec.
///
/// Each code identifies a specific failure mode with a prescribed recovery action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCode {
    /// No input records (stdin or file was empty).
    Empty,
    /// Invalid JSONL (parse error) or unknown record version.
    BadInput,
    /// One or more non-skipped records lack `bytes_hash`.
    MissingHash,
}

impl RefusalCode {
    /// Wire-format string for JSON output (e.g. `"E_EMPTY"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "E_EMPTY",
            Self::BadInput => "E_BAD_INPUT",
            Self::MissingHash => "E_MISSING_HASH",
        }
    }
}

impl Serialize for RefusalCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The inner refusal body containing code, message, detail, and recovery command.
#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub code: RefusalCode,
    pub message: String,
    pub detail: Value,
    pub next_command: Option<String>,
}

/// Top-level refusal envelope emitted to stdout on exit 2.
///
/// Shape:
/// ```json
/// {
///   "version": "lock.v0",
///   "outcome": "REFUSAL",
///   "refusal": { "code": "...", "message": "...", "detail": {}, "next_command": "..." }
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct RefusalEnvelope {
    pub version: String,
    pub outcome: String,
    pub refusal: Refusal,
}

impl RefusalEnvelope {
    /// Serialize to deterministic JSON (sorted keys at all levels, compact, no trailing newline).
    pub fn to_json(&self) -> String {
        // Convert struct → Value (gives us a Map), then serialize.
        // serde_json::Map preserves insertion order from struct field declaration,
        // so we convert through Value to get sorted keys via to_value + to_string.
        let value = serde_json::to_value(self).expect("RefusalEnvelope is always serializable");
        let sorted = sort_value(value);
        serde_json::to_string(&sorted).expect("sorted Value is always serializable")
    }
}

/// Recursively sort all object keys in a JSON value.
fn sort_value(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let sorted: serde_json::Map<String, Value> =
                map.into_iter().map(|(k, v)| (k, sort_value(v))).collect();
            // serde_json::Map is backed by BTreeMap when the "preserve_order" feature
            // is NOT enabled (default). Re-collecting produces sorted keys.
            // To be safe regardless of feature flags, explicitly sort via BTreeMap.
            let btree: std::collections::BTreeMap<String, Value> = sorted.into_iter().collect();
            let re_map: serde_json::Map<String, Value> = btree.into_iter().collect();
            Value::Object(re_map)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_value).collect()),
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// Build an `E_EMPTY` refusal: no input records at all.
pub fn empty() -> RefusalEnvelope {
    RefusalEnvelope {
        version: LOCK_VERSION.to_string(),
        outcome: "REFUSAL".to_string(),
        refusal: Refusal {
            code: RefusalCode::Empty,
            message: "no input records — run vacuum first".to_string(),
            detail: serde_json::json!({}),
            next_command: Some("vacuum <path> | hash | lock".to_string()),
        },
    }
}

/// Build an `E_BAD_INPUT` refusal for a JSONL parse error.
pub fn bad_input_parse(line: usize, error: &str) -> RefusalEnvelope {
    RefusalEnvelope {
        version: LOCK_VERSION.to_string(),
        outcome: "REFUSAL".to_string(),
        refusal: Refusal {
            code: RefusalCode::BadInput,
            message: format!("invalid JSONL at line {line} — check upstream tool output"),
            detail: serde_json::json!({
                "line": line,
                "error": error,
            }),
            next_command: None,
        },
    }
}

/// Build an `E_BAD_INPUT` refusal for an unknown record version.
pub fn bad_input_version(line: usize, version: &str) -> RefusalEnvelope {
    RefusalEnvelope {
        version: LOCK_VERSION.to_string(),
        outcome: "REFUSAL".to_string(),
        refusal: Refusal {
            code: RefusalCode::BadInput,
            message: format!(
                "unknown record version \"{version}\" at line {line} — check upstream tool output"
            ),
            detail: serde_json::json!({
                "line": line,
                "version": version,
            }),
            next_command: None,
        },
    }
}

/// Build an `E_MISSING_HASH` refusal for non-skipped records that lack `bytes_hash`.
///
/// `all_paths` is the full list of affected paths; only up to [`MAX_SAMPLE_PATHS`]
/// are included in the envelope detail.
pub fn missing_hash(count: usize, all_paths: Vec<String>) -> RefusalEnvelope {
    let sample_paths: Vec<String> = all_paths.into_iter().take(MAX_SAMPLE_PATHS).collect();
    let noun = if count == 1 {
        "record lacks"
    } else {
        "records lack"
    };
    RefusalEnvelope {
        version: LOCK_VERSION.to_string(),
        outcome: "REFUSAL".to_string(),
        refusal: Refusal {
            code: RefusalCode::MissingHash,
            message: format!("{count} {noun} bytes_hash — run hash first"),
            detail: serde_json::json!({
                "count": count,
                "sample_paths": sample_paths,
            }),
            next_command: Some("vacuum <path> | hash | lock".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_envelope_shape() {
        let env = empty();
        assert_eq!(env.version, LOCK_VERSION);
        assert_eq!(env.outcome, "REFUSAL");
        assert_eq!(env.refusal.code, RefusalCode::Empty);
        assert_eq!(env.refusal.code.as_str(), "E_EMPTY");
        assert_eq!(env.refusal.detail, serde_json::json!({}));
        assert!(env.refusal.next_command.is_some());
    }

    #[test]
    fn bad_input_parse_envelope_shape() {
        let env = bad_input_parse(42, "expected value at line 1 column 1");
        assert_eq!(env.refusal.code, RefusalCode::BadInput);
        assert_eq!(env.refusal.code.as_str(), "E_BAD_INPUT");
        assert_eq!(env.refusal.detail["line"], 42);
        assert_eq!(
            env.refusal.detail["error"],
            "expected value at line 1 column 1"
        );
        assert!(env.refusal.next_command.is_none());
    }

    #[test]
    fn bad_input_version_envelope_shape() {
        let env = bad_input_version(3, "hash.v2");
        assert_eq!(env.refusal.code, RefusalCode::BadInput);
        assert_eq!(env.refusal.detail["line"], 3);
        assert_eq!(env.refusal.detail["version"], "hash.v2");
        assert!(env.refusal.next_command.is_none());
    }

    #[test]
    fn missing_hash_envelope_shape() {
        let paths = vec![
            "data/model.xlsx".to_string(),
            "data/tape.csv".to_string(),
            "data/readme.pdf".to_string(),
        ];
        let env = missing_hash(3, paths.clone());
        assert_eq!(env.refusal.code, RefusalCode::MissingHash);
        assert_eq!(env.refusal.code.as_str(), "E_MISSING_HASH");
        assert_eq!(env.refusal.detail["count"], 3);
        let sample: Vec<String> =
            serde_json::from_value(env.refusal.detail["sample_paths"].clone()).unwrap();
        assert_eq!(sample, paths);
        assert!(env.refusal.next_command.is_some());
    }

    #[test]
    fn refusal_code_serialize() {
        let json = serde_json::to_string(&RefusalCode::Empty).unwrap();
        assert_eq!(json, "\"E_EMPTY\"");

        let json = serde_json::to_string(&RefusalCode::BadInput).unwrap();
        assert_eq!(json, "\"E_BAD_INPUT\"");

        let json = serde_json::to_string(&RefusalCode::MissingHash).unwrap();
        assert_eq!(json, "\"E_MISSING_HASH\"");
    }

    #[test]
    fn envelope_json_has_sorted_keys() {
        let env = empty();
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        // Top-level keys should be alphabetically sorted: outcome, refusal, version
        let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        assert_eq!(keys, &["outcome", "refusal", "version"]);

        // Refusal keys should be sorted: code, detail, message, next_command
        let refusal_keys: Vec<&String> = parsed["refusal"].as_object().unwrap().keys().collect();
        assert_eq!(refusal_keys, &["code", "detail", "message", "next_command"]);
    }

    #[test]
    fn envelope_json_round_trip() {
        let env = missing_hash(2, vec!["a.csv".to_string(), "b.xlsx".to_string()]);
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["version"], "lock.v0");
        assert_eq!(parsed["outcome"], "REFUSAL");
        assert_eq!(parsed["refusal"]["code"], "E_MISSING_HASH");
        assert_eq!(parsed["refusal"]["detail"]["count"], 2);
        assert_eq!(parsed["refusal"]["detail"]["sample_paths"][0], "a.csv");
        assert_eq!(parsed["refusal"]["detail"]["sample_paths"][1], "b.xlsx");
    }

    #[test]
    fn envelope_json_no_trailing_newline() {
        let json = empty().to_json();
        assert!(!json.ends_with('\n'));
    }

    #[test]
    fn empty_detail_is_empty_object() {
        let env = empty();
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["refusal"]["detail"], serde_json::json!({}));
    }

    #[test]
    fn null_next_command_serializes_as_null() {
        let env = bad_input_parse(1, "bad json");
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["refusal"]["next_command"].is_null());
    }

    #[test]
    fn missing_hash_singular_message() {
        let env = missing_hash(1, vec!["single.csv".to_string()]);
        assert!(
            env.refusal.message.contains("1 record lacks"),
            "expected singular: {}",
            env.refusal.message
        );
    }

    #[test]
    fn missing_hash_plural_message() {
        let env = missing_hash(3, vec!["a.csv".to_string(), "b.csv".to_string()]);
        assert!(
            env.refusal.message.contains("3 records lack"),
            "expected plural: {}",
            env.refusal.message
        );
    }

    #[test]
    fn missing_hash_truncates_sample_paths() {
        let paths: Vec<String> = (0..20).map(|i| format!("file_{i}.csv")).collect();
        let env = missing_hash(20, paths);
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["refusal"]["detail"]["count"], 20);
        let sample = parsed["refusal"]["detail"]["sample_paths"]
            .as_array()
            .unwrap();
        assert_eq!(sample.len(), MAX_SAMPLE_PATHS);
        assert_eq!(sample[0], "file_0.csv");
        assert_eq!(sample[4], "file_4.csv");
    }

    #[test]
    fn envelope_compact_no_extra_whitespace() {
        let json = empty().to_json();
        assert!(!json.contains('\n'), "must be compact (no newlines)");
        assert!(!json.contains("  "), "must be compact (no indentation)");
    }

    #[test]
    fn deterministic_output() {
        let json1 = missing_hash(2, vec!["a.csv".to_string(), "b.csv".to_string()]).to_json();
        let json2 = missing_hash(2, vec!["a.csv".to_string(), "b.csv".to_string()]).to_json();
        assert_eq!(json1, json2, "same inputs must produce identical output");
    }

    #[test]
    fn detail_keys_sorted_in_missing_hash() {
        let env = missing_hash(2, vec!["a.csv".to_string()]);
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        let det_keys: Vec<&String> = parsed["refusal"]["detail"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(det_keys, &["count", "sample_paths"]);
    }

    #[test]
    fn detail_keys_sorted_in_parse_error() {
        let env = bad_input_parse(10, "unexpected EOF");
        let json = env.to_json();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        let det_keys: Vec<&String> = parsed["refusal"]["detail"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(det_keys, &["error", "line"]);
    }
}
