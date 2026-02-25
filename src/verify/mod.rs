pub mod members;
pub mod output;

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::cli::VerifyArgs;
use crate::lockfile::self_hash;
use crate::refusal::sort_value;

/// Verify output schema version.
pub const VERIFY_VERSION: &str = "lock-verify.v0";

// ---------------------------------------------------------------------------
// Verify refusal codes
// ---------------------------------------------------------------------------

/// Refusal codes specific to the verify subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyRefusalCode {
    /// Cannot read the lockfile from disk.
    Io,
    /// Malformed JSON, missing required fields, absolute paths, traversal.
    BadLockfile,
    /// Lockfile version is not supported.
    UnsupportedVersion,
    /// `--root` directory does not exist.
    RootNotFound,
    /// Unrecognized hash algorithm prefix in a member's `bytes_hash`.
    UnknownAlgorithm,
}

impl VerifyRefusalCode {
    /// Wire-format string for JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Io => "E_IO",
            Self::BadLockfile => "E_BAD_LOCKFILE",
            Self::UnsupportedVersion => "E_UNSUPPORTED_VERSION",
            Self::RootNotFound => "E_ROOT_NOT_FOUND",
            Self::UnknownAlgorithm => "E_UNKNOWN_ALGORITHM",
        }
    }
}

impl Serialize for VerifyRefusalCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Refusal envelope constructors
// ---------------------------------------------------------------------------

/// Serialize a verify refusal envelope to JSON with the correct code wire format.
///
/// We can't reuse `RefusalEnvelope::to_json()` directly because the code field
/// needs to use `VerifyRefusalCode`, not `RefusalCode`. Instead, we build the
/// JSON value manually with the correct code string.
fn verify_refusal_json(code: VerifyRefusalCode, message: String, detail: Value) -> String {
    let value = serde_json::json!({
        "version": VERIFY_VERSION,
        "outcome": "REFUSAL",
        "refusal": {
            "code": code.as_str(),
            "message": message,
            "detail": detail,
            "next_command": null,
        }
    });
    let sorted = sort_value(value);
    serde_json::to_string(&sorted).expect("sorted Value is always serializable")
}

/// E_IO: cannot read lockfile.
pub fn refusal_io(path: &Path, error: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::Io,
        format!("cannot read lockfile — {error}"),
        serde_json::json!({
            "path": path.display().to_string(),
            "error": error,
        }),
    )
}

/// E_BAD_LOCKFILE: malformed JSON.
pub fn refusal_bad_lockfile_parse(error: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::BadLockfile,
        format!("malformed lockfile JSON — {error}"),
        serde_json::json!({
            "error": error,
        }),
    )
}

/// E_BAD_LOCKFILE: missing required fields.
pub fn refusal_bad_lockfile_missing_fields(missing: &[&str]) -> String {
    verify_refusal_json(
        VerifyRefusalCode::BadLockfile,
        "lockfile missing required fields".to_string(),
        serde_json::json!({
            "missing_fields": missing,
        }),
    )
}

/// E_BAD_LOCKFILE: absolute member path.
pub fn refusal_bad_lockfile_absolute_path(member_index: usize, member_path: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::BadLockfile,
        format!("member path is absolute: {member_path}"),
        serde_json::json!({
            "member_index": member_index,
            "member_path": member_path,
        }),
    )
}

/// E_BAD_LOCKFILE: path traversal.
pub fn refusal_bad_lockfile_traversal(member_index: usize, member_path: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::BadLockfile,
        format!("member path contains traversal: {member_path}"),
        serde_json::json!({
            "member_index": member_index,
            "member_path": member_path,
        }),
    )
}

/// E_UNSUPPORTED_VERSION: lockfile version not recognized.
pub fn refusal_unsupported_version(version: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::UnsupportedVersion,
        format!("unsupported lockfile version: {version}"),
        serde_json::json!({
            "version": version,
        }),
    )
}

/// E_ROOT_NOT_FOUND: --root directory does not exist.
pub fn refusal_root_not_found(root: &Path) -> String {
    verify_refusal_json(
        VerifyRefusalCode::RootNotFound,
        format!("root directory not found: {}", root.display()),
        serde_json::json!({
            "root": root.display().to_string(),
        }),
    )
}

/// E_UNKNOWN_ALGORITHM: unrecognized hash algorithm prefix.
pub fn refusal_unknown_algorithm(member_path: &str, algorithm: &str) -> String {
    verify_refusal_json(
        VerifyRefusalCode::UnknownAlgorithm,
        format!("unrecognized hash algorithm: {algorithm}"),
        serde_json::json!({
            "member_path": member_path,
            "algorithm": algorithm,
        }),
    )
}

// ---------------------------------------------------------------------------
// Lockfile validation
// ---------------------------------------------------------------------------

/// Recognized lockfile versions.
const SUPPORTED_VERSIONS: &[&str] = &["lock.v0"];

/// Recognized hash algorithm prefixes.
const SUPPORTED_ALGORITHMS: &[&str] = &["sha256", "blake3"];

/// Validation result: either the parsed JSON value or a refusal JSON string.
pub enum ValidationResult {
    /// Valid lockfile JSON, ready for verification.
    Ok(Value),
    /// Refusal JSON string to emit to stdout.
    Refusal(String),
}

/// Validate a lockfile JSON string.
///
/// Checks:
/// 1. Valid JSON
/// 2. Required fields: `version`, `lock_hash`, `members`
/// 3. Version is supported
/// 4. No absolute member paths
/// 5. No `..` traversal in member paths
/// 6. Recognized algorithm prefixes
pub fn validate_lockfile_json(json: &str) -> ValidationResult {
    let value: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return ValidationResult::Refusal(refusal_bad_lockfile_parse(&e.to_string())),
    };

    // Check required fields.
    let mut missing = Vec::new();
    if value.get("version").is_none() {
        missing.push("version");
    }
    if value.get("lock_hash").is_none() {
        missing.push("lock_hash");
    }
    if value.get("members").is_none() {
        missing.push("members");
    }
    if !missing.is_empty() {
        return ValidationResult::Refusal(refusal_bad_lockfile_missing_fields(&missing));
    }

    // Check version.
    let version = value["version"].as_str().unwrap_or("");
    if !SUPPORTED_VERSIONS.contains(&version) {
        return ValidationResult::Refusal(refusal_unsupported_version(version));
    }

    // Check member paths.
    if let Some(members) = value["members"].as_array() {
        for (i, member) in members.iter().enumerate() {
            let path = member.get("path").and_then(Value::as_str).unwrap_or("");

            // Absolute path check.
            if path.starts_with('/') || path.starts_with('\\') {
                return ValidationResult::Refusal(refusal_bad_lockfile_absolute_path(i, path));
            }

            // Traversal check.
            if path.split('/').any(|seg| seg == "..") || path.split('\\').any(|seg| seg == "..") {
                return ValidationResult::Refusal(refusal_bad_lockfile_traversal(i, path));
            }

            // Algorithm prefix check.
            if let Some(hash) = member.get("bytes_hash").and_then(Value::as_str)
                && let Some(prefix) = hash.split(':').next()
                && !SUPPORTED_ALGORITHMS.contains(&prefix)
            {
                return ValidationResult::Refusal(refusal_unknown_algorithm(path, prefix));
            }
        }
    }

    ValidationResult::Ok(value)
}

// ---------------------------------------------------------------------------
// Verify result types
// ---------------------------------------------------------------------------

/// The top-level result emitted by `lock verify`.
#[derive(Debug, Clone, Serialize)]
struct VerifyResult {
    version: String,
    outcome: String,
    lockfile: String,
    lock_hash: LockHashResult,
    members: Option<Value>,
    tool_versions: std::collections::BTreeMap<String, String>,
}

/// Self-hash verification detail.
#[derive(Debug, Clone, Serialize)]
struct LockHashResult {
    stored: String,
    computed: String,
    valid: bool,
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Run the verify subcommand. Returns the exit code.
pub fn run_verify(args: &VerifyArgs) -> u8 {
    // Step 1: Read lockfile from disk.
    let json = match fs::read_to_string(&args.lockfile) {
        Ok(content) => content,
        Err(e) => {
            let payload = refusal_io(&args.lockfile, &e.to_string());
            print!("{payload}");
            emit_witness(args, 2, "REFUSAL", payload.as_bytes());
            return 2;
        }
    };

    // Step 2: Validate lockfile JSON.
    match validate_lockfile_json(&json) {
        ValidationResult::Ok(_) => {}
        ValidationResult::Refusal(payload) => {
            print!("{payload}");
            emit_witness(args, 2, "REFUSAL", payload.as_bytes());
            return 2;
        }
    }

    // Step 3: Validate --root exists (if provided).
    if let Some(root) = &args.root
        && !root.is_dir()
    {
        let payload = refusal_root_not_found(root);
        print!("{payload}");
        emit_witness(args, 2, "REFUSAL", payload.as_bytes());
        return 2;
    }

    // Step 4: Level 1 — self-hash verification.
    let detail = match self_hash::verify_lock_hash_detail(&json) {
        Ok(d) => d,
        Err(e) => {
            // Shouldn't happen (already validated JSON), but handle gracefully.
            let payload = refusal_bad_lockfile_parse(&e.to_string());
            print!("{payload}");
            emit_witness(args, 2, "REFUSAL", payload.as_bytes());
            return 2;
        }
    };

    let lock_hash_result = LockHashResult {
        stored: detail.stored,
        computed: detail.computed,
        valid: detail.valid,
    };

    // Step 5: Level 2 — member verification (if --root and self-hash valid).
    let (members_value, outcome, exit_code) = if !lock_hash_result.valid {
        // Self-hash failed — skip member verification.
        (None, "VERIFY_FAILED", 1u8)
    } else if let Some(root) = &args.root {
        // Level 2: verify members against filesystem.
        let lockfile_value: Value = serde_json::from_str(&json).expect("already validated");
        let members_result = members::verify_members(&lockfile_value, root);
        let (outcome, exit_code) = members::members_outcome(&members_result, args.strict);
        let members_json =
            serde_json::to_value(&members_result).expect("MembersResult is serializable");
        (Some(members_json), outcome, exit_code)
    } else {
        (None, "VERIFY_OK", 0)
    };

    // Step 6: Build tool_versions.
    let mut tool_versions = std::collections::BTreeMap::new();
    tool_versions.insert("lock".to_string(), env!("CARGO_PKG_VERSION").to_string());

    // Step 7: Build result.
    let result = VerifyResult {
        version: VERIFY_VERSION.to_string(),
        outcome: outcome.to_string(),
        lockfile: args.lockfile.display().to_string(),
        lock_hash: lock_hash_result,
        members: members_value,
        tool_versions,
    };

    // Step 8: Emit output.
    let payload = if args.json {
        emit_verify_json(&result)
    } else {
        let json_value = serde_json::to_value(&result).expect("VerifyResult is serializable");
        let sorted = sort_value(json_value);
        output::render_human(&sorted)
    };
    print!("{payload}");
    emit_witness(args, exit_code, outcome, payload.as_bytes());
    exit_code
}

fn emit_verify_json(result: &VerifyResult) -> String {
    let value = serde_json::to_value(result).expect("VerifyResult is always serializable");
    let sorted = sort_value(value);
    serde_json::to_string(&sorted).expect("sorted Value is always serializable")
}

fn emit_witness(args: &VerifyArgs, exit_code: u8, outcome: &str, output_bytes: &[u8]) {
    if args.no_witness {
        return;
    }

    let params = serde_json::json!({
        "subcommand": "verify",
        "root": args.root.as_ref().map(|p| p.display().to_string()),
        "strict": args.strict,
    });
    let inputs = serde_json::json!([
        { "path": args.lockfile.display().to_string(), "hash": null, "bytes": null }
    ]);

    crate::witness::append_witness_record(outcome, exit_code, output_bytes, params, inputs);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- VerifyRefusalCode --

    #[test]
    fn refusal_code_as_str() {
        assert_eq!(VerifyRefusalCode::Io.as_str(), "E_IO");
        assert_eq!(VerifyRefusalCode::BadLockfile.as_str(), "E_BAD_LOCKFILE");
        assert_eq!(
            VerifyRefusalCode::UnsupportedVersion.as_str(),
            "E_UNSUPPORTED_VERSION"
        );
        assert_eq!(VerifyRefusalCode::RootNotFound.as_str(), "E_ROOT_NOT_FOUND");
        assert_eq!(
            VerifyRefusalCode::UnknownAlgorithm.as_str(),
            "E_UNKNOWN_ALGORITHM"
        );
    }

    #[test]
    fn refusal_code_serialize() {
        let json = serde_json::to_string(&VerifyRefusalCode::Io).unwrap();
        assert_eq!(json, "\"E_IO\"");

        let json = serde_json::to_string(&VerifyRefusalCode::BadLockfile).unwrap();
        assert_eq!(json, "\"E_BAD_LOCKFILE\"");
    }

    // -- Refusal envelope constructors --

    fn parse_refusal(json: &str) -> Value {
        let parsed: Value = serde_json::from_str(json).expect("refusal must be valid JSON");
        assert_eq!(parsed["version"], VERIFY_VERSION);
        assert_eq!(parsed["outcome"], "REFUSAL");
        parsed
    }

    #[test]
    fn refusal_io_envelope() {
        let json = refusal_io(Path::new("/tmp/bad.lock.json"), "permission denied");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_IO");
        assert_eq!(parsed["refusal"]["detail"]["path"], "/tmp/bad.lock.json");
        assert_eq!(parsed["refusal"]["detail"]["error"], "permission denied");
    }

    #[test]
    fn refusal_bad_lockfile_parse_envelope() {
        let json = refusal_bad_lockfile_parse("expected value at line 1 column 1");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
        assert!(
            parsed["refusal"]["detail"]["error"]
                .as_str()
                .unwrap()
                .contains("expected value")
        );
    }

    #[test]
    fn refusal_bad_lockfile_missing_fields_envelope() {
        let json = refusal_bad_lockfile_missing_fields(&["version", "members"]);
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
        let fields = parsed["refusal"]["detail"]["missing_fields"]
            .as_array()
            .unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], "version");
        assert_eq!(fields[1], "members");
    }

    #[test]
    fn refusal_bad_lockfile_absolute_path_envelope() {
        let json = refusal_bad_lockfile_absolute_path(0, "/etc/passwd");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
        assert_eq!(parsed["refusal"]["detail"]["member_index"], 0);
        assert_eq!(parsed["refusal"]["detail"]["member_path"], "/etc/passwd");
    }

    #[test]
    fn refusal_bad_lockfile_traversal_envelope() {
        let json = refusal_bad_lockfile_traversal(2, "data/../../../etc/passwd");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
        assert_eq!(parsed["refusal"]["detail"]["member_index"], 2);
        assert_eq!(
            parsed["refusal"]["detail"]["member_path"],
            "data/../../../etc/passwd"
        );
    }

    #[test]
    fn refusal_unsupported_version_envelope() {
        let json = refusal_unsupported_version("lock.v99");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_UNSUPPORTED_VERSION");
        assert_eq!(parsed["refusal"]["detail"]["version"], "lock.v99");
    }

    #[test]
    fn refusal_root_not_found_envelope() {
        let json = refusal_root_not_found(Path::new("/nonexistent"));
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_ROOT_NOT_FOUND");
        assert_eq!(parsed["refusal"]["detail"]["root"], "/nonexistent");
    }

    #[test]
    fn refusal_unknown_algorithm_envelope() {
        let json = refusal_unknown_algorithm("data.csv", "md5");
        let parsed = parse_refusal(&json);
        assert_eq!(parsed["refusal"]["code"], "E_UNKNOWN_ALGORITHM");
        assert_eq!(parsed["refusal"]["detail"]["member_path"], "data.csv");
        assert_eq!(parsed["refusal"]["detail"]["algorithm"], "md5");
    }

    #[test]
    fn refusal_envelopes_have_sorted_keys() {
        let json = refusal_io(Path::new("test.json"), "not found");
        let parsed: Value = serde_json::from_str(&json).unwrap();

        // Top-level keys sorted: outcome, refusal, version
        let top_keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        assert_eq!(top_keys, &["outcome", "refusal", "version"]);

        // Refusal keys sorted: code, detail, message, next_command
        let ref_keys: Vec<&String> = parsed["refusal"].as_object().unwrap().keys().collect();
        assert_eq!(ref_keys, &["code", "detail", "message", "next_command"]);
    }

    #[test]
    fn refusal_version_is_lock_verify_v0() {
        let json = refusal_io(Path::new("test.json"), "err");
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["version"], "lock-verify.v0");
    }

    // -- Validation --

    fn valid_lockfile_json() -> String {
        serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc123",
            "members": [
                { "path": "data/tape.csv", "bytes_hash": "sha256:aaa", "size": 100 },
                { "path": "data/model.xlsx", "bytes_hash": "blake3:bbb", "size": 200 }
            ],
            "skipped": [],
            "member_count": 2,
            "skipped_count": 0
        })
        .to_string()
    }

    #[test]
    fn validate_valid_lockfile() {
        match validate_lockfile_json(&valid_lockfile_json()) {
            ValidationResult::Ok(_) => {}
            ValidationResult::Refusal(r) => panic!("expected Ok, got refusal: {r}"),
        }
    }

    #[test]
    fn validate_malformed_json() {
        match validate_lockfile_json("not json {{{") {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_missing_version() {
        let json = serde_json::json!({
            "lock_hash": "sha256:abc",
            "members": []
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
                let fields = parsed["refusal"]["detail"]["missing_fields"]
                    .as_array()
                    .unwrap();
                assert!(fields.contains(&Value::String("version".to_string())));
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_missing_lock_hash() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "members": []
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                let fields = parsed["refusal"]["detail"]["missing_fields"]
                    .as_array()
                    .unwrap();
                assert!(fields.contains(&Value::String("lock_hash".to_string())));
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_missing_members() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc"
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                let fields = parsed["refusal"]["detail"]["missing_fields"]
                    .as_array()
                    .unwrap();
                assert!(fields.contains(&Value::String("members".to_string())));
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_multiple_missing_fields() {
        let json = serde_json::json!({}).to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                let fields = parsed["refusal"]["detail"]["missing_fields"]
                    .as_array()
                    .unwrap();
                assert_eq!(fields.len(), 3);
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_unsupported_version() {
        let json = serde_json::json!({
            "version": "lock.v99",
            "lock_hash": "sha256:abc",
            "members": []
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_UNSUPPORTED_VERSION");
                assert_eq!(parsed["refusal"]["detail"]["version"], "lock.v99");
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_absolute_member_path() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": [
                { "path": "/etc/passwd", "bytes_hash": "sha256:aaa", "size": 100 }
            ]
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
                assert_eq!(parsed["refusal"]["detail"]["member_index"], 0);
                assert_eq!(parsed["refusal"]["detail"]["member_path"], "/etc/passwd");
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_traversal_path() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": [
                { "path": "data/../../../etc/passwd", "bytes_hash": "sha256:aaa", "size": 100 }
            ]
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
                assert_eq!(parsed["refusal"]["detail"]["member_index"], 0);
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_unknown_algorithm() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": [
                { "path": "data.csv", "bytes_hash": "md5:aaa", "size": 100 }
            ]
        })
        .to_string();
        match validate_lockfile_json(&json) {
            ValidationResult::Refusal(r) => {
                let parsed: Value = serde_json::from_str(&r).unwrap();
                assert_eq!(parsed["refusal"]["code"], "E_UNKNOWN_ALGORITHM");
                assert_eq!(parsed["refusal"]["detail"]["member_path"], "data.csv");
                assert_eq!(parsed["refusal"]["detail"]["algorithm"], "md5");
            }
            ValidationResult::Ok(_) => panic!("expected refusal"),
        }
    }

    #[test]
    fn validate_sha256_algorithm_accepted() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": [
                { "path": "a.csv", "bytes_hash": "sha256:aaa", "size": 100 }
            ]
        })
        .to_string();
        assert!(matches!(
            validate_lockfile_json(&json),
            ValidationResult::Ok(_)
        ));
    }

    #[test]
    fn validate_blake3_algorithm_accepted() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": [
                { "path": "a.csv", "bytes_hash": "blake3:aaa", "size": 100 }
            ]
        })
        .to_string();
        assert!(matches!(
            validate_lockfile_json(&json),
            ValidationResult::Ok(_)
        ));
    }

    #[test]
    fn validate_empty_members_array_accepted() {
        let json = serde_json::json!({
            "version": "lock.v0",
            "lock_hash": "sha256:abc",
            "members": []
        })
        .to_string();
        assert!(matches!(
            validate_lockfile_json(&json),
            ValidationResult::Ok(_)
        ));
    }

    // -- Orchestration tests --

    use crate::cli::VerifyArgs;

    fn make_valid_lockfile_on_disk(dir: &std::path::Path) -> std::path::PathBuf {
        use crate::lockfile::self_hash::{compute_lock_hash, to_canonical_json};
        use crate::lockfile::{Lockfile, Member};

        let mut lockfile = Lockfile {
            version: "lock.v0".to_string(),
            lock_hash: String::new(),
            dataset_id: Some("test".to_string()),
            as_of: None,
            note: None,
            created: "2026-01-01T00:00:00Z".to_string(),
            tool_versions: std::collections::BTreeMap::from([(
                "lock".to_string(),
                "0.1.0".to_string(),
            )]),
            profiles: vec![],
            skipped: vec![],
            members: vec![Member {
                path: "a.csv".to_string(),
                bytes_hash: "sha256:aaaa".to_string(),
                size: 100,
                fingerprint: None,
            }],
            skipped_count: 0,
            member_count: 1,
        };
        lockfile.lock_hash = compute_lock_hash(&lockfile);
        let json = to_canonical_json(&lockfile).unwrap();
        let path = dir.join("test.lock.json");
        std::fs::write(&path, &json).unwrap();
        path
    }

    fn make_verify_args(lockfile: std::path::PathBuf) -> VerifyArgs {
        VerifyArgs {
            lockfile,
            root: None,
            json: true,
            no_witness: true,
            strict: false,
        }
    }

    #[test]
    fn run_verify_valid_lockfile_returns_0() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_valid_lockfile_on_disk(dir.path());
        let args = make_verify_args(path);
        assert_eq!(run_verify(&args), 0);
    }

    #[test]
    fn run_verify_tampered_lockfile_returns_1() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_valid_lockfile_on_disk(dir.path());

        // Tamper with the lockfile.
        let content = std::fs::read_to_string(&path).unwrap();
        let tampered = content.replace("\"test\"", "\"tampered\"");
        std::fs::write(&path, tampered).unwrap();

        let args = make_verify_args(path);
        assert_eq!(run_verify(&args), 1);
    }

    #[test]
    fn run_verify_missing_file_returns_2() {
        let args = make_verify_args(std::path::PathBuf::from("/nonexistent/file.lock.json"));
        assert_eq!(run_verify(&args), 2);
    }

    #[test]
    fn run_verify_malformed_json_returns_2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.lock.json");
        std::fs::write(&path, "not json {{{").unwrap();
        let args = make_verify_args(path);
        assert_eq!(run_verify(&args), 2);
    }

    #[test]
    fn run_verify_unsupported_version_returns_2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.lock.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "version": "lock.v99",
                "lock_hash": "sha256:abc",
                "members": []
            })
            .to_string(),
        )
        .unwrap();
        let args = make_verify_args(path);
        assert_eq!(run_verify(&args), 2);
    }

    #[test]
    fn run_verify_root_not_found_returns_2() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_valid_lockfile_on_disk(dir.path());
        let mut args = make_verify_args(path);
        args.root = Some(std::path::PathBuf::from("/nonexistent/dir"));
        assert_eq!(run_verify(&args), 2);
    }
}
