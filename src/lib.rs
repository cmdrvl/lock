#![deny(unsafe_code)]

use chrono::{SecondsFormat, Utc};

pub mod cli;
pub mod input;
pub mod lockfile;
pub mod output;
pub mod refusal;
pub mod witness;

pub fn run() -> u8 {
    cli::run()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrchestrationOutput {
    outcome: output::DomainOutcome,
    payload_json: String,
}

pub fn run_lock(cli: &cli::Cli) -> u8 {
    let orchestrated = match input::read_jsonl(cli.input.as_deref()) {
        Ok(read_result) => orchestrate_from_read_result(cli, read_result),
        Err(error) => match error {
            input::InputError::Parse(detail) => {
                refusal_output(refusal::bad_input_parse(detail.line, &detail.error))
            }
            input::InputError::Io(io_error) => {
                refusal_output(refusal::bad_input_parse(0, &io_error.to_string()))
            }
        },
    };

    print!("{}", orchestrated.payload_json);

    // Append witness record unless --no-witness.
    if !cli.no_witness {
        let input_path = cli
            .input
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "stdin".to_string());

        witness::append_witness_record(
            orchestrated.outcome,
            orchestrated.payload_json.as_bytes(),
            &witness::WitnessParams {
                input_path,
                dataset_id: cli.dataset_id.clone(),
                as_of: cli.as_of.clone(),
                note: cli.note.clone(),
            },
        );
    }

    orchestrated.outcome.exit_code()
}

fn orchestrate_from_read_result(
    cli: &cli::Cli,
    read_result: input::ReadResult,
) -> OrchestrationOutput {
    let input::ReadResult::Records(records) = read_result else {
        return refusal_output(refusal::empty());
    };

    if let Err(error) = input::validate_records(&records) {
        return match error {
            input::ValidationError::BadVersion(detail) => {
                let version = detail.version.as_deref().unwrap_or("<missing>");
                refusal_output(refusal::bad_input_version(detail.line, version))
            }
            input::ValidationError::MissingHash(detail) => {
                refusal_output(refusal::missing_hash(detail.count, detail.sample_paths))
            }
        };
    }

    let classification = match lockfile::classify_records(&records) {
        Ok(classification) => classification,
        Err(error) => {
            let (line, message) = match error {
                lockfile::ClassificationError::MissingPath { line_number } => {
                    (line_number, "missing path/relative_path")
                }
                lockfile::ClassificationError::MissingBytesHash { line_number } => {
                    (line_number, "missing bytes_hash")
                }
                lockfile::ClassificationError::MissingSize { line_number } => {
                    (line_number, "missing size")
                }
            };
            return refusal_output(refusal::bad_input_parse(line, message));
        }
    };

    let metadata = lockfile::hydrate_metadata(
        &records,
        env!("CARGO_PKG_VERSION"),
        cli.dataset_id.as_deref(),
        cli.as_of.as_deref(),
        cli.note.as_deref(),
    );

    let mut lockfile = lockfile::Lockfile {
        version: refusal::LOCK_VERSION.to_owned(),
        lock_hash: String::new(),
        dataset_id: metadata.dataset_id,
        as_of: metadata.as_of,
        note: metadata.note,
        created: current_created_timestamp(),
        tool_versions: metadata.tool_versions,
        profiles: metadata.profiles,
        skipped: classification.skipped,
        members: classification.members,
        skipped_count: classification.skipped_count,
        member_count: classification.member_count,
    };

    lockfile.lock_hash = lockfile::self_hash::compute_lock_hash(&lockfile);

    match output::render_lockfile(&lockfile) {
        Ok(artifact) => OrchestrationOutput {
            outcome: artifact.outcome,
            payload_json: artifact.json,
        },
        Err(error) => refusal_output(refusal::bad_input_parse(0, &error.to_string())),
    }
}

fn refusal_output(envelope: refusal::RefusalEnvelope) -> OrchestrationOutput {
    OrchestrationOutput {
        outcome: output::DomainOutcome::Refusal,
        payload_json: envelope.to_json(),
    }
}

fn current_created_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path, path::PathBuf};

    use serde_json::json;
    use tempfile::TempDir;

    use super::{orchestrate_from_read_result, output, run_lock};
    use crate::{cli, input};

    fn make_cli() -> cli::Cli {
        cli::Cli {
            command: None,
            input: None,
            dataset_id: Some("dataset-a".to_owned()),
            as_of: Some("2026-02-24T00:00:00Z".to_owned()),
            note: Some("note".to_owned()),
            no_witness: false,
            describe: false,
            schema: false,
        }
    }

    #[test]
    fn orchestration_maps_empty_input_to_refusal() {
        let cli = make_cli();

        let output = orchestrate_from_read_result(&cli, input::ReadResult::Empty);

        assert_eq!(output.outcome, output::DomainOutcome::Refusal);
        let parsed: serde_json::Value =
            serde_json::from_str(&output.payload_json).expect("valid JSON");
        assert_eq!(parsed["outcome"], "REFUSAL");
        assert_eq!(parsed["refusal"]["code"], "E_EMPTY");
    }

    #[test]
    fn orchestration_maps_missing_hash_to_refusal() {
        let cli = make_cli();
        let records = vec![input::InputRecord {
            line_number: 1,
            value: json!({
                "version": "hash.v0",
                "relative_path": "a.csv"
            }),
        }];

        let output = orchestrate_from_read_result(&cli, input::ReadResult::Records(records));

        assert_eq!(output.outcome, output::DomainOutcome::Refusal);
        let parsed: serde_json::Value =
            serde_json::from_str(&output.payload_json).expect("valid JSON");
        assert_eq!(parsed["refusal"]["code"], "E_MISSING_HASH");
    }

    #[test]
    fn orchestration_maps_skipped_records_to_lock_partial() {
        let cli = make_cli();
        let records = vec![
            input::InputRecord {
                line_number: 1,
                value: json!({
                    "version": "hash.v0",
                    "relative_path": "a.csv",
                    "bytes_hash": "sha256:aaaa",
                    "size": 1
                }),
            },
            input::InputRecord {
                line_number: 2,
                value: json!({
                    "version": "hash.v0",
                    "_skipped": true,
                    "relative_path": "b.csv",
                    "_warnings": []
                }),
            },
        ];

        let output = orchestrate_from_read_result(&cli, input::ReadResult::Records(records));

        assert_eq!(output.outcome, output::DomainOutcome::LockPartial);
        let parsed: serde_json::Value =
            serde_json::from_str(&output.payload_json).expect("valid JSON");
        assert_eq!(parsed["skipped_count"], 1);
        assert_eq!(parsed["member_count"], 1);
    }

    #[test]
    fn orchestration_maps_complete_records_to_lock_created() {
        let cli = make_cli();
        let records = vec![input::InputRecord {
            line_number: 1,
            value: json!({
                "version": "hash.v0",
                "relative_path": "a.csv",
                "bytes_hash": "sha256:aaaa",
                "size": 1,
                "tool_versions": { "hash": "0.1.0" }
            }),
        }];

        let output = orchestrate_from_read_result(&cli, input::ReadResult::Records(records));

        assert_eq!(output.outcome, output::DomainOutcome::LockCreated);
        let parsed: serde_json::Value =
            serde_json::from_str(&output.payload_json).expect("valid JSON");
        assert_eq!(parsed["skipped_count"], 0);
        assert_eq!(parsed["member_count"], 1);
        assert_eq!(parsed["version"], "lock.v0");
    }

    #[test]
    fn run_lock_returns_refusal_for_missing_input_file() {
        let mut cli = make_cli();
        cli.input = Some("does-not-exist.jsonl".into());

        let code = run_lock(&cli);

        assert_eq!(code, 2);
    }

    #[test]
    fn run_lock_appends_witness_for_lock_created_by_default() {
        let (_input_dir, input_path) = write_input_file(concat!(
            r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaa","size":1,"tool_versions":{"hash":"0.1.0"}}"#,
            "\n"
        ));
        let ledger_dir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = ledger_dir.path().join("witness.jsonl");
        let _guard = EnvGuard::set("EPISTEMIC_WITNESS", &ledger_path);

        let code = run_lock(&make_file_cli(input_path, false));

        assert_eq!(code, 0);
        let record = read_single_witness_record(&ledger_path);
        assert_eq!(record["outcome"], "LOCK_CREATED");
        assert_eq!(record["exit_code"], 0);
    }

    #[test]
    fn run_lock_appends_witness_for_lock_partial_by_default() {
        let (_input_dir, input_path) = write_input_file(concat!(
            r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaa","size":1,"tool_versions":{"hash":"0.1.0"}}"#,
            "\n",
            r#"{"version":"hash.v0","_skipped":true,"relative_path":"b.csv","_warnings":[]}"#,
            "\n"
        ));
        let ledger_dir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = ledger_dir.path().join("witness.jsonl");
        let _guard = EnvGuard::set("EPISTEMIC_WITNESS", &ledger_path);

        let code = run_lock(&make_file_cli(input_path, false));

        assert_eq!(code, 1);
        let record = read_single_witness_record(&ledger_path);
        assert_eq!(record["outcome"], "LOCK_PARTIAL");
        assert_eq!(record["exit_code"], 1);
    }

    #[test]
    fn run_lock_appends_witness_for_refusal_by_default() {
        let (_input_dir, input_path) = write_input_file(concat!(
            r#"{"version":"hash.v0","relative_path":"a.csv","size":1}"#,
            "\n"
        ));
        let ledger_dir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = ledger_dir.path().join("witness.jsonl");
        let _guard = EnvGuard::set("EPISTEMIC_WITNESS", &ledger_path);

        let code = run_lock(&make_file_cli(input_path, false));

        assert_eq!(code, 2);
        let record = read_single_witness_record(&ledger_path);
        assert_eq!(record["outcome"], "REFUSAL");
        assert_eq!(record["exit_code"], 2);
    }

    #[test]
    fn run_lock_no_witness_suppresses_append() {
        let (_input_dir, input_path) = write_input_file(concat!(
            r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaa","size":1}"#,
            "\n"
        ));
        let ledger_dir = tempfile::tempdir().expect("create temp dir");
        let ledger_path = ledger_dir.path().join("witness.jsonl");
        let _guard = EnvGuard::set("EPISTEMIC_WITNESS", &ledger_path);

        let code = run_lock(&make_file_cli(input_path, true));

        assert_eq!(code, 0);
        assert!(!ledger_path.exists());
    }

    #[test]
    fn run_lock_keeps_exit_semantics_when_witness_append_fails() {
        let (_input_dir, input_path) = write_input_file(concat!(
            r#"{"version":"hash.v0","relative_path":"a.csv","bytes_hash":"sha256:aaaa","size":1}"#,
            "\n"
        ));
        let _guard = EnvGuard::set("EPISTEMIC_WITNESS", Path::new("/dev/null/witness.jsonl"));

        let code = run_lock(&make_file_cli(input_path, false));

        assert_eq!(code, 0);
    }

    fn make_file_cli(input: PathBuf, no_witness: bool) -> cli::Cli {
        let mut cli = make_cli();
        cli.input = Some(input);
        cli.no_witness = no_witness;
        cli
    }

    fn write_input_file(contents: &str) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("input.jsonl");
        fs::write(&path, contents).expect("write input file");
        (dir, path)
    }

    fn read_single_witness_record(path: &Path) -> serde_json::Value {
        let content = fs::read_to_string(path).expect("read witness ledger");
        let mut lines = content.lines();
        let line = lines.next().expect("witness record must exist");
        assert!(
            lines.next().is_none(),
            "expected exactly one witness record"
        );
        serde_json::from_str(line).expect("parse witness record")
    }

    #[allow(unsafe_code)]
    struct EnvGuard {
        key: String,
        original: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    #[allow(unsafe_code)]
    impl EnvGuard {
        fn set(key: &str, value: &Path) -> Self {
            let lock = crate::witness::TEST_ENV_LOCK
                .lock()
                .expect("lock env mutex");
            let original = env::var(key).ok();
            // SAFETY: Test-only env mutation guarded by global test mutex.
            unsafe { env::set_var(key, value) };
            Self {
                key: key.to_owned(),
                original,
                _lock: lock,
            }
        }
    }

    #[allow(unsafe_code)]
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: Test-only env restoration guarded by global test mutex.
                Some(value) => unsafe { env::set_var(&self.key, value) },
                // SAFETY: Test-only env restoration guarded by global test mutex.
                None => unsafe { env::remove_var(&self.key) },
            }
        }
    }
}
