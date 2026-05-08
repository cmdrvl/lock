use serde_json::{Value, json};

use crate::{cli::DoctorAction, witness::resolve_ledger_path};

const HEALTH_SCHEMA_VERSION: &str = "lock.doctor.health.v1";
const CAPABILITIES_SCHEMA_VERSION: &str = "lock.doctor.capabilities.v1";
const TRIAGE_SCHEMA_VERSION: &str = "lock.doctor.triage.v1";
const READ_ONLY_DOCTOR_CONTRACT: &str = "cmdrvl.read_only_doctor.v1";

const OPERATOR_JSON: &str = include_str!("../operator.json");
const LOCK_SCHEMA: &str = include_str!("../schemas/lock-v0.schema.json");
const VERIFY_SCHEMA: &str = include_str!("../schemas/lock-verify-v0.schema.json");

pub fn dispatch(robot_triage: bool, json_mode: bool, action: Option<&DoctorAction>) -> u8 {
    if robot_triage {
        let report = triage_report();
        print_json(&report);
        return exit_for_report(report.get("health").unwrap_or(&Value::Null));
    }

    match action {
        Some(DoctorAction::Health { json }) => render_health(*json || json_mode),
        Some(DoctorAction::Capabilities { json }) => render_capabilities(*json || json_mode),
        Some(DoctorAction::RobotDocs) => {
            print_robot_docs();
            0
        }
        None => render_health(json_mode),
    }
}

fn render_health(json_mode: bool) -> u8 {
    let report = health_report();
    if json_mode {
        print_json(&report);
    } else {
        print_health_human(&report);
    }
    exit_for_report(&report)
}

fn render_capabilities(json_mode: bool) -> u8 {
    let report = capabilities_report();
    if json_mode {
        print_json(&report);
    } else {
        print_capabilities_human(&report);
    }
    0
}

fn health_report() -> Value {
    let checks = vec![
        operator_manifest_check(),
        lock_schema_check(),
        verify_schema_check(),
        witness_path_check(),
        artifact_stdout_contract_check(),
    ];
    let summary = summary_from_checks(&checks);
    let ok = summary.get("error").and_then(Value::as_u64).unwrap_or(0) == 0;

    json!({
        "schema_version": HEALTH_SCHEMA_VERSION,
        "tool": "lock",
        "version": env!("CARGO_PKG_VERSION"),
        "contract": READ_ONLY_DOCTOR_CONTRACT,
        "read_only": true,
        "ok": ok,
        "summary": summary,
        "checks": checks,
        "recommended_actions": recommended_actions(ok),
        "fixers": [],
    })
}

fn capabilities_report() -> Value {
    json!({
        "schema_version": CAPABILITIES_SCHEMA_VERSION,
        "tool": "lock",
        "version": env!("CARGO_PKG_VERSION"),
        "contract": READ_ONLY_DOCTOR_CONTRACT,
        "read_only": true,
        "network": {
            "required": false,
            "used": false
        },
        "side_effects": {
            "reads_stdin": false,
            "reads_input_jsonl": false,
            "reads_lockfiles": false,
            "verifies_member_content": false,
            "creates_lockfiles": false,
            "writes_output_files": false,
            "writes_witness_ledger": false,
            "creates_witness_directory": false,
            "writes_doctor_artifacts": false,
            "rewrites_operator_manifest": false,
            "rewrites_schema": false,
            "uses_network": false
        },
        "commands": [
            {
                "command": "lock doctor health",
                "json": "lock doctor health --json",
                "description": "Run read-only static health checks."
            },
            {
                "command": "lock doctor capabilities --json",
                "description": "Describe the doctor command surface and mutation policy."
            },
            {
                "command": "lock doctor robot-docs",
                "description": "Print agent-oriented usage notes."
            },
            {
                "command": "lock doctor --robot-triage",
                "description": "Emit health and capabilities in one robot-readable report."
            }
        ],
        "detectors": [
            {
                "name": "operator_manifest",
                "mode": "compiled_static_json",
                "mutates": false
            },
            {
                "name": "lock_schema",
                "mode": "compiled_static_json",
                "mutates": false
            },
            {
                "name": "verify_schema",
                "mode": "compiled_static_json",
                "mutates": false
            },
            {
                "name": "witness_path_resolution",
                "mode": "environment_resolution_only",
                "mutates": false
            },
            {
                "name": "artifact_stdout_contract",
                "mode": "static_contract",
                "mutates": false
            }
        ],
        "output_contract": {
            "lock_stdout": "lock.v0 artifact JSON or REFUSAL envelope",
            "verify_stdout": "human text or lock-verify.v0 JSON depending on --json",
            "doctor_stdout": "human text or JSON doctor reports",
            "doctor_stderr": "unused on successful doctor commands"
        },
        "fix_mode": {
            "status": "not_available",
            "command": null,
            "reason": "No lock fixer has detector, backup, inverse, and fixture coverage yet."
        },
        "fixers": []
    })
}

fn triage_report() -> Value {
    let health = health_report();
    let capabilities = capabilities_report();
    let ok = health.get("ok").cloned().unwrap_or(Value::Bool(false));
    let recommended_actions = health
        .get("recommended_actions")
        .cloned()
        .unwrap_or_else(|| json!([]));

    json!({
        "schema_version": TRIAGE_SCHEMA_VERSION,
        "tool": "lock",
        "version": env!("CARGO_PKG_VERSION"),
        "contract": READ_ONLY_DOCTOR_CONTRACT,
        "ok": ok,
        "health": health,
        "capabilities": capabilities,
        "recommended_actions": recommended_actions,
    })
}

fn operator_manifest_check() -> Value {
    let parsed = match serde_json::from_str::<Value>(OPERATOR_JSON) {
        Ok(value) => value,
        Err(error) => {
            return check(
                "operator_manifest",
                "error",
                format!("Compiled operator manifest is invalid JSON: {error}"),
                json!({ "source": "operator.json" }),
            );
        }
    };

    let expected_version = env!("CARGO_PKG_VERSION");
    let name_ok = parsed.get("name").and_then(Value::as_str) == Some("lock");
    let schema_ok = parsed.get("schema_version").and_then(Value::as_str) == Some("operator.v0");
    let version_ok = parsed.get("version").and_then(Value::as_str) == Some(expected_version);

    if name_ok && schema_ok && version_ok {
        check(
            "operator_manifest",
            "ok",
            "Compiled operator manifest matches the current binary.",
            json!({
                "schema_version": parsed.get("schema_version"),
                "version": parsed.get("version")
            }),
        )
    } else {
        check(
            "operator_manifest",
            "error",
            "Compiled operator manifest does not match the current binary contract.",
            json!({
                "expected_name": "lock",
                "actual_name": parsed.get("name"),
                "expected_schema_version": "operator.v0",
                "actual_schema_version": parsed.get("schema_version"),
                "expected_version": expected_version,
                "actual_version": parsed.get("version")
            }),
        )
    }
}

fn lock_schema_check() -> Value {
    schema_title_check(
        "lock_schema",
        LOCK_SCHEMA,
        "lock.v0",
        "schemas/lock-v0.schema.json",
    )
}

fn verify_schema_check() -> Value {
    schema_title_check(
        "verify_schema",
        VERIFY_SCHEMA,
        "lock-verify.v0",
        "schemas/lock-verify-v0.schema.json",
    )
}

fn schema_title_check(name: &str, source: &str, expected_title: &str, source_path: &str) -> Value {
    let parsed = match serde_json::from_str::<Value>(source) {
        Ok(value) => value,
        Err(error) => {
            return check(
                name,
                "error",
                format!("Compiled schema is invalid JSON: {error}"),
                json!({ "source": source_path }),
            );
        }
    };

    if parsed.get("title").and_then(Value::as_str) == Some(expected_title) {
        check(
            name,
            "ok",
            format!("Compiled schema advertises {expected_title}."),
            json!({
                "title": parsed.get("title"),
                "schema": parsed.get("$schema")
            }),
        )
    } else {
        check(
            name,
            "error",
            format!("Compiled schema title is not {expected_title}."),
            json!({
                "expected_title": expected_title,
                "actual_title": parsed.get("title")
            }),
        )
    }
}

fn witness_path_check() -> Value {
    let path = resolve_ledger_path();
    let parent = path.parent();

    check(
        "witness_path_resolution",
        "ok",
        "Resolved witness ledger path without creating directories or appending records.",
        json!({
            "path": path.display().to_string(),
            "parent": parent.map(|value| value.display().to_string()),
            "parent_exists": parent.is_some_and(|value| value.exists()),
            "write_attempted": false
        }),
    )
}

fn artifact_stdout_contract_check() -> Value {
    check(
        "artifact_stdout_contract",
        "ok",
        "Doctor commands are outside the lock and verify paths; lock artifact stdout remains structured JSON.",
        json!({
            "lock_stdout": "lock.v0 JSON or REFUSAL envelope",
            "verify_stdout": "human text or lock-verify.v0 JSON",
            "doctor_stdout": "doctor report",
            "witness_append": false
        }),
    )
}

fn check(name: &str, status: &str, message: impl Into<String>, details: Value) -> Value {
    json!({
        "name": name,
        "status": status,
        "message": message.into(),
        "details": details
    })
}

fn summary_from_checks(checks: &[Value]) -> Value {
    let mut ok = 0;
    let mut warn = 0;
    let mut error = 0;

    for check in checks {
        match check.get("status").and_then(Value::as_str) {
            Some("ok") => ok += 1,
            Some("warn") => warn += 1,
            Some("error") => error += 1,
            _ => error += 1,
        }
    }

    json!({
        "ok": ok,
        "warn": warn,
        "error": error,
        "total": checks.len()
    })
}

fn recommended_actions(ok: bool) -> Vec<&'static str> {
    if ok {
        vec![]
    } else {
        vec!["Inspect the failing compiled manifest or schema check before releasing lock."]
    }
}

fn print_health_human(report: &Value) {
    let summary = report.get("summary").unwrap_or(&Value::Null);
    let errors = summary.get("error").and_then(Value::as_u64).unwrap_or(0);
    let warnings = summary.get("warn").and_then(Value::as_u64).unwrap_or(0);
    let passed = summary.get("ok").and_then(Value::as_u64).unwrap_or(0);
    let state = if errors == 0 { "healthy" } else { "unhealthy" };

    println!("lock doctor {state}: {passed} checks passed, {warnings} warnings, {errors} errors");

    if let Some(checks) = report.get("checks").and_then(Value::as_array) {
        for check in checks {
            let status = check
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_ascii_uppercase();
            let name = check
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let message = check
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("<no message>");
            println!("[{status}] {name}: {message}");
        }
    }
}

fn print_capabilities_human(report: &Value) {
    println!("lock doctor capabilities");
    println!(
        "read_only: {}",
        report.get("read_only").unwrap_or(&Value::Bool(false))
    );
    println!(
        "contract: {}",
        report.get("contract").and_then(Value::as_str).unwrap_or("")
    );
    println!(
        "fix_mode: {}",
        report
            .get("fix_mode")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    println!("commands:");
    if let Some(commands) = report.get("commands").and_then(Value::as_array) {
        for command in commands {
            if let Some(name) = command.get("command").and_then(Value::as_str) {
                println!("  - {name}");
            }
        }
    }
}

fn print_robot_docs() {
    println!("# lock doctor robot docs");
    println!();
    println!("`lock doctor` is a read-only diagnostic surface for agents.");
    println!(
        "It does not read stdin or input files, create lockfiles, verify member content, append witness records, create witness directories, write doctor artifacts, rewrite metadata, or use the network."
    );
    println!();
    println!("Commands:");
    println!("- `lock doctor health` for human health output.");
    println!("- `lock doctor health --json` for machine-readable health.");
    println!("- `lock doctor capabilities --json` for command and side-effect policy.");
    println!("- `lock doctor --robot-triage` for a single JSON triage payload.");
    println!();
    println!("No fix mode is available. `lock doctor --fix` is intentionally unsupported.");
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    );
}

fn exit_for_report(report: &Value) -> u8 {
    if report.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        0
    } else {
        2
    }
}
