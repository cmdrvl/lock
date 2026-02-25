use crate::lockfile::Lockfile;
use crate::lockfile::self_hash::to_canonical_json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainOutcome {
    LockCreated,
    LockPartial,
    Refusal,
}

impl DomainOutcome {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::LockCreated => 0,
            Self::LockPartial => 1,
            Self::Refusal => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOutput {
    pub outcome: DomainOutcome,
    pub json: String,
}

pub fn outcome_from_lockfile(lockfile: &Lockfile) -> DomainOutcome {
    if lockfile.skipped.is_empty() {
        DomainOutcome::LockCreated
    } else {
        DomainOutcome::LockPartial
    }
}

pub fn render_lockfile(lockfile: &Lockfile) -> Result<ArtifactOutput, serde_json::Error> {
    Ok(ArtifactOutput {
        outcome: outcome_from_lockfile(lockfile),
        json: to_canonical_json(lockfile)?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{DomainOutcome, outcome_from_lockfile, render_lockfile};
    use crate::lockfile::{Lockfile, Member, SkippedEntry, Warning};

    fn make_lockfile() -> Lockfile {
        Lockfile {
            version: "lock.v0".to_owned(),
            lock_hash: "sha256:abc".to_owned(),
            dataset_id: Some("dataset-a".to_owned()),
            as_of: None,
            note: None,
            created: "2026-02-24T00:00:00Z".to_owned(),
            tool_versions: BTreeMap::from([("lock".to_owned(), "0.1.0".to_owned())]),
            profiles: vec![],
            skipped: vec![],
            members: vec![Member {
                path: "a.csv".to_owned(),
                bytes_hash: "sha256:aaaa".to_owned(),
                size: 10,
                fingerprint: None,
            }],
            skipped_count: 0,
            member_count: 1,
        }
    }

    #[test]
    fn outcome_is_lock_created_when_no_skipped_records() {
        let lockfile = make_lockfile();
        assert_eq!(outcome_from_lockfile(&lockfile), DomainOutcome::LockCreated);
    }

    #[test]
    fn outcome_is_lock_partial_when_skipped_records_exist() {
        let mut lockfile = make_lockfile();
        lockfile.skipped = vec![SkippedEntry {
            path: "missing.csv".to_owned(),
            warnings: vec![Warning {
                tool: "hash".to_owned(),
                code: "E_IO".to_owned(),
                message: "Cannot read file".to_owned(),
                detail: BTreeMap::new(),
            }],
        }];
        lockfile.skipped_count = 1;

        assert_eq!(outcome_from_lockfile(&lockfile), DomainOutcome::LockPartial);
    }

    #[test]
    fn render_lockfile_is_compact_and_json() {
        let lockfile = make_lockfile();

        let rendered = render_lockfile(&lockfile).expect("render should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered.json).expect("rendered output should parse");

        assert_eq!(rendered.outcome, DomainOutcome::LockCreated);
        assert_eq!(parsed["version"], "lock.v0");
        assert_eq!(parsed["member_count"], 1);
        assert!(!rendered.json.ends_with('\n'));
    }
}
