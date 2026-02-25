use std::io::Read;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of verifying all members against the filesystem.
#[derive(Debug, Clone, Serialize)]
pub struct MembersResult {
    pub root: String,
    pub checked: usize,
    pub verified: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<MemberFailure>,
    pub skips: Vec<MemberSkip>,
}

/// A member that failed verification.
#[derive(Debug, Clone, Serialize)]
pub struct MemberFailure {
    pub path: String,
    pub reason: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub expected_size: Option<u64>,
    pub actual_size: Option<u64>,
}

/// A member that was skipped due to I/O error.
#[derive(Debug, Clone, Serialize)]
pub struct MemberSkip {
    pub path: String,
    pub reason: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Verification logic
// ---------------------------------------------------------------------------

/// Verify all members in the lockfile against the filesystem.
///
/// For each member: resolve path against root, check existence, stat size,
/// and stream-hash using the algorithm prefix from the stored `bytes_hash`.
pub fn verify_members(lockfile_json: &Value, root: &Path) -> MembersResult {
    let members = lockfile_json
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut failures = Vec::new();
    let mut skips = Vec::new();
    let mut verified = 0;

    for member in &members {
        let member_path = member.get("path").and_then(Value::as_str).unwrap_or("");
        let expected_hash = member
            .get("bytes_hash")
            .and_then(Value::as_str)
            .unwrap_or("");
        let expected_size = member.get("size").and_then(Value::as_u64);

        let full_path = root.join(member_path);

        // Check existence.
        if !full_path.exists() {
            failures.push(MemberFailure {
                path: member_path.to_string(),
                reason: "MISSING".to_string(),
                expected: Some(expected_hash.to_string()),
                actual: None,
                expected_size,
                actual_size: None,
            });
            continue;
        }

        // Stat size.
        let metadata = match std::fs::metadata(&full_path) {
            Ok(m) => m,
            Err(e) => {
                skips.push(MemberSkip {
                    path: member_path.to_string(),
                    reason: "IO_ERROR".to_string(),
                    detail: e.to_string(),
                });
                continue;
            }
        };

        let actual_size = metadata.len();
        if let Some(exp_size) = expected_size
            && actual_size != exp_size
        {
            failures.push(MemberFailure {
                path: member_path.to_string(),
                reason: "SIZE_MISMATCH".to_string(),
                expected: Some(expected_hash.to_string()),
                actual: None,
                expected_size: Some(exp_size),
                actual_size: Some(actual_size),
            });
            continue;
        }

        // Stream-hash the file.
        match stream_hash(&full_path, expected_hash) {
            Ok(actual_hash) => {
                if actual_hash == expected_hash {
                    verified += 1;
                } else {
                    failures.push(MemberFailure {
                        path: member_path.to_string(),
                        reason: "HASH_MISMATCH".to_string(),
                        expected: Some(expected_hash.to_string()),
                        actual: Some(actual_hash),
                        expected_size,
                        actual_size: Some(actual_size),
                    });
                }
            }
            Err(e) => {
                skips.push(MemberSkip {
                    path: member_path.to_string(),
                    reason: "IO_ERROR".to_string(),
                    detail: e,
                });
            }
        }
    }

    let checked = members.len();
    let failed = failures.len();
    let skipped = skips.len();

    MembersResult {
        root: root.display().to_string(),
        checked,
        verified,
        failed,
        skipped,
        failures,
        skips,
    }
}

/// Determine the outcome from member verification results.
///
/// Returns (outcome, exit_code).
pub fn members_outcome(result: &MembersResult, strict: bool) -> (&'static str, u8) {
    if result.failed > 0 {
        ("VERIFY_FAILED", 1)
    } else if result.skipped > 0 {
        if strict {
            ("VERIFY_FAILED", 1)
        } else {
            ("VERIFY_PARTIAL", 1)
        }
    } else {
        ("VERIFY_OK", 0)
    }
}

/// Stream-hash a file using the algorithm indicated by the expected hash prefix.
fn stream_hash(path: &Path, expected_hash: &str) -> Result<String, String> {
    let prefix = expected_hash.split(':').next().unwrap_or("sha256");

    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 8192];

    match prefix {
        "sha256" => {
            let mut hasher = Sha256::new();
            loop {
                let n = file.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            let digest = hasher.finalize();
            Ok(format!("sha256:{:x}", digest))
        }
        "blake3" => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let n = file.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(format!("blake3:{}", hasher.finalize().to_hex()))
        }
        _ => Err(format!("unsupported algorithm: {prefix}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_root(files: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
        }
        let root = dir.path().to_path_buf();
        (dir, root)
    }

    fn hash_sha256(data: &[u8]) -> String {
        let digest = Sha256::digest(data);
        format!("sha256:{:x}", digest)
    }

    fn hash_blake3(data: &[u8]) -> String {
        format!("blake3:{}", blake3::hash(data).to_hex())
    }

    #[test]
    fn all_members_verified() {
        let content = b"hello world";
        let hash = hash_sha256(content);
        let (_dir, root) = make_test_root(&[("a.csv", content)]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "a.csv", "bytes_hash": hash, "size": content.len() }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.checked, 1);
        assert_eq!(result.verified, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn missing_file_is_failure() {
        let (_dir, root) = make_test_root(&[]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "missing.csv", "bytes_hash": "sha256:aaa", "size": 10 }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures[0].reason, "MISSING");
        assert!(result.failures[0].actual.is_none());
    }

    #[test]
    fn size_mismatch_is_failure() {
        let content = b"short";
        let hash = hash_sha256(content);
        let (_dir, root) = make_test_root(&[("a.csv", content)]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "a.csv", "bytes_hash": hash, "size": 99999 }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures[0].reason, "SIZE_MISMATCH");
        assert_eq!(result.failures[0].expected_size, Some(99999));
        assert_eq!(result.failures[0].actual_size, Some(content.len() as u64));
    }

    #[test]
    fn hash_mismatch_is_failure() {
        let content = b"original";
        let (_dir, root) = make_test_root(&[("a.csv", b"modified")]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "a.csv", "bytes_hash": hash_sha256(content), "size": 8 }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.failed, 1);
        assert_eq!(result.failures[0].reason, "HASH_MISMATCH");
        assert!(result.failures[0].actual.is_some());
    }

    #[test]
    fn blake3_algorithm_supported() {
        let content = b"blake3 test";
        let hash = hash_blake3(content);
        let (_dir, root) = make_test_root(&[("b.csv", content)]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "b.csv", "bytes_hash": hash, "size": content.len() }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.verified, 1);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn counts_are_consistent() {
        let content_a = b"aaa";
        let (_dir, root) = make_test_root(&[("a.csv", content_a)]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "a.csv", "bytes_hash": hash_sha256(content_a), "size": content_a.len() },
                { "path": "missing.csv", "bytes_hash": "sha256:xxx", "size": 10 }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.checked, 2);
        assert_eq!(
            result.checked,
            result.verified + result.failed + result.skipped
        );
    }

    #[test]
    fn strict_promotes_partial_to_failed() {
        let result = MembersResult {
            root: "/tmp".to_string(),
            checked: 2,
            verified: 1,
            failed: 0,
            skipped: 1,
            failures: vec![],
            skips: vec![MemberSkip {
                path: "err.csv".to_string(),
                reason: "IO_ERROR".to_string(),
                detail: "permission denied".to_string(),
            }],
        };

        let (outcome, exit_code) = members_outcome(&result, false);
        assert_eq!(outcome, "VERIFY_PARTIAL");
        assert_eq!(exit_code, 1);

        let (outcome, exit_code) = members_outcome(&result, true);
        assert_eq!(outcome, "VERIFY_FAILED");
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn all_verified_returns_ok() {
        let result = MembersResult {
            root: "/tmp".to_string(),
            checked: 1,
            verified: 1,
            failed: 0,
            skipped: 0,
            failures: vec![],
            skips: vec![],
        };

        let (outcome, exit_code) = members_outcome(&result, false);
        assert_eq!(outcome, "VERIFY_OK");
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn failures_return_failed() {
        let result = MembersResult {
            root: "/tmp".to_string(),
            checked: 1,
            verified: 0,
            failed: 1,
            skipped: 0,
            failures: vec![MemberFailure {
                path: "a.csv".to_string(),
                reason: "MISSING".to_string(),
                expected: None,
                actual: None,
                expected_size: None,
                actual_size: None,
            }],
            skips: vec![],
        };

        let (outcome, exit_code) = members_outcome(&result, false);
        assert_eq!(outcome, "VERIFY_FAILED");
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn nested_paths_resolved_against_root() {
        let content = b"nested";
        let hash = hash_sha256(content);
        let (_dir, root) = make_test_root(&[("sub/dir/a.csv", content)]);

        let lockfile = serde_json::json!({
            "members": [
                { "path": "sub/dir/a.csv", "bytes_hash": hash, "size": content.len() }
            ]
        });

        let result = verify_members(&lockfile, &root);
        assert_eq!(result.verified, 1);
    }
}
