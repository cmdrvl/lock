use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[allow(dead_code)]
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[allow(dead_code)]
pub fn lock_command(label: &str) -> Command {
    let root = unique_root(label);
    let home = root.join("home");
    write_healthy_guard_hooks(&home);

    let mut command = Command::new(env!("CARGO_BIN_EXE_lock"));
    command.env("HOME", &home);
    command.env("USERPROFILE", root.join("profile"));
    command
}

pub fn write_guard_hooks(home: &Path, dcg_command: &str) {
    let settings_path = home.join(".claude/settings.json");
    let veil_path = home.join("bin/veil");
    create_executable(&veil_path);
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).expect("settings parent should be creatable");
    }
    fs::write(
        settings_path,
        serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Read", "hooks": [{ "type": "command", "command": veil_path.display().to_string() }] },
                    { "matcher": "Grep", "hooks": [{ "type": "command", "command": veil_path.display().to_string() }] },
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": veil_path.display().to_string() },
                            { "type": "command", "command": dcg_command }
                        ]
                    }
                ]
            }
        })
        .to_string(),
    )
    .expect("settings should be writable");
}

pub fn write_healthy_guard_hooks(home: &Path) {
    let dcg_path = home.join("bin/dcg");
    create_executable(&dcg_path);
    write_guard_hooks(home, &dcg_path.display().to_string());
}

#[allow(dead_code)]
fn unique_root(label: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "lock-integration-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary root should be creatable");
    root
}

fn create_executable(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("executable parent should be creatable");
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("test executable should be writable");
    file.write_all(b"#!/bin/sh\nexit 0\n")
        .expect("test executable should be writable");

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .expect("test executable should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .expect("test executable permissions should be writable");
    }
}
