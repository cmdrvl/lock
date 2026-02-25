use std::collections::BTreeMap;

use lock::lockfile::self_hash::to_canonical_json;
use lock::lockfile::{FingerprintResult, Lockfile, Member, SkippedEntry, Warning};
use lock::output::render_lockfile;
use lock::refusal;

fn fixture_lockfile() -> Lockfile {
    Lockfile {
        version: "lock.v0".to_owned(),
        lock_hash: "sha256:fixture-lock-hash".to_owned(),
        dataset_id: Some("dataset-golden".to_owned()),
        as_of: Some("2026-01-31T00:00:00Z".to_owned()),
        note: Some("fixture note".to_owned()),
        created: "2026-02-01T00:00:00Z".to_owned(),
        tool_versions: BTreeMap::from([
            ("fingerprint".to_owned(), "0.1.0".to_owned()),
            ("hash".to_owned(), "0.1.0".to_owned()),
            ("lock".to_owned(), "0.1.0".to_owned()),
            ("vacuum".to_owned(), "0.1.0".to_owned()),
        ]),
        profiles: vec![],
        skipped: vec![SkippedEntry {
            path: "skip/me.csv".to_owned(),
            warnings: vec![Warning {
                tool: "vacuum".to_owned(),
                code: "W_SKIPPED".to_owned(),
                message: "fixture skip".to_owned(),
                detail: BTreeMap::from([("reason".to_owned(), "fixture".to_owned())]),
            }],
        }],
        members: vec![
            Member {
                path: "a.csv".to_owned(),
                bytes_hash: "sha256:aaaaaaaa".to_owned(),
                size: 10,
                fingerprint: Some(FingerprintResult {
                    fingerprint_id: "fp.v1".to_owned(),
                    fingerprint_version: "0.3.0".to_owned(),
                    matched: true,
                    content_hash: Some(
                        "blake3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                            .to_owned(),
                    ),
                }),
            },
            Member {
                path: "b.csv".to_owned(),
                bytes_hash: "sha256:bbbbbbbb".to_owned(),
                size: 20,
                fingerprint: None,
            },
        ],
        skipped_count: 1,
        member_count: 2,
    }
}

#[test]
fn golden_lockfile_render_matches_fixture() {
    let actual = render_lockfile(&fixture_lockfile()).unwrap().json;
    let expected_value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/golden/lock_created.json")).unwrap();
    let expected = to_canonical_json(&expected_value).unwrap();

    assert_eq!(actual, expected);
    assert!(!actual.ends_with('\n'));
}

#[test]
fn golden_refusal_missing_hash_matches_fixture() {
    let actual = refusal::missing_hash(2, vec!["a.csv".to_owned(), "b.csv".to_owned()]).to_json();
    let expected_value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/golden/refusal_missing_hash.json")).unwrap();
    let expected = to_canonical_json(&expected_value).unwrap();

    assert_eq!(actual, expected);
    assert!(!actual.ends_with('\n'));
}
