use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Lockfile;

/// Compute the `lock_hash` for a lockfile.
///
/// Algorithm (from PLAN.md):
/// 1. Set `lock_hash` to `""` (empty string).
/// 2. Serialize to canonical JSON (sorted keys, compact, no trailing newline).
/// 3. SHA256 the canonical byte sequence.
/// 4. Return `"sha256:<hex>"`.
pub fn compute_lock_hash(lockfile: &Lockfile) -> String {
    let mut pre_hash = lockfile.clone();
    pre_hash.lock_hash = String::new();

    let canonical =
        to_canonical_json(&pre_hash).expect("Lockfile should always be serializable to JSON");

    let digest = Sha256::digest(canonical.as_bytes());
    format!("sha256:{:x}", digest)
}

/// Verify the `lock_hash` of a lockfile.
///
/// Returns `true` if the stored `lock_hash` matches a fresh computation.
pub fn verify_lock_hash(lockfile: &Lockfile) -> bool {
    let expected = compute_lock_hash(lockfile);
    lockfile.lock_hash == expected
}

/// Verify the `lock_hash` of a lockfile from raw JSON bytes.
///
/// Parses the JSON, blanks `lock_hash`, re-serializes canonically, SHA256s,
/// and compares to the stored value.
pub fn verify_lock_hash_from_json(json: &str) -> Result<bool, serde_json::Error> {
    let mut value: Value = serde_json::from_str(json)?;

    // Extract stored lock_hash.
    let stored_hash = value
        .get("lock_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Blank lock_hash for canonical computation.
    if let Some(obj) = value.as_object_mut() {
        obj.insert("lock_hash".to_string(), Value::String(String::new()));
    }

    let canonical = to_canonical_json(&value)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let computed = format!("sha256:{:x}", digest);

    Ok(stored_hash == computed)
}

/// Serialize a value to canonical JSON (sorted keys at all levels, compact, no trailing newline).
pub fn to_canonical_json<T>(value: &T) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    let json_value = serde_json::to_value(value)?;
    let sorted = sort_json_value(json_value);
    serde_json::to_string(&sorted)
}

fn sort_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .into_iter()
                .map(|(key, value)| (key, sort_json_value(value)))
                .collect::<BTreeMap<String, Value>>()
                .into_iter()
                .collect();
            Value::Object(sorted)
        }
        Value::Array(array) => Value::Array(array.into_iter().map(sort_json_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::lockfile::{Lockfile, Member};

    fn make_test_lockfile() -> Lockfile {
        let mut tool_versions = BTreeMap::new();
        tool_versions.insert("vacuum".to_string(), "0.1.0".to_string());
        tool_versions.insert("hash".to_string(), "0.1.0".to_string());
        tool_versions.insert("lock".to_string(), "0.1.0".to_string());

        Lockfile {
            version: "lock.v0".to_string(),
            lock_hash: String::new(),
            dataset_id: Some("test-dataset".to_string()),
            as_of: None,
            note: None,
            created: "2026-01-15T10:30:00Z".to_string(),
            tool_versions,
            profiles: vec![],
            skipped: vec![],
            members: vec![
                Member {
                    path: "alpha.csv".to_string(),
                    bytes_hash: "sha256:aaaa".to_string(),
                    size: 100,
                    fingerprint: None,
                },
                Member {
                    path: "beta.csv".to_string(),
                    bytes_hash: "sha256:bbbb".to_string(),
                    size: 200,
                    fingerprint: None,
                },
            ],
            skipped_count: 0,
            member_count: 2,
        }
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let input = serde_json::json!({
            "zeta": {
                "delta": 4,
                "alpha": 1
            },
            "beta": [
                { "gamma": 3, "beta": 2 },
                { "epsilon": 5, "alpha": 1 }
            ],
            "alpha": 0
        });

        let canonical = to_canonical_json(&input).expect("canonical serialization should succeed");

        assert_eq!(
            canonical,
            r#"{"alpha":0,"beta":[{"beta":2,"gamma":3},{"alpha":1,"epsilon":5}],"zeta":{"alpha":1,"delta":4}}"#
        );
    }

    #[test]
    fn canonical_json_is_deterministic_for_equivalent_values() {
        let first: serde_json::Value =
            serde_json::from_str(r#"{"b":{"y":2,"x":1},"a":[{"d":4,"c":3}], "z":0}"#)
                .expect("valid json");
        let second: serde_json::Value =
            serde_json::from_str(r#"{"z":0,"a":[{"c":3,"d":4}],"b":{"x":1,"y":2}}"#)
                .expect("valid json");

        let canonical_first =
            to_canonical_json(&first).expect("canonical serialization should succeed");
        let canonical_second =
            to_canonical_json(&second).expect("canonical serialization should succeed");

        assert_eq!(canonical_first, canonical_second);
    }

    #[test]
    fn canonical_json_has_no_trailing_newline() {
        let input = serde_json::json!({"b": 1, "a": 2});
        let canonical = to_canonical_json(&input).expect("canonical serialization should succeed");
        assert!(!canonical.ends_with('\n'));
    }

    #[test]
    fn lockfile_canonical_json_orders_top_level_and_nested_keys() {
        let lockfile = make_test_lockfile();
        let canonical = to_canonical_json(&lockfile).expect("should serialize");
        let parsed: Value = serde_json::from_str(&canonical).expect("should parse");

        let top_level_keys = parsed
            .as_object()
            .expect("top level should be object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            top_level_keys,
            vec![
                "as_of",
                "created",
                "dataset_id",
                "lock_hash",
                "member_count",
                "members",
                "note",
                "profiles",
                "skipped",
                "skipped_count",
                "tool_versions",
                "version"
            ]
        );

        let member_keys = parsed["members"][0]
            .as_object()
            .expect("member should be object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            member_keys,
            vec!["bytes_hash", "fingerprint", "path", "size"]
        );
    }

    #[test]
    fn compute_produces_sha256_prefixed_hex() {
        let lockfile = make_test_lockfile();
        let hash = compute_lock_hash(&lockfile);
        assert!(
            hash.starts_with("sha256:"),
            "hash should start with sha256:"
        );
        let hex_part = &hash["sha256:".len()..];
        assert_eq!(hex_part.len(), 64, "SHA256 hex should be 64 chars");
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex"
        );
    }

    #[test]
    fn compute_is_deterministic() {
        let lockfile = make_test_lockfile();
        let hash1 = compute_lock_hash(&lockfile);
        let hash2 = compute_lock_hash(&lockfile);
        assert_eq!(hash1, hash2, "same input must produce same hash");
    }

    #[test]
    fn verify_round_trip() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash = compute_lock_hash(&lockfile);
        assert!(
            verify_lock_hash(&lockfile),
            "freshly computed lock_hash must verify"
        );
    }

    #[test]
    fn verify_detects_tampering() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash = compute_lock_hash(&lockfile);

        // Tamper with the lockfile.
        lockfile.dataset_id = Some("tampered".to_string());

        assert!(
            !verify_lock_hash(&lockfile),
            "tampered lockfile must not verify"
        );
    }

    #[test]
    fn verify_detects_hash_tampering() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();
        assert!(
            !verify_lock_hash(&lockfile),
            "wrong lock_hash must not verify"
        );
    }

    #[test]
    fn verify_from_json_round_trip() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash = compute_lock_hash(&lockfile);

        // Serialize the complete lockfile (with real lock_hash) to JSON.
        let json = to_canonical_json(&lockfile).expect("should serialize");

        assert!(
            verify_lock_hash_from_json(&json).expect("should parse"),
            "round-trip from JSON must verify"
        );
    }

    #[test]
    fn verify_from_json_detects_tampering() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash = compute_lock_hash(&lockfile);

        // Serialize, then tamper.
        let mut value: Value = serde_json::to_value(&lockfile).expect("should serialize");
        value["dataset_id"] = Value::String("tampered".to_string());
        let tampered_json = serde_json::to_string(&value).expect("should serialize");

        assert!(
            !verify_lock_hash_from_json(&tampered_json).expect("should parse"),
            "tampered JSON must not verify"
        );
    }

    #[test]
    fn verify_from_json_detects_nested_member_tampering() {
        let mut lockfile = make_test_lockfile();
        lockfile.lock_hash = compute_lock_hash(&lockfile);

        let mut value: Value = serde_json::to_value(&lockfile).expect("should serialize");
        value["members"][0]["size"] = Value::from(999_u64);
        let tampered_json = serde_json::to_string(&value).expect("should serialize");

        assert!(
            !verify_lock_hash_from_json(&tampered_json).expect("should parse"),
            "tampering nested member fields must invalidate verification"
        );
    }

    #[test]
    fn different_data_produces_different_hash() {
        let lockfile1 = make_test_lockfile();
        let mut lockfile2 = make_test_lockfile();
        lockfile2.dataset_id = Some("other-dataset".to_string());

        let hash1 = compute_lock_hash(&lockfile1);
        let hash2 = compute_lock_hash(&lockfile2);
        assert_ne!(hash1, hash2, "different data must produce different hashes");
    }

    #[test]
    fn pre_hash_lockfile_uses_empty_lock_hash() {
        // Verify that the canonical JSON used for hashing has lock_hash as ""
        let lockfile = make_test_lockfile();
        let mut pre_hash = lockfile.clone();
        pre_hash.lock_hash = String::new();
        let canonical = to_canonical_json(&pre_hash).expect("should serialize");

        let parsed: Value = serde_json::from_str(&canonical).expect("should parse");
        assert_eq!(
            parsed["lock_hash"], "",
            "pre-hash canonical form must have empty lock_hash"
        );
    }
}
