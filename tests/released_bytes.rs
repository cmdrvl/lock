//! Bytes and hashes produced by the released lock 0.7.0 binary must be reproduced exactly.
//!
//! `hostile.lock.json` is that binary's output for `hostile.jsonl`; the literals below are its
//! output too. The inputs hold member names that sort differently under RFC 8785 (U+E000 and
//! U+10000) and numbers RFC 8785 writes differently (`1.0`, `1e+21`), so a switch to
//! `cmdrvl_core::jcs` fails here. Do not regenerate any of this from the code under test.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lock::lockfile::self_hash::{
    to_canonical_json, verify_lock_hash_detail, verify_lock_hash_from_json,
};
use serde_json::Value;

const HOSTILE_INPUT: &str = include_str!("fixtures/released-0.7.0/hostile.jsonl");
const HOSTILE_LOCK: &str = include_str!("fixtures/released-0.7.0/hostile.lock.json");
const FLOATS_LOCK: &str = include_str!("fixtures/released-0.7.0/floats.lock.json");

/// `lock_hash.computed` printed by released `lock verify floats.lock.json --json`.
const FLOATS_COMPUTED: &str =
    "sha256:d432b39aa3a7c4baace0c729a33b911d1a3d3f6f60f21eb802f1f0aa2709104e";

const PARSER_FLOATS_LOCK: &str = r#"{"version":"lock.v0","lock_hash":"sha256:feed","members":[],"x":[9.335744933994957e-17,5.084767197776064e190,2.6104882325689648e-130]}"#;
/// `lock_hash.computed` printed by released `lock verify` for [`PARSER_FLOATS_LOCK`].
const PARSER_FLOATS_COMPUTED: &str =
    "sha256:303ce1df8e3a5a08da4dc255f610ae5b78dcb8de70bd845c45d64469a3011d0f";

const RELEASED_MISSING_HASH: &str = r#"{"outcome":"REFUSAL","refusal":{"code":"E_MISSING_HASH","detail":{"count":1,"sample_paths":["a.csv"]},"message":"1 record lacks bytes_hash — run hashbytes first","next_command":"vacuum <path> | hashbytes | lock --dataset-id \"<dataset>\" > dataset.lock.json"},"version":"lock.v0"}"#;

const RELEASED_VERIFY_IO: &str = r#"{"outcome":"REFUSAL","refusal":{"code":"E_IO","detail":{"error":"No such file or directory (os error 2)","path":"nope.lock.json"},"message":"cannot read lockfile — No such file or directory (os error 2)","next_command":null},"version":"lock-verify.v0"}"#;

fn lock_in(dir: &Path, ledger: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lock"))
        .current_dir(dir)
        .env("EPISTEMIC_WITNESS", ledger)
        .env_remove("SOURCE_DATE_EPOCH")
        .args(args)
        .output()
        .expect("run lock")
}

fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let ledger = dir.path().join("witness.jsonl");
    (dir, ledger)
}

fn b3(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

#[test]
fn released_lockfile_is_created_byte_for_byte() {
    let (dir, ledger) = scratch();
    std::fs::write(dir.path().join("hostile.jsonl"), HOSTILE_INPUT).expect("write input");

    let output = lock_in(
        dir.path(),
        &ledger,
        &["hostile.jsonl", "--created", "2026-01-15T10:30:00Z"],
    );

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), HOSTILE_LOCK);

    // The witness record hashes the exact input and output bytes, and its id is the hash of the
    // ledger line with an empty id.
    let line = std::fs::read_to_string(&ledger).expect("ledger");
    let line = line.strip_suffix('\n').expect("one line");
    let record: Value = serde_json::from_str(line).expect("record");
    assert_eq!(record["inputs"][0]["hash"], b3(HOSTILE_INPUT.as_bytes()));
    assert_eq!(record["output_hash"], b3(HOSTILE_LOCK.as_bytes()));
    let id = record["id"].as_str().expect("id");
    let blanked = line.replace(&format!("\"id\":\"{id}\""), "\"id\":\"\"");
    assert_eq!(id, b3(blanked.as_bytes()));
}

#[test]
fn released_lockfile_verifies_and_reserializes_identically() {
    assert!(HOSTILE_LOCK.contains('\u{e000}') && HOSTILE_LOCK.contains('\u{10000}'));
    let value: Value = serde_json::from_str(HOSTILE_LOCK).expect("released lockfile");
    assert_eq!(to_canonical_json(&value).expect("serialize"), HOSTILE_LOCK);
    assert!(verify_lock_hash_from_json(HOSTILE_LOCK).expect("parse"));
}

#[test]
fn released_lock_hash_over_floats_and_hostile_keys_is_reproduced() {
    let detail = verify_lock_hash_detail(FLOATS_LOCK).expect("parse");
    assert_eq!(detail.computed, FLOATS_COMPUTED);
    assert_eq!(detail.stored, "sha256:feed");
    assert!(!detail.valid);
}

/// serde_json's default float parser reads these strings one ULP away from the correctly rounded
/// value, and released lock hashes what that parser reads. Enabling `float_roundtrip` (which
/// cmdrvl-core's default features do) changes this `lock_hash`.
#[test]
fn released_float_parsing_is_unchanged() {
    let detail = verify_lock_hash_detail(PARSER_FLOATS_LOCK).expect("parse");
    assert_eq!(detail.computed, PARSER_FLOATS_COMPUTED);
}

#[test]
fn released_refusals_are_reproduced() {
    let (dir, ledger) = scratch();
    std::fs::write(
        dir.path().join("missing.jsonl"),
        "{\"version\":\"hash.v0\",\"relative_path\":\"a.csv\",\"size\":1}\n",
    )
    .expect("write input");

    let create = lock_in(dir.path(), &ledger, &["missing.jsonl", "--no-witness"]);
    assert_eq!(create.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&create.stdout),
        RELEASED_MISSING_HASH
    );

    let verify = lock_in(
        dir.path(),
        &ledger,
        &["verify", "nope.lock.json", "--json", "--no-witness"],
    );
    assert_eq!(verify.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&verify.stdout), RELEASED_VERIFY_IO);
}
