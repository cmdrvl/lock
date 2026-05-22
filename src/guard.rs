#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::refusal::RefusalEnvelope;

const REQUIRED_VEIL_MATCHERS: &[&str] = &["Read", "Grep", "Bash"];
const DCG_COMMAND_STEMS: &[&str] = &[
    "dcg",
    "destructive_command_guard",
    "destructive-command-guard",
];

#[derive(Debug, Clone, Eq, PartialEq)]
struct GuardInspection {
    veil: Vec<MatcherInspection>,
    dcg: HookStatus,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct MatcherInspection {
    matcher: &'static str,
    status: HookStatus,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum HookStatus {
    Installed {
        command: String,
        executable: PathBuf,
    },
    Missing,
    Invalid {
        command: String,
        reason: String,
    },
}

pub(crate) fn enforce_guard_preflight() -> Result<(), Box<RefusalEnvelope>> {
    let settings_path = default_settings_path().map_err(|error| {
        Box::new(crate::refusal::guard_preflight(
            None,
            vec![format!(
                "could not resolve Claude settings path from HOME or USERPROFILE: {error}"
            )],
        ))
    })?;

    let inspection = inspect_guard_preflight(&settings_path).map_err(|error| {
        Box::new(crate::refusal::guard_preflight(
            Some(&settings_path),
            vec![error],
        ))
    })?;

    if inspection.is_healthy() {
        Ok(())
    } else {
        Err(Box::new(crate::refusal::guard_preflight(
            Some(&settings_path),
            inspection.findings(),
        )))
    }
}

fn default_settings_path() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.to_string_lossy().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME and USERPROFILE are unset"))?;

    Ok(PathBuf::from(home).join(".claude/settings.json"))
}

fn inspect_guard_preflight(path: &Path) -> Result<GuardInspection, String> {
    let settings = load_settings(path)?;
    let entries = pre_tool_use_entries(&settings);

    Ok(GuardInspection {
        veil: inspect_veil_entries(entries),
        dcg: inspect_dcg_entries(entries),
    })
}

fn load_settings(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("Claude settings are not valid JSON: {error}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!(
            "could not read Claude settings at {}: {error}",
            path.display()
        )),
    }
}

fn pre_tool_use_entries(settings: &Value) -> &[Value] {
    settings
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn inspect_veil_entries(entries: &[Value]) -> Vec<MatcherInspection> {
    REQUIRED_VEIL_MATCHERS
        .iter()
        .map(|matcher| MatcherInspection {
            matcher,
            status: entries
                .iter()
                .filter(|entry| matcher_matches(entry, matcher))
                .find_map(veil_hook_command)
                .map_or(HookStatus::Missing, resolve_hook_status),
        })
        .collect()
}

fn inspect_dcg_entries(entries: &[Value]) -> HookStatus {
    entries
        .iter()
        .filter(|entry| matcher_matches(entry, "Bash"))
        .find_map(dcg_hook_command)
        .map_or(HookStatus::Missing, resolve_hook_status)
}

fn matcher_matches(entry: &Value, matcher: &str) -> bool {
    entry
        .get("matcher")
        .and_then(Value::as_str)
        .is_some_and(|value| value == matcher)
}

fn veil_hook_command(entry: &Value) -> Option<String> {
    entry
        .get("hooks")
        .and_then(Value::as_array)?
        .iter()
        .find_map(veil_hook_command_from_hook)
}

fn dcg_hook_command(entry: &Value) -> Option<String> {
    entry
        .get("hooks")
        .and_then(Value::as_array)?
        .iter()
        .find_map(dcg_hook_command_from_hook)
}

fn veil_hook_command_from_hook(hook: &Value) -> Option<String> {
    let command = command_hook_command(hook)?;
    is_veil_command(&command).then_some(command)
}

fn dcg_hook_command_from_hook(hook: &Value) -> Option<String> {
    let command = command_hook_command(hook)?;
    is_dcg_command(&command).then_some(command)
}

fn command_hook_command(hook: &Value) -> Option<String> {
    let object = hook.as_object()?;
    let is_command_hook = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "command");
    if !is_command_hook {
        return None;
    }

    object
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_veil_command(command: &str) -> bool {
    command_program(command)
        .and_then(|program| {
            Path::new(&program)
                .file_stem()
                .and_then(OsStr::to_str)
                .map(str::to_owned)
        })
        .is_some_and(|stem| stem == "veil")
}

fn is_dcg_command(command: &str) -> bool {
    command_program(command)
        .and_then(|program| {
            Path::new(&program)
                .file_stem()
                .and_then(OsStr::to_str)
                .map(str::to_owned)
        })
        .map(|stem| stem.to_ascii_lowercase())
        .is_some_and(|stem| DCG_COMMAND_STEMS.iter().any(|candidate| candidate == &stem))
}

fn resolve_hook_status(command: String) -> HookStatus {
    match resolve_hook_command_executable(&command) {
        Ok(executable) => HookStatus::Installed {
            command,
            executable,
        },
        Err(reason) => HookStatus::Invalid { command, reason },
    }
}

fn resolve_hook_command_executable(command: &str) -> Result<PathBuf, String> {
    let program = command_program(command)
        .ok_or_else(|| format!("could not parse guard hook command `{command}`"))?;
    let program = expand_program_path(&program)?;
    let executable = if references_path(&program) {
        PathBuf::from(&program)
    } else {
        find_in_path(&program).ok_or_else(|| format!("`{program}` was not found in PATH"))?
    };
    let metadata =
        fs::metadata(&executable).map_err(|error| format!("{}: {error}", executable.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a file", executable.display()));
    }
    if !is_executable(&metadata) {
        return Err(format!("{} is not executable", executable.display()));
    }

    fs::canonicalize(&executable).or(Ok(executable))
}

fn command_program(command: &str) -> Option<String> {
    shlex::split(command).and_then(|parts| parts.into_iter().next())
}

fn expand_program_path(program: &str) -> Result<String, String> {
    if let Some(suffix) = program.strip_prefix("$HOME/") {
        let home = std::env::var("HOME")
            .map_err(|_| format!("HOME is unset, so `{program}` cannot be resolved"))?;
        return Ok(PathBuf::from(home).join(suffix).display().to_string());
    }

    if let Some(suffix) = program.strip_prefix("${HOME}/") {
        let home = std::env::var("HOME")
            .map_err(|_| format!("HOME is unset, so `{program}` cannot be resolved"))?;
        return Ok(PathBuf::from(home).join(suffix).display().to_string());
    }

    if let Some(suffix) = program.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .map_err(|_| format!("HOME is unset, so `{program}` cannot be resolved"))?;
        return Ok(PathBuf::from(home).join(suffix).display().to_string());
    }

    Ok(program.to_owned())
}

fn references_path(program: &str) -> bool {
    let path = Path::new(program);
    path.is_absolute() || path.components().count() > 1
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(program))
            .find(|path| path.is_file())
    })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

impl GuardInspection {
    fn is_healthy(&self) -> bool {
        self.veil.iter().all(MatcherInspection::is_healthy) && self.dcg.is_healthy()
    }

    fn findings(&self) -> Vec<String> {
        let mut findings = Vec::new();

        for matcher in &self.veil {
            match &matcher.status {
                HookStatus::Installed { .. } => {}
                HookStatus::Missing => {
                    findings.push(format!("veil {} hook is missing", matcher.matcher));
                }
                HookStatus::Invalid { command, reason } => findings.push(format!(
                    "veil {} hook command `{command}` is invalid: {reason}",
                    matcher.matcher
                )),
            }
        }

        match &self.dcg {
            HookStatus::Installed { .. } => {}
            HookStatus::Missing => findings.push("dcg Bash hook is missing".to_owned()),
            HookStatus::Invalid { command, reason } => findings.push(format!(
                "dcg Bash hook command `{command}` is invalid: {reason}"
            )),
        }

        findings
    }
}

impl MatcherInspection {
    fn is_healthy(&self) -> bool {
        self.status.is_healthy()
    }
}

impl HookStatus {
    fn is_healthy(&self) -> bool {
        matches!(self, Self::Installed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("lock-guard-{label}-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).expect("temporary root should be creatable");
        root
    }

    fn create_executable(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("executable parent should be creatable");
        }
        fs::write(path, "#!/bin/sh\nexit 0\n").expect("test executable should be writable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(path)
                .expect("test executable should exist")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)
                .expect("test executable permissions should be writable");
        }
    }

    #[test]
    fn missing_settings_fail_closed() {
        let root = temp_root("missing");
        let inspection = inspect_guard_preflight(&root.join(".claude/settings.json"))
            .expect("missing settings should be inspectable");

        assert!(!inspection.is_healthy());
        assert!(inspection.findings().iter().any(|finding| {
            finding.contains("veil Read hook is missing")
                || finding.contains("dcg Bash hook is missing")
        }));
    }

    #[test]
    fn healthy_settings_pass_with_split_bash_entries() {
        let root = temp_root("healthy");
        let settings_path = root.join(".claude/settings.json");
        let veil = root.join("bin/veil");
        let dcg = root.join("bin/dcg");
        create_executable(&veil);
        create_executable(&dcg);
        fs::create_dir_all(settings_path.parent().expect("settings should have parent"))
            .expect("settings parent should be creatable");
        fs::write(
            &settings_path,
            json!({
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Read", "hooks": [{ "type": "command", "command": veil.display().to_string() }] },
                        { "matcher": "Grep", "hooks": [{ "type": "command", "command": veil.display().to_string() }] },
                        { "matcher": "Bash", "hooks": [{ "type": "command", "command": veil.display().to_string() }] },
                        { "matcher": "Bash", "hooks": [{ "type": "command", "command": dcg.display().to_string() }] }
                    ]
                }
            })
            .to_string(),
        )
        .expect("settings should be writable");

        assert!(
            inspect_guard_preflight(&settings_path)
                .expect("settings should inspect")
                .is_healthy()
        );
    }
}
