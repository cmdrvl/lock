use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn lock_binary() -> &'static str {
    env!("CARGO_BIN_EXE_lock")
}

fn run_lock(args: &[&str], ledger_path: Option<&Path>) -> Output {
    let mut cmd = Command::new(lock_binary());
    cmd.args(args);
    if let Some(path) = ledger_path {
        cmd.env("EPISTEMIC_WITNESS", path);
    }
    cmd.output().expect("run lock binary")
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    format!("sha256:{:x}", digest)
}

/// Create a temp directory with known files and a JSONL manifest.
/// Returns (dir, manifest_path, data_root) where data_root contains the member files.
fn create_fixture(files: &[(&str, &[u8])]) -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let data_root = dir.path().join("data");
    fs::create_dir_all(&data_root).unwrap();

    let mut manifest_lines = Vec::new();
    for (name, content) in files {
        let file_path = data_root.join(name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file_path, content).unwrap();

        let hash = sha256_hex(content);
        let line = serde_json::json!({
            "version": "hash.v0",
            "relative_path": *name,
            "bytes_hash": hash,
            "size": content.len(),
            "tool_versions": { "hash": "0.1.0" }
        });
        manifest_lines.push(serde_json::to_string(&line).unwrap());
    }

    let manifest_path = dir.path().join("manifest.jsonl");
    fs::write(&manifest_path, manifest_lines.join("\n") + "\n").unwrap();

    (dir, manifest_path, data_root)
}

/// Create a lockfile via the lock binary. Returns the lockfile path and parsed JSON.
fn create_lockfile(manifest_path: &Path, dir: &Path) -> (PathBuf, Value) {
    let output = run_lock(&[manifest_path.to_str().unwrap(), "--no-witness"], None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "lock creation failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let lockfile_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let lockfile_path = dir.join("test.lock.json");
    fs::write(&lockfile_path, &output.stdout).unwrap();

    (lockfile_path, lockfile_json)
}

// ---------------------------------------------------------------------------
// Level 1: Self-hash only
// ---------------------------------------------------------------------------

#[test]
fn verify_level1_valid_lockfile_exits_0_verify_ok() {
    let (_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"hello world")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "VERIFY_OK");
    assert_eq!(parsed["version"], "lock-verify.v0");
    assert_eq!(parsed["lock_hash"]["valid"], true);
    assert!(parsed["members"].is_null());
}

// ---------------------------------------------------------------------------
// Level 2: Self-hash + member verification
// ---------------------------------------------------------------------------

#[test]
fn verify_level2_all_members_verified_exits_0() {
    let (_dir, manifest_path, data_root) =
        create_fixture(&[("a.csv", b"hello world"), ("b.csv", b"goodbye world")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            data_root.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "VERIFY_OK");
    assert_eq!(parsed["lock_hash"]["valid"], true);
    assert_eq!(parsed["members"]["checked"], 2);
    assert_eq!(parsed["members"]["verified"], 2);
    assert_eq!(parsed["members"]["failed"], 0);
}

#[test]
fn verify_level2_modified_file_exits_1_hash_mismatch() {
    let (_dir, manifest_path, data_root) =
        create_fixture(&[("a.csv", b"original content"), ("b.csv", b"unchanged")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Modify a.csv after lockfile creation.
    fs::write(data_root.join("a.csv"), b"modified content").unwrap();

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            data_root.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "VERIFY_FAILED");
    assert_eq!(parsed["lock_hash"]["valid"], true);

    let failures = parsed["members"]["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["path"], "a.csv");
    // Could be SIZE_MISMATCH or HASH_MISMATCH depending on whether size changed.
    let reason = failures[0]["reason"].as_str().unwrap();
    assert!(
        reason == "SIZE_MISMATCH" || reason == "HASH_MISMATCH",
        "expected SIZE_MISMATCH or HASH_MISMATCH, got {reason}"
    );
}

#[test]
fn verify_level2_deleted_file_exits_1_missing() {
    let (_dir, manifest_path, data_root) =
        create_fixture(&[("a.csv", b"will be deleted"), ("b.csv", b"stays")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Delete a.csv after lockfile creation.
    fs::remove_file(data_root.join("a.csv")).unwrap();

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            data_root.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "VERIFY_FAILED");

    let failures = parsed["members"]["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["path"], "a.csv");
    assert_eq!(failures[0]["reason"], "MISSING");
}

// ---------------------------------------------------------------------------
// Tampered lockfile
// ---------------------------------------------------------------------------

#[test]
fn verify_tampered_lockfile_exits_1_hash_invalid_members_null() {
    let (_dir, manifest_path, data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Tamper with the lockfile JSON.
    let content = fs::read_to_string(&lockfile_path).unwrap();
    let tampered = content.replace("\"dataset_id\":null", "\"dataset_id\":\"tampered\"");
    fs::write(&lockfile_path, tampered).unwrap();

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            data_root.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "VERIFY_FAILED");
    assert_eq!(parsed["lock_hash"]["valid"], false);
    assert!(
        parsed["members"].is_null(),
        "members must be null when self-hash fails"
    );
}

// ---------------------------------------------------------------------------
// Refusals (exit 2)
// ---------------------------------------------------------------------------

#[test]
fn verify_root_not_found_exits_2() {
    let (_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            "/nonexistent/dir/that/does/not/exist",
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_ROOT_NOT_FOUND");
}

#[test]
fn verify_malformed_lockfile_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile_path = dir.path().join("bad.lock.json");
    fs::write(&lockfile_path, "not json {{{").unwrap();

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_BAD_LOCKFILE");
}

#[test]
fn verify_missing_lockfile_exits_2() {
    let output = run_lock(
        &[
            "verify",
            "/nonexistent/file.lock.json",
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["outcome"], "REFUSAL");
    assert_eq!(parsed["refusal"]["code"], "E_IO");
}

// ---------------------------------------------------------------------------
// --strict flag
// ---------------------------------------------------------------------------

#[test]
fn verify_strict_promotes_partial_to_failed() {
    let (_dir, manifest_path, data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Make file unreadable to trigger IO_ERROR / skip.
    // On macOS/Linux we can remove read permission.
    let file_path = data_root.join("a.csv");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o000);
        fs::set_permissions(&file_path, perms).unwrap();
    }

    // Only run the strict assertion on unix where we can control permissions.
    #[cfg(unix)]
    {
        // Without --strict: should be VERIFY_PARTIAL or VERIFY_FAILED depending on
        // whether the size check can stat the file (it can — metadata doesn't need read).
        // Actually, metadata works without read permission on unix, and the file exists,
        // so size check passes. The stream_hash open() will fail → IO_ERROR → skip.
        let output = run_lock(
            &[
                "verify",
                lockfile_path.to_str().unwrap(),
                "--root",
                data_root.to_str().unwrap(),
                "--json",
                "--no-witness",
            ],
            None,
        );
        assert_eq!(output.status.code(), Some(1));
        let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(parsed["outcome"], "VERIFY_PARTIAL");
        assert_eq!(parsed["members"]["skipped"], 1);

        // With --strict: VERIFY_PARTIAL becomes VERIFY_FAILED.
        let output = run_lock(
            &[
                "verify",
                lockfile_path.to_str().unwrap(),
                "--root",
                data_root.to_str().unwrap(),
                "--json",
                "--no-witness",
                "--strict",
            ],
            None,
        );
        assert_eq!(output.status.code(), Some(1));
        let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(parsed["outcome"], "VERIFY_FAILED");

        // Restore permissions for cleanup.
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Output format: --json vs human-readable
// ---------------------------------------------------------------------------

#[test]
fn verify_json_output_is_valid_json() {
    let (_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed.is_object(), "--json output must be a JSON object");
    assert_eq!(parsed["version"], "lock-verify.v0");
}

#[test]
fn verify_human_output_has_unicode_checkmark() {
    let (_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Default (no --json) should produce human-readable output.
    let output = run_lock(
        &["verify", lockfile_path.to_str().unwrap(), "--no-witness"],
        None,
    );
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('\u{2713}'),
        "human output must contain checkmark (✓), got: {stdout}"
    );
    assert!(
        stdout.contains("self-hash valid"),
        "human output must mention self-hash valid, got: {stdout}"
    );
}

#[test]
fn verify_human_output_failed_has_cross() {
    let (_dir, manifest_path, data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _dir.path());

    // Delete the file to cause MISSING failure.
    fs::remove_file(data_root.join("a.csv")).unwrap();

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--root",
            data_root.to_str().unwrap(),
            "--no-witness",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('\u{2717}'),
        "failed human output must contain cross (✗), got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Witness integration
// ---------------------------------------------------------------------------

#[test]
fn verify_appends_witness_record_with_subcommand() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("witness.jsonl");

    let (_fixture_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _fixture_dir.path());

    let output = run_lock(
        &["verify", lockfile_path.to_str().unwrap(), "--json"],
        Some(&ledger_path),
    );
    assert_eq!(output.status.code(), Some(0));

    let content = fs::read_to_string(&ledger_path).expect("witness ledger must exist");
    let record: Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
    assert_eq!(record["tool"], "lock");
    assert_eq!(record["outcome"], "VERIFY_OK");
    assert_eq!(record["params"]["subcommand"], "verify");
    assert_eq!(record["exit_code"], 0);
}

#[test]
fn verify_no_witness_suppresses_append() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("witness.jsonl");

    let (_fixture_dir, manifest_path, _data_root) = create_fixture(&[("a.csv", b"data")]);
    let (lockfile_path, _) = create_lockfile(&manifest_path, _fixture_dir.path());

    let output = run_lock(
        &[
            "verify",
            lockfile_path.to_str().unwrap(),
            "--json",
            "--no-witness",
        ],
        Some(&ledger_path),
    );
    assert_eq!(output.status.code(), Some(0));

    assert!(
        !ledger_path.exists(),
        "--no-witness must suppress witness append"
    );
}

// ---------------------------------------------------------------------------
// No regressions: existing lock creation still works
// ---------------------------------------------------------------------------

#[test]
fn existing_lock_creation_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("input.jsonl");
    fs::write(
        &manifest_path,
        r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaaaaaa","size":10,"tool_versions":{"hash":"0.1.0"}}
"#,
    )
    .unwrap();

    let output = run_lock(&[manifest_path.to_str().unwrap(), "--no-witness"], None);
    assert_eq!(output.status.code(), Some(0));

    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["version"], "lock.v0");
    assert_eq!(parsed["member_count"], 1);
}
