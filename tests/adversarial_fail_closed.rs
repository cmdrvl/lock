use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use lock::lockfile::self_hash::verify_lock_hash_detail_value;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod support;

const CHILD_TIMEOUT: Duration = Duration::from_secs(5);

fn run_bounded<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = support::lock_command("adversarial-fail-closed")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn lock child");
    let child_id = child.id();
    let started = Instant::now();

    loop {
        if child.try_wait().expect("poll lock child").is_some() {
            return child.wait_with_output().expect("collect lock child output");
        }
        if started.elapsed() >= CHILD_TIMEOUT {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("collect timed-out lock child output");
            assert!(
                started.elapsed() < CHILD_TIMEOUT,
                "lock child {child_id} exceeded {CHILD_TIMEOUT:?}"
            );
            return output;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn sha256_hex(data: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(data))
}

fn create_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Value) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    fs::create_dir(&root).unwrap();
    let content = b"stable member content";
    let member = root.join("member.csv");
    fs::write(&member, content).unwrap();

    let manifest = tmp.path().join("manifest.jsonl");
    fs::write(
        &manifest,
        format!(
            "{}\n",
            json!({
                "version": "hash.v0",
                "relative_path": "member.csv",
                "bytes_hash": sha256_hex(content),
                "size": content.len(),
                "tool_versions": {"hash": "0.1.0"}
            })
        ),
    )
    .unwrap();

    let created = run_bounded([
        manifest.to_str().unwrap(),
        "--created",
        "2026-01-15T00:00:00Z",
        "--no-witness",
    ]);
    assert!(
        created.status.success(),
        "lock creation failed: {}",
        String::from_utf8_lossy(&created.stdout)
    );
    let lockfile: Value = serde_json::from_slice(&created.stdout).unwrap();
    let lock_path = tmp.path().join("fixture.lock.json");
    fs::write(&lock_path, &created.stdout).unwrap();
    (tmp, root, lock_path, lockfile)
}

fn verify_args(lock_path: &Path, root: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        "verify".to_owned(),
        lock_path.display().to_string(),
        "--json".to_owned(),
        "--no-witness".to_owned(),
    ];
    if let Some(root) = root {
        args.extend(["--root".to_owned(), root.display().to_string()]);
    }
    args
}

fn assert_refusal(output: Output, expected_code: &str) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={stdout}\nstderr={stderr}"
    );
    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    let parsed: Result<Value, _> = serde_json::from_slice(&output.stdout);
    assert!(parsed.is_ok(), "refusal stdout is not JSON: {stdout}");
    let envelope = parsed.unwrap_or(Value::Null);
    assert_eq!(envelope["outcome"], "REFUSAL");
    assert_eq!(envelope["refusal"]["code"], expected_code);
    envelope
}

fn rewrite_lock(lock_path: &Path, lockfile: &Value, change: impl FnOnce(&mut Value)) {
    let mut changed = lockfile.clone();
    change(&mut changed);
    let computed = verify_lock_hash_detail_value(&changed).unwrap().computed;
    changed["lock_hash"] = json!(computed);
    fs::write(lock_path, serde_json::to_vec(&changed).unwrap()).unwrap();
}

#[test]
fn verify_adversarial_matrix_is_bounded_and_fail_closed() {
    let (tmp, root, lock_path, lockfile) = create_fixture();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let member = root.join("member.csv");
        fs::set_permissions(&member, fs::Permissions::from_mode(0o000)).unwrap();
        let output = run_bounded(verify_args(&lock_path, Some(&root)));
        let refusal = assert_refusal(output, "E_IO");
        assert_eq!(refusal["refusal"]["detail"]["path"], "member.csv");
        fs::set_permissions(&member, fs::Permissions::from_mode(0o644)).unwrap();
    }

    fs::remove_file(root.join("member.csv")).unwrap();
    fs::create_dir(root.join("member.csv")).unwrap();
    let output = run_bounded(verify_args(&lock_path, Some(&root)));
    let refusal = assert_refusal(output, "E_BAD_LOCKFILE");
    assert_eq!(refusal["refusal"]["detail"]["reason"], "NON_REGULAR");
    fs::remove_dir(root.join("member.csv")).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = tmp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"outside secret").unwrap();

        symlink(outside.join("secret.txt"), root.join("direct-link")).unwrap();
        rewrite_lock(&lock_path, &lockfile, |value| {
            value["members"][0]["path"] = json!("direct-link");
        });
        let output = run_bounded(verify_args(&lock_path, Some(&root)));
        let refusal = assert_refusal(output, "E_BAD_LOCKFILE");
        assert_eq!(refusal["refusal"]["detail"]["reason"], "SYMLINK");
        fs::remove_file(root.join("direct-link")).unwrap();

        symlink(&outside, root.join("parent-link")).unwrap();
        rewrite_lock(&lock_path, &lockfile, |value| {
            value["members"][0]["path"] = json!("parent-link/secret.txt");
        });
        let output = run_bounded(verify_args(&lock_path, Some(&root)));
        let refusal = assert_refusal(output, "E_BAD_LOCKFILE");
        assert_eq!(refusal["refusal"]["detail"]["reason"], "SYMLINK");
        fs::remove_file(root.join("parent-link")).unwrap();

        fs::create_dir(root.join("inside")).unwrap();
        fs::write(root.join("inside/secret.txt"), b"inside secret").unwrap();
        symlink(root.join("inside"), root.join("inside-link")).unwrap();
        rewrite_lock(&lock_path, &lockfile, |value| {
            value["members"][0]["path"] = json!("inside-link/secret.txt");
        });
        let output = run_bounded(verify_args(&lock_path, Some(&root)));
        let refusal = assert_refusal(output, "E_BAD_LOCKFILE");
        assert_eq!(refusal["refusal"]["detail"]["reason"], "SYMLINK");
        fs::remove_file(root.join("inside-link")).unwrap();
    }

    fs::write(root.join("member.csv"), b"stable member content").unwrap();

    for (_, path) in [("traversal", "../outside.txt"), ("absolute", "/etc/passwd")] {
        rewrite_lock(&lock_path, &lockfile, |value| {
            value["members"][0]["path"] = json!(path);
        });
        let output = run_bounded(verify_args(&lock_path, Some(&root)));
        assert_refusal(output, "E_BAD_LOCKFILE");
    }

    rewrite_lock(&lock_path, &lockfile, |value| {
        value["members"][0]["path"] = json!("./member.csv");
    });
    assert_refusal(
        run_bounded(verify_args(&lock_path, Some(&root))),
        "E_BAD_LOCKFILE",
    );

    rewrite_lock(&lock_path, &lockfile, |value| {
        let duplicate = value["members"][0].clone();
        value["members"] = json!([duplicate.clone(), duplicate]);
    });
    assert_refusal(
        run_bounded(verify_args(&lock_path, Some(&root))),
        "E_BAD_LOCKFILE",
    );

    rewrite_lock(&lock_path, &lockfile, |value| {
        value["members"][0]["bytes_hash"] = json!("md5:deadbeef");
    });
    assert_refusal(
        run_bounded(verify_args(&lock_path, Some(&root))),
        "E_UNKNOWN_ALGORITHM",
    );

    let sha256_digest = lockfile["members"][0]["bytes_hash"]
        .as_str()
        .unwrap()
        .strip_prefix("sha256:")
        .unwrap()
        .to_owned();
    rewrite_lock(&lock_path, &lockfile, |value| {
        value["members"][0]["bytes_hash"] = json!(format!("blake3:{sha256_digest}"));
    });
    let output = run_bounded(verify_args(&lock_path, Some(&root)));
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(parsed["outcome"], "VERIFY_FAILED");
    assert_eq!(parsed["members"]["failures"][0]["reason"], "HASH_MISMATCH");

    // A valid lock whose member bytes changed is a domain negative, not a refusal.
    fs::write(root.join("member.csv"), b"tampered bytes").unwrap();
    fs::write(&lock_path, serde_json::to_vec(&lockfile).unwrap()).unwrap();
    let output = run_bounded(verify_args(&lock_path, Some(&root)));
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    assert_eq!(parsed["outcome"], "VERIFY_FAILED");
    assert_eq!(parsed["members"]["failures"][0]["reason"], "SIZE_MISMATCH");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o000);
        fs::set_permissions(&lock_path, permissions).unwrap();
        assert_refusal(run_bounded(verify_args(&lock_path, None)), "E_IO");
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644)).unwrap();
    }

    let lock_directory = tmp.path().join("lock-directory");
    fs::create_dir(&lock_directory).unwrap();
    assert_refusal(run_bounded(verify_args(&lock_directory, None)), "E_IO");
}
