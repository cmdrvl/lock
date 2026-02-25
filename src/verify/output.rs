use serde_json::Value;

/// Render a verify result as human-readable text.
///
/// Returns a string suitable for printing to stdout (no trailing newline).
pub fn render_human(result: &Value) -> String {
    let outcome = result["outcome"].as_str().unwrap_or("?");
    let lockfile = result["lockfile"].as_str().unwrap_or("?");

    match outcome {
        "VERIFY_OK" => render_ok(result, lockfile),
        "VERIFY_FAILED" => render_failed(result, lockfile),
        "VERIFY_PARTIAL" => render_partial(result, lockfile),
        "REFUSAL" => render_refusal(result),
        _ => format!("? {lockfile}: unknown outcome {outcome}"),
    }
}

fn render_ok(result: &Value, lockfile: &str) -> String {
    let hash_prefix = result["lock_hash"]["stored"]
        .as_str()
        .and_then(|h| h.get(..15))
        .unwrap_or("?");

    if let Some(members) = result.get("members").filter(|v| !v.is_null()) {
        let checked = members["checked"].as_u64().unwrap_or(0);
        let verified = members["verified"].as_u64().unwrap_or(0);
        format!(
            "\u{2713} {lockfile}: self-hash valid ({hash_prefix}...), {verified}/{checked} members verified"
        )
    } else {
        format!("\u{2713} {lockfile}: self-hash valid ({hash_prefix}...)")
    }
}

fn render_failed(result: &Value, lockfile: &str) -> String {
    let mut lines = Vec::new();

    let hash_valid = result["lock_hash"]["valid"].as_bool().unwrap_or(true);

    if !hash_valid {
        // Tampered self-hash.
        let stored = result["lock_hash"]["stored"].as_str().unwrap_or("?");
        let computed = result["lock_hash"]["computed"].as_str().unwrap_or("?");
        lines.push(format!("\u{2717} {lockfile}: self-hash TAMPERED"));
        lines.push(format!("  stored:   {stored}"));
        lines.push(format!("  computed: {computed}"));
    } else if let Some(members) = result.get("members").filter(|v| !v.is_null()) {
        // Member drift.
        let checked = members["checked"].as_u64().unwrap_or(0);
        let failed = members["failed"].as_u64().unwrap_or(0);
        let verified = members["verified"].as_u64().unwrap_or(0);
        lines.push(format!(
            "\u{2717} {lockfile}: {failed} of {checked} members failed ({verified} verified)"
        ));

        if let Some(failures) = members["failures"].as_array() {
            for f in failures {
                let path = f["path"].as_str().unwrap_or("?");
                let reason = f["reason"].as_str().unwrap_or("?");
                lines.push(format!("  {reason}: {path}"));
            }
        }
    } else {
        lines.push(format!("\u{2717} {lockfile}: VERIFY_FAILED"));
    }

    lines.join("\n")
}

fn render_partial(result: &Value, lockfile: &str) -> String {
    let mut lines = Vec::new();

    if let Some(members) = result.get("members").filter(|v| !v.is_null()) {
        let verified = members["verified"].as_u64().unwrap_or(0);
        let skipped = members["skipped"].as_u64().unwrap_or(0);
        lines.push(format!(
            "\u{26A0} {lockfile}: {verified} verified, {skipped} skipped"
        ));

        if let Some(skips) = members["skips"].as_array() {
            for s in skips {
                let path = s["path"].as_str().unwrap_or("?");
                let reason = s["reason"].as_str().unwrap_or("?");
                let detail = s["detail"].as_str().unwrap_or("");
                lines.push(format!("  {reason}: {path} ({detail})"));
            }
        }
    } else {
        lines.push(format!("\u{26A0} {lockfile}: VERIFY_PARTIAL"));
    }

    lines.join("\n")
}

fn render_refusal(result: &Value) -> String {
    let code = result["refusal"]["code"].as_str().unwrap_or("?");
    let message = result["refusal"]["message"].as_str().unwrap_or("?");
    format!("\u{2717} {code}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_verify_ok_self_hash_only() {
        let result = serde_json::json!({
            "outcome": "VERIFY_OK",
            "lockfile": "dec.lock.json",
            "lock_hash": {
                "stored": "sha256:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
                "computed": "sha256:a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
                "valid": true
            },
            "members": null
        });
        let output = render_human(&result);
        assert!(output.starts_with('\u{2713}'));
        assert!(output.contains("dec.lock.json"));
        assert!(output.contains("self-hash valid"));
        assert!(!output.contains("members"));
    }

    #[test]
    fn render_verify_ok_with_members() {
        let result = serde_json::json!({
            "outcome": "VERIFY_OK",
            "lockfile": "dec.lock.json",
            "lock_hash": { "stored": "sha256:abc123", "computed": "sha256:abc123", "valid": true },
            "members": { "checked": 5, "verified": 5, "failed": 0, "skipped": 0 }
        });
        let output = render_human(&result);
        assert!(output.contains("5/5 members verified"));
    }

    #[test]
    fn render_verify_failed_tampered() {
        let result = serde_json::json!({
            "outcome": "VERIFY_FAILED",
            "lockfile": "dec.lock.json",
            "lock_hash": {
                "stored": "sha256:aaaa",
                "computed": "sha256:bbbb",
                "valid": false
            },
            "members": null
        });
        let output = render_human(&result);
        assert!(output.contains('\u{2717}'));
        assert!(output.contains("TAMPERED"));
        assert!(output.contains("stored:"));
        assert!(output.contains("computed:"));
    }

    #[test]
    fn render_verify_failed_drift() {
        let result = serde_json::json!({
            "outcome": "VERIFY_FAILED",
            "lockfile": "dec.lock.json",
            "lock_hash": { "stored": "sha256:abc", "computed": "sha256:abc", "valid": true },
            "members": {
                "checked": 3,
                "verified": 1,
                "failed": 2,
                "skipped": 0,
                "failures": [
                    { "path": "tape.csv", "reason": "HASH_MISMATCH" },
                    { "path": "draft.xlsx", "reason": "MISSING" }
                ]
            }
        });
        let output = render_human(&result);
        assert!(output.contains("2 of 3 members failed"));
        assert!(output.contains("HASH_MISMATCH: tape.csv"));
        assert!(output.contains("MISSING: draft.xlsx"));
    }

    #[test]
    fn render_verify_partial() {
        let result = serde_json::json!({
            "outcome": "VERIFY_PARTIAL",
            "lockfile": "dec.lock.json",
            "lock_hash": { "stored": "sha256:abc", "computed": "sha256:abc", "valid": true },
            "members": {
                "checked": 3,
                "verified": 2,
                "failed": 0,
                "skipped": 1,
                "skips": [
                    { "path": "locked.csv", "reason": "IO_ERROR", "detail": "permission denied" }
                ]
            }
        });
        let output = render_human(&result);
        assert!(output.contains('\u{26A0}'));
        assert!(output.contains("2 verified, 1 skipped"));
        assert!(output.contains("IO_ERROR: locked.csv"));
    }

    #[test]
    fn render_refusal_output() {
        let result = serde_json::json!({
            "outcome": "REFUSAL",
            "refusal": {
                "code": "E_BAD_LOCKFILE",
                "message": "malformed lockfile JSON"
            }
        });
        let output = render_human(&result);
        assert!(output.contains('\u{2717}'));
        assert!(output.contains("E_BAD_LOCKFILE"));
        assert!(output.contains("malformed lockfile JSON"));
    }
}
