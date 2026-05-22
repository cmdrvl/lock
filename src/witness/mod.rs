use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::WitnessFilters;

#[cfg(test)]
static TEST_ENV_OVERRIDE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
static TEST_ENV_OVERRIDE: std::sync::Mutex<Option<Option<String>>> = std::sync::Mutex::new(None);

const TOOL_NAME: &str = "lock";
const WITNESS_ENV: &str = "EPISTEMIC_WITNESS";

#[cfg(test)]
pub(crate) struct TestWitnessEnvGuard {
    original: Option<Option<String>>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestWitnessEnvGuard {
    pub(crate) fn set(value: impl Into<String>) -> Self {
        Self::replace(Some(value.into()))
    }

    pub(crate) fn unset() -> Self {
        Self::replace(None)
    }

    fn replace(value: Option<String>) -> Self {
        let lock = TEST_ENV_OVERRIDE_LOCK
            .lock()
            .expect("lock witness env override mutex");
        let mut override_value = TEST_ENV_OVERRIDE
            .lock()
            .expect("lock witness env override value");
        let original = override_value.replace(value);
        drop(override_value);
        Self {
            original,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for TestWitnessEnvGuard {
    fn drop(&mut self) {
        if let Ok(mut override_value) = TEST_ENV_OVERRIDE.lock() {
            *override_value = self.original.take();
        }
    }
}

/// Resolve the witness ledger path.
///
/// Resolution order:
/// 1. `EPISTEMIC_WITNESS` env var, if set
/// 2. `~/.cmdrvl/state/witness/witness.jsonl`
pub fn resolve_ledger_path() -> PathBuf {
    resolve_ledger_path_from_env(env_value)
}

fn resolve_ledger_path_for_append() -> io::Result<PathBuf> {
    ensure_ledger_migrated_from_env(env_value)?;
    let path = resolve_ledger_path();
    if non_empty_env(env_value, WITNESS_ENV).is_none() {
        prepare_canonical_tree_from_env(env_value)?;
    }
    Ok(path)
}

fn resolve_ledger_path_for_query() -> io::Result<PathBuf> {
    ensure_ledger_migrated_from_env(env_value)?;
    Ok(resolve_ledger_path())
}

fn resolve_ledger_path_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(path) = non_empty_env(get_env, WITNESS_ENV) {
        return PathBuf::from(path);
    }

    cmdrvl_root_from_env(get_env)
        .join("state")
        .join("witness")
        .join("witness.jsonl")
}

fn env_value(key: &str) -> Option<OsString> {
    if key == WITNESS_ENV
        && let Some(value) = test_witness_env_override()
    {
        return value.map(OsString::from);
    }

    env::var_os(key)
}

#[cfg(test)]
fn test_witness_env_override() -> Option<Option<String>> {
    TEST_ENV_OVERRIDE
        .lock()
        .expect("lock witness env override value")
        .clone()
}

#[cfg(not(test))]
fn test_witness_env_override() -> Option<Option<String>> {
    None
}

fn non_empty_env<F>(get_env: F, key: &str) -> Option<OsString>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let value = get_env(key)?;
    if value.is_empty() {
        return None;
    }
    if value.to_str().is_some_and(|value| value.trim().is_empty()) {
        return None;
    }
    Some(value)
}

fn cmdrvl_root_from_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        return PathBuf::from(home).join(".cmdrvl");
    }

    PathBuf::from(".cmdrvl")
}

fn ensure_ledger_migrated_from_env<F>(get_env: F) -> io::Result<()>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if non_empty_env(get_env, WITNESS_ENV).is_some() {
        return Ok(());
    }

    let canonical = resolve_ledger_path_from_env(get_env);
    let Some(legacy) = legacy_ledger_paths_from_env(get_env)
        .into_iter()
        .find(|path| path != &canonical && path.is_file())
    else {
        return Ok(());
    };

    let root = cmdrvl_root_from_env(get_env);
    prepare_canonical_tree_from_env(get_env)?;
    let notice_path = root.join("notices").join("deprecated-paths.jsonl");
    let migration_path = root.join("migrations").join("applied.jsonl");

    if canonical.exists() {
        append_record_once(
            &notice_path,
            deprecation_record(
                &legacy,
                &canonical,
                "legacy_path_present",
                "canonical_preferred",
            ),
        )?;
        return Ok(());
    }

    if let Some(parent) = canonical.parent() {
        fs::create_dir_all(parent)?;
        harden_directory(parent)?;
    }

    fs::copy(&legacy, &canonical)?;
    let permissions = fs::metadata(&legacy)?.permissions();
    fs::set_permissions(&canonical, permissions)?;

    append_record_once(
        &migration_path,
        migration_record(&legacy, &canonical, "copied_legacy_to_canonical"),
    )?;
    append_record_once(
        &notice_path,
        deprecation_record(
            &legacy,
            &canonical,
            "legacy_path_migrated",
            "canonical_created",
        ),
    )?;

    Ok(())
}

fn prepare_canonical_tree_from_env<F>(get_env: F) -> io::Result<()>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    if non_empty_env(get_env, WITNESS_ENV).is_some() {
        return Ok(());
    }

    let root = cmdrvl_root_from_env(get_env);
    for dir in [
        root.clone(),
        root.join("state"),
        root.join("state").join("witness"),
    ] {
        fs::create_dir_all(&dir)?;
        harden_directory(&dir)?;
    }

    Ok(())
}

fn legacy_ledger_paths_from_env<F>(get_env: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString> + Copy,
{
    let mut paths = Vec::new();

    if let Some(home) =
        non_empty_env(get_env, "HOME").or_else(|| non_empty_env(get_env, "USERPROFILE"))
    {
        paths.push(PathBuf::from(home).join(".epistemic").join("witness.jsonl"));
    }

    paths.push(PathBuf::from(".epistemic").join("witness.jsonl"));
    paths
}

fn migration_record(source: &Path, destination: &Path, action: &str) -> Value {
    serde_json::json!({
        "version": "cmdrvl.migration.v1",
        "tool": TOOL_NAME,
        "path_class": "witness_ledger",
        "source_path": source.display().to_string(),
        "destination_path": destination.display().to_string(),
        "action": action,
        "outcome": "ok",
        "secret_values_recorded": false
    })
}

fn deprecation_record(source: &Path, destination: &Path, action: &str, outcome: &str) -> Value {
    serde_json::json!({
        "version": "cmdrvl.deprecated_path_notice.v1",
        "tool": TOOL_NAME,
        "path_class": "witness_ledger",
        "source_path": source.display().to_string(),
        "destination_path": destination.display().to_string(),
        "action": action,
        "outcome": outcome,
        "secret_values_recorded": false
    })
}

fn append_record_once(path: &Path, record: Value) -> io::Result<()> {
    if record_already_exists(path, &record)? {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        harden_directory(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{record}")?;
    Ok(())
}

fn record_already_exists(path: &Path, record: &Value) -> io::Result<bool> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Ok(false);
    };

    Ok(contents.lines().any(|line| {
        let Ok(existing) = serde_json::from_str::<Value>(line) else {
            return false;
        };

        existing.get("tool") == record.get("tool")
            && existing.get("path_class") == record.get("path_class")
            && existing.get("source_path") == record.get("source_path")
            && existing.get("destination_path") == record.get("destination_path")
            && existing.get("action") == record.get("action")
    }))
}

#[cfg(unix)]
fn harden_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn harden_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// A parsed witness record from the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessRecord {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub ts: Option<String>,
    #[serde(default)]
    pub output_hash: Option<String>,
    #[serde(default)]
    pub inputs: Option<Vec<Value>>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub binary_hash: Option<String>,
    /// Capture any additional fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Read all witness records from the ledger file.
///
/// Returns an empty vec if the file does not exist.
/// Returns an error only for I/O failures other than not-found.
pub fn read_ledger(path: &std::path::Path) -> io::Result<Vec<WitnessRecord>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let reader = io::BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip unparseable lines silently (ledger may contain records from other tools).
        if let Ok(record) = serde_json::from_str::<WitnessRecord>(trimmed) {
            records.push(record);
        }
    }

    Ok(records)
}

/// Apply filters to a set of witness records.
pub fn apply_filters<'a>(
    records: &'a [WitnessRecord],
    filters: &WitnessFilters,
) -> Vec<&'a WitnessRecord> {
    records
        .iter()
        .filter(|r| matches_filters(r, filters))
        .collect()
}

fn matches_filters(record: &WitnessRecord, filters: &WitnessFilters) -> bool {
    // Tool filter.
    if let Some(tool) = &filters.tool
        && record.tool.as_deref() != Some(tool.as_str())
    {
        return false;
    }

    // Outcome filter.
    if let Some(outcome) = &filters.outcome
        && record.outcome.as_deref() != Some(outcome.as_str())
    {
        return false;
    }

    // Input hash substring filter.
    if let Some(hash_sub) = &filters.input_hash {
        let has_match = record
            .inputs
            .as_ref()
            .map(|inputs| {
                inputs.iter().any(|input| {
                    input
                        .get("hash")
                        .and_then(Value::as_str)
                        .is_some_and(|h| h.contains(hash_sub.as_str()))
                })
            })
            .unwrap_or(false);
        if !has_match {
            return false;
        }
    }

    // Since filter (RFC3339 instant comparison).
    if let Some(since) = &filters.since {
        let Some(since_ts) = parse_rfc3339_timestamp(since) else {
            return false;
        };
        let Some(record_ts) = record.ts.as_deref().and_then(parse_rfc3339_timestamp) else {
            return false;
        };
        if record_ts <= since_ts {
            return false;
        }
    }

    // Until filter (RFC3339 instant comparison).
    if let Some(until) = &filters.until {
        let Some(until_ts) = parse_rfc3339_timestamp(until) else {
            return false;
        };
        let Some(record_ts) = record.ts.as_deref().and_then(parse_rfc3339_timestamp) else {
            return false;
        };
        if record_ts >= until_ts {
            return false;
        }
    }

    true
}

fn parse_rfc3339_timestamp(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

// ---------------------------------------------------------------------------
// Witness append (called from orchestration after stdout output)
// ---------------------------------------------------------------------------

/// Append a witness record to the ledger after a lock run.
///
/// `outcome` is the outcome string (e.g. "LOCK_CREATED", "VERIFY_OK").
/// `exit_code` is the process exit code for this outcome.
/// `output_bytes` is the raw bytes written to stdout (lockfile JSON or refusal envelope).
/// `params` is the subcommand-specific parameters as a JSON value.
/// `inputs` is the inputs array as a JSON value.
///
/// This function computes `output_hash` as BLAKE3 of those bytes, builds the witness
/// record, and appends it as a single JSONL line.
///
/// Witness failures are non-fatal: errors are printed to stderr but do not
/// change the domain exit code.
pub fn append_witness_record(
    outcome: &str,
    exit_code: u8,
    output_bytes: &[u8],
    params: Value,
    inputs: Value,
) {
    let ledger_path = match resolve_ledger_path_for_append() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("lock: witness append warning: {e}");
            return;
        }
    };
    if let Err(e) = append_witness_record_to(
        outcome,
        exit_code,
        output_bytes,
        params,
        inputs,
        &ledger_path,
    ) {
        eprintln!("lock: witness append warning: {e}");
    }
}

fn append_witness_record_to(
    outcome: &str,
    exit_code: u8,
    output_bytes: &[u8],
    params: Value,
    inputs: Value,
    ledger_path: &std::path::Path,
) -> io::Result<()> {
    // Ensure parent directory exists.
    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Open and lock the ledger so each record is written as a single append step.
    let mut file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(ledger_path)?;
    file.lock_exclusive()?;

    // Compute output_hash (BLAKE3 of stdout bytes).
    let output_hash = format!("blake3:{}", blake3::hash(output_bytes).to_hex());

    let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);

    // Build the record without id first, compute id as BLAKE3 of the record.
    let mut record = serde_json::json!({
        "id": "",
        "tool": "lock",
        "version": env!("CARGO_PKG_VERSION"),
        "binary_hash": null,
        "inputs": inputs,
        "params": params,
        "outcome": outcome,
        "exit_code": exit_code,
        "output_hash": output_hash,
        "ts": ts,
    });

    // Compute record id as BLAKE3 of the record with id="".
    let pre_id_json = serde_json::to_string(&record).map_err(io::Error::other)?;
    let record_id = format!("blake3:{}", blake3::hash(pre_id_json.as_bytes()).to_hex());
    record["id"] = Value::String(record_id);

    // Serialize final record (compact, single line).
    let line = serde_json::to_string(&record).map_err(io::Error::other)?;

    // Append to ledger.
    writeln!(file, "{line}")?;
    file.unlock()?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch functions (called from cli::dispatch_witness)
// ---------------------------------------------------------------------------

/// Execute `lock witness query` — list matching records.
///
/// Exit codes:
/// - `0`: matches found
/// - `1`: no matches
/// - `2`: error
pub fn dispatch_query(filters: &WitnessFilters, limit: usize, json_output: bool) -> u8 {
    let path = match resolve_ledger_path_for_query() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };
    let records = match read_ledger(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };

    let mut matched: Vec<&WitnessRecord> = apply_filters(&records, filters);

    // Most recent first (by parsed instant, falling back to string comparison).
    matched.sort_by(|a, b| {
        let a_parsed = a.ts.as_deref().and_then(parse_rfc3339_timestamp);
        let b_parsed = b.ts.as_deref().and_then(parse_rfc3339_timestamp);
        match (b_parsed, a_parsed) {
            (Some(b_ts), Some(a_ts)) => b_ts.cmp(&a_ts),
            _ => b.ts.cmp(&a.ts),
        }
    });
    matched.truncate(limit);

    if matched.is_empty() {
        if json_output {
            println!("[]");
        } else {
            eprintln!("no matching witness records");
        }
        return 1;
    }

    if json_output {
        let values: Vec<Value> = matched
            .iter()
            .filter_map(|r| serde_json::to_value(r).ok())
            .collect();
        if let Ok(out) = serde_json::to_string_pretty(&values) {
            println!("{out}");
        }
    } else {
        for r in &matched {
            print_record_human(r);
        }
    }

    0
}

/// Execute `lock witness last` — show the most recent record.
///
/// Exit codes:
/// - `0`: record found
/// - `1`: no records
/// - `2`: error
pub fn dispatch_last(json_output: bool) -> u8 {
    let path = match resolve_ledger_path_for_query() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };
    let records = match read_ledger(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };

    // Find the most recent record by timestamp (or last in file order).
    let last = records.iter().max_by(|a, b| {
        let a_parsed = a.ts.as_deref().and_then(parse_rfc3339_timestamp);
        let b_parsed = b.ts.as_deref().and_then(parse_rfc3339_timestamp);
        match (a_parsed, b_parsed) {
            (Some(a_ts), Some(b_ts)) => a_ts.cmp(&b_ts),
            _ => a.ts.cmp(&b.ts),
        }
    });

    match last {
        Some(record) => {
            if json_output {
                if let Ok(out) = serde_json::to_string_pretty(record) {
                    println!("{out}");
                }
            } else {
                print_record_human(record);
            }
            0
        }
        None => {
            if json_output {
                println!("null");
            } else {
                eprintln!("no witness records");
            }
            1
        }
    }
}

/// Execute `lock witness count` — count matching records.
///
/// Exit codes:
/// - `0`: always (count is valid even if zero)
pub fn dispatch_count(filters: &WitnessFilters, json_output: bool) -> u8 {
    let path = match resolve_ledger_path_for_query() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };
    let records = match read_ledger(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lock: witness ledger error: {e}");
            return 2;
        }
    };

    let count = apply_filters(&records, filters).len();

    if json_output {
        println!("{}", serde_json::json!({ "count": count }));
    } else {
        println!("{count}");
    }

    0
}

/// Print a witness record in human-readable format to stdout.
fn print_record_human(record: &WitnessRecord) {
    let tool = record.tool.as_deref().unwrap_or("?");
    let version = record.version.as_deref().unwrap_or("?");
    let outcome = record.outcome.as_deref().unwrap_or("?");
    let ts = record.ts.as_deref().unwrap_or("?");
    let exit = record
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());

    println!("{ts}  {tool} {version}  {outcome} (exit {exit})");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    fn make_record(tool: &str, outcome: &str, ts: &str) -> WitnessRecord {
        WitnessRecord {
            id: None,
            tool: Some(tool.to_string()),
            version: Some("0.1.0".to_string()),
            outcome: Some(outcome.to_string()),
            exit_code: Some(0),
            ts: Some(ts.to_string()),
            output_hash: None,
            inputs: None,
            params: None,
            binary_hash: None,
            extra: serde_json::Map::new(),
        }
    }

    fn canonical_witness_path(home: &Path) -> PathBuf {
        home.join(".cmdrvl")
            .join("state")
            .join("witness")
            .join("witness.jsonl")
    }

    #[test]
    fn resolve_ledger_path_uses_env_var() {
        let _guard = TestWitnessEnvGuard::set("/tmp/test-witness.jsonl");
        let path = resolve_ledger_path();
        assert_eq!(path, PathBuf::from("/tmp/test-witness.jsonl"));
    }

    #[test]
    fn resolve_ledger_path_falls_back_to_home() {
        let _guard = TestWitnessEnvGuard::unset();
        let path = resolve_ledger_path();
        assert!(path.ends_with(".cmdrvl/state/witness/witness.jsonl"));
    }

    #[test]
    fn resolve_ledger_path_ignores_empty_env_var() {
        let _guard = TestWitnessEnvGuard::set("");
        let path = resolve_ledger_path();
        assert!(path.ends_with(".cmdrvl/state/witness/witness.jsonl"));
    }

    #[test]
    fn resolve_ledger_path_from_env_uses_cmdrvl_home() {
        let path = resolve_ledger_path_from_env(|key| match key {
            "HOME" => Some(OsString::from("/tmp/home")),
            "EPISTEMIC_WITNESS" | "USERPROFILE" => None,
            _ => None,
        });

        assert_eq!(
            path,
            PathBuf::from("/tmp/home/.cmdrvl/state/witness/witness.jsonl")
        );
    }

    #[test]
    fn explicit_witness_override_skips_migration() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let legacy = home.join(".epistemic").join("witness.jsonl");
        std::fs::create_dir_all(legacy.parent().expect("legacy parent")).unwrap();
        std::fs::write(&legacy, "{\"version\":\"witness.v0\"}\n").unwrap();
        let override_path = home.join("override.jsonl");

        ensure_ledger_migrated_from_env(|key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            "EPISTEMIC_WITNESS" => Some(override_path.as_os_str().to_owned()),
            "USERPROFILE" => None,
            _ => None,
        })
        .unwrap();

        assert!(!canonical_witness_path(home).exists());
        assert!(!home.join(".cmdrvl/migrations/applied.jsonl").exists());
    }

    #[test]
    fn migrates_legacy_home_witness_to_cmdrvl_root() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let legacy = home.join(".epistemic").join("witness.jsonl");
        std::fs::create_dir_all(legacy.parent().expect("legacy parent")).unwrap();
        std::fs::write(&legacy, "{\"version\":\"witness.v0\"}\n").unwrap();

        ensure_ledger_migrated_from_env(|key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            "EPISTEMIC_WITNESS" | "USERPROFILE" => None,
            _ => None,
        })
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(canonical_witness_path(home)).unwrap(),
            "{\"version\":\"witness.v0\"}\n"
        );

        let migration =
            std::fs::read_to_string(home.join(".cmdrvl/migrations/applied.jsonl")).unwrap();
        assert!(migration.contains("cmdrvl.migration.v1"));
        assert!(migration.contains("copied_legacy_to_canonical"));

        let notice =
            std::fs::read_to_string(home.join(".cmdrvl/notices/deprecated-paths.jsonl")).unwrap();
        assert!(notice.contains("cmdrvl.deprecated_path_notice.v1"));
        assert!(notice.contains("legacy_path_migrated"));
    }

    #[test]
    fn migration_prefers_existing_canonical_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let legacy = home.join(".epistemic").join("witness.jsonl");
        let canonical = canonical_witness_path(home);
        std::fs::create_dir_all(legacy.parent().expect("legacy parent")).unwrap();
        std::fs::create_dir_all(canonical.parent().expect("canonical parent")).unwrap();
        std::fs::write(&legacy, "legacy\n").unwrap();
        std::fs::write(&canonical, "canonical\n").unwrap();

        ensure_ledger_migrated_from_env(|key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            "EPISTEMIC_WITNESS" | "USERPROFILE" => None,
            _ => None,
        })
        .unwrap();

        assert_eq!(std::fs::read_to_string(&canonical).unwrap(), "canonical\n");
        let notice =
            std::fs::read_to_string(home.join(".cmdrvl/notices/deprecated-paths.jsonl")).unwrap();
        assert!(notice.contains("legacy_path_present"));
        assert!(notice.contains("canonical_preferred"));
    }

    #[test]
    fn read_ledger_returns_empty_for_missing_file() {
        let records = read_ledger(std::path::Path::new("/nonexistent/witness.jsonl")).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn read_ledger_parses_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("witness.jsonl");
        std::fs::write(
            &path,
            r#"{"tool":"lock","version":"0.1.0","outcome":"LOCK_CREATED","ts":"2026-01-01T00:00:00Z"}
{"tool":"hash","version":"0.2.0","outcome":"REFUSAL","ts":"2026-01-02T00:00:00Z"}
"#,
        )
        .unwrap();

        let records = read_ledger(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tool.as_deref(), Some("lock"));
        assert_eq!(records[1].tool.as_deref(), Some("hash"));
    }

    #[test]
    fn read_ledger_skips_blank_and_invalid_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("witness.jsonl");
        std::fs::write(
            &path,
            r#"{"tool":"lock","outcome":"LOCK_CREATED","ts":"2026-01-01T00:00:00Z"}

not json
{"tool":"hash","outcome":"REFUSAL","ts":"2026-01-02T00:00:00Z"}
"#,
        )
        .unwrap();

        let records = read_ledger(&path).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn filter_by_tool() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("hash", "REFUSAL", "2026-01-02T00:00:00Z"),
            make_record("lock", "LOCK_PARTIAL", "2026-01-03T00:00:00Z"),
        ];

        let filters = WitnessFilters {
            tool: Some("lock".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
        assert!(matched.iter().all(|r| r.tool.as_deref() == Some("lock")));
    }

    #[test]
    fn filter_by_outcome() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("lock", "REFUSAL", "2026-01-02T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-03T00:00:00Z"),
        ];

        let filters = WitnessFilters {
            outcome: Some("LOCK_CREATED".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn filter_by_since() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-05T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-10T00:00:00Z"),
        ];

        let filters = WitnessFilters {
            since: Some("2026-01-03T00:00:00Z".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn filter_by_since_uses_instant_comparison_for_mixed_offsets() {
        let records = vec![
            // 2025-12-31T22:00:00Z
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00+02:00"),
            // 2025-12-31T23:30:00Z
            make_record("lock", "LOCK_CREATED", "2025-12-31T23:30:00Z"),
        ];

        let filters = WitnessFilters {
            since: Some("2025-12-31T22:45:00Z".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].ts.as_deref(), Some("2025-12-31T23:30:00Z"));
    }

    #[test]
    fn filter_by_until() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-05T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-10T00:00:00Z"),
        ];

        let filters = WitnessFilters {
            until: Some("2026-01-06T00:00:00Z".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn filter_with_invalid_since_timestamp_matches_none() {
        let records = vec![make_record("lock", "LOCK_CREATED", "2026-01-05T00:00:00Z")];

        let filters = WitnessFilters {
            since: Some("not-a-timestamp".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert!(matched.is_empty());
    }

    #[test]
    fn filter_with_invalid_record_timestamp_matches_none_when_time_filter_used() {
        let records = vec![make_record("lock", "LOCK_CREATED", "not-a-timestamp")];

        let filters = WitnessFilters {
            since: Some("2026-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert!(matched.is_empty());
    }

    #[test]
    fn filter_by_since_uses_instant_not_lexicographic_order() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00+02:00"),
            make_record("lock", "LOCK_CREATED", "2025-12-31T22:30:00Z"),
        ];

        let filters = WitnessFilters {
            since: Some("2025-12-31T22:15:00Z".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].ts.as_deref(), Some("2025-12-31T22:30:00Z"));
    }

    #[test]
    fn filter_with_invalid_timestamp_values_yields_no_matches() {
        let records = vec![make_record("lock", "LOCK_CREATED", "not-a-timestamp")];

        let invalid_since = WitnessFilters {
            since: Some("not-a-filter-timestamp".to_string()),
            ..Default::default()
        };
        assert!(apply_filters(&records, &invalid_since).is_empty());

        let invalid_record_ts = WitnessFilters {
            since: Some("2026-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        assert!(apply_filters(&records, &invalid_record_ts).is_empty());
    }

    #[test]
    fn filter_combined() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("hash", "LOCK_CREATED", "2026-01-02T00:00:00Z"),
            make_record("lock", "REFUSAL", "2026-01-03T00:00:00Z"),
            make_record("lock", "LOCK_CREATED", "2026-01-04T00:00:00Z"),
        ];

        let filters = WitnessFilters {
            tool: Some("lock".to_string()),
            outcome: Some("LOCK_CREATED".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn empty_filters_match_all() {
        let records = vec![
            make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z"),
            make_record("hash", "REFUSAL", "2026-01-02T00:00:00Z"),
        ];

        let filters = WitnessFilters::default();
        let matched = apply_filters(&records, &filters);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn no_matches_returns_empty() {
        let records = vec![make_record("lock", "LOCK_CREATED", "2026-01-01T00:00:00Z")];

        let filters = WitnessFilters {
            tool: Some("nonexistent".to_string()),
            ..Default::default()
        };

        let matched = apply_filters(&records, &filters);
        assert!(matched.is_empty());
    }

    fn default_params() -> Value {
        serde_json::json!({
            "dataset_id": null,
            "as_of": null,
            "note": null,
        })
    }

    fn default_inputs() -> Value {
        serde_json::json!([
            { "path": "stdin", "hash": null, "bytes": null }
        ])
    }

    #[test]
    fn append_creates_ledger_and_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("witness.jsonl");

        let params = serde_json::json!({
            "dataset_id": "test-ds",
            "as_of": null,
            "note": null,
        });

        append_witness_record_to(
            "LOCK_CREATED",
            0,
            b"{}",
            params,
            default_inputs(),
            &ledger_path,
        )
        .unwrap();

        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["tool"], "lock");
        assert_eq!(record["outcome"], "LOCK_CREATED");
        assert_eq!(record["exit_code"], 0);
        assert!(record["id"].as_str().unwrap().starts_with("blake3:"));
        assert!(
            record["output_hash"]
                .as_str()
                .unwrap()
                .starts_with("blake3:")
        );
        assert_eq!(record["inputs"][0]["path"], "stdin");
        assert_eq!(record["params"]["dataset_id"], "test-ds");
    }

    #[test]
    fn append_is_additive() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("witness.jsonl");

        append_witness_record_to(
            "LOCK_CREATED",
            0,
            b"first",
            default_params(),
            default_inputs(),
            &ledger_path,
        )
        .unwrap();
        append_witness_record_to(
            "LOCK_PARTIAL",
            1,
            b"second",
            default_params(),
            default_inputs(),
            &ledger_path,
        )
        .unwrap();

        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();

        assert_eq!(first["outcome"], "LOCK_CREATED");
        assert_eq!(second["outcome"], "LOCK_PARTIAL");
        assert_eq!(second["exit_code"], 1);
        assert_ne!(second["id"], first["id"]);
    }

    #[test]
    fn append_refusal_records_exit_code_2() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("witness.jsonl");

        let params = serde_json::json!({
            "dataset_id": null,
            "as_of": null,
            "note": "refused",
        });
        let inputs = serde_json::json!([
            { "path": "input.jsonl", "hash": null, "bytes": null }
        ]);

        append_witness_record_to(
            "REFUSAL",
            2,
            b"refusal envelope",
            params,
            inputs,
            &ledger_path,
        )
        .unwrap();

        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(record["outcome"], "REFUSAL");
        assert_eq!(record["exit_code"], 2);
        assert_eq!(record["inputs"][0]["path"], "input.jsonl");
        assert_eq!(record["params"]["note"], "refused");
    }

    #[test]
    fn append_output_hash_is_blake3_of_output_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("witness.jsonl");

        let output = b"test output bytes";
        let expected_hash = format!("blake3:{}", blake3::hash(output).to_hex());

        append_witness_record_to(
            "LOCK_CREATED",
            0,
            output,
            default_params(),
            default_inputs(),
            &ledger_path,
        )
        .unwrap();

        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(record["output_hash"], expected_hash);
    }

    #[test]
    fn append_to_invalid_path_returns_error() {
        let result = append_witness_record_to(
            "LOCK_CREATED",
            0,
            b"{}",
            default_params(),
            default_inputs(),
            std::path::Path::new("/dev/null/impossible/witness.jsonl"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn append_verify_params_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_path = dir.path().join("witness.jsonl");

        let params = serde_json::json!({
            "subcommand": "verify",
            "root": "/data/dec",
            "strict": true,
        });
        let inputs = serde_json::json!([
            { "path": "dec.lock.json", "hash": null, "bytes": null }
        ]);

        append_witness_record_to(
            "VERIFY_OK",
            0,
            b"verify output",
            params,
            inputs,
            &ledger_path,
        )
        .unwrap();

        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(record["outcome"], "VERIFY_OK");
        assert_eq!(record["exit_code"], 0);
        assert_eq!(record["params"]["subcommand"], "verify");
        assert_eq!(record["params"]["root"], "/data/dec");
        assert_eq!(record["params"]["strict"], true);
        assert_eq!(record["inputs"][0]["path"], "dec.lock.json");
    }
}
