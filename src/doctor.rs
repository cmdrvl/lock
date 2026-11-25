use serde_json::{Value, json};

use crate::cli::{DoctorAction, RobotDocsAction};

const DOCTOR_SCHEMA_VERSION: &str = "cmdrvl.doctor.v1";
const OPERATOR_JSON: &str = include_str!("../operator.json");
const LOCK_SCHEMA: &str = include_str!("../schemas/lock-v0.schema.json");
const VERIFY_SCHEMA: &str = include_str!("../schemas/lock-verify-v0.schema.json");

pub fn dispatch(
    robot_triage: bool,
    fix: bool,
    json_mode: bool,
    action: Option<&DoctorAction>,
) -> u8 {
    if fix {
        return fix_unavailable();
    }

    if robot_triage {
        return dispatch_robot_triage();
    }

    match action {
        Some(DoctorAction::Health { json }) => render_health(*json || json_mode),
        Some(DoctorAction::Capabilities { json }) => render_capabilities(*json || json_mode),
        Some(DoctorAction::Triage(args)) => render_triage(json_mode || args.json),
        Some(DoctorAction::RobotDocs) => {
            print_robot_docs(None);
            0
        }
        None => render_health(json_mode),
    }
}

pub fn dispatch_robot_triage() -> u8 {
    render_triage(true)
}

pub fn dispatch_capabilities(json_mode: bool) -> u8 {
    if json_mode {
        print_operator_json();
    } else {
        println!("lock capabilities");
        println!("operator contract: lock --describe");
        println!("doctor: lock doctor --json");
        println!("triage: lock doctor triage --json");
        println!("robot docs: lock robot-docs guide");
    }
    0
}

pub fn dispatch_robot_docs(action: Option<&RobotDocsAction>) -> u8 {
    print_robot_docs(action);
    0
}

fn render_health(json_mode: bool) -> u8 {
    let report = doctor_report("health");
    if json_mode {
        print_json(&report);
    } else {
        print_health_human(&report);
    }
    exit_for_report(&report)
}

fn render_capabilities(json_mode: bool) -> u8 {
    let report = doctor_report("capabilities");
    if json_mode {
        print_json(&report);
    } else {
        println!("lock doctor capabilities");
        println!("contract: lock capabilities --json");
        println!("healthy: {}", report["healthy"]);
    }
    exit_for_report(&report)
}

fn render_triage(json_mode: bool) -> u8 {
    let report = doctor_report("triage");
    if json_mode {
        print_json(&report);
    } else {
        print_health_human(&report);
        println!("next: lock capabilities --json");
    }
    exit_for_report(&report)
}

fn doctor_report(mode: &str) -> Value {
    let findings = health_findings();
    let healthy = !findings
        .iter()
        .any(|finding| finding.get("severity").and_then(Value::as_str) == Some("error"));
    let mut report = json!({
        "schema_version": DOCTOR_SCHEMA_VERSION,
        "mode": mode,
        "tool": "lock",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "healthy": healthy,
        "findings": findings,
    });

    match mode {
        "health" => {}
        "capabilities" => report["contract"] = operator_contract(),
        "triage" => {
            report["next_actions"] = if healthy {
                json!(["lock capabilities --json", "lock robot-docs guide"])
            } else {
                json!(["lock doctor --json", "lock doctor capabilities --json"])
            };
        }
        _ => unreachable!("doctor mode is fixed by the caller"),
    }
    report
}

fn health_findings() -> Vec<Value> {
    let checks = [
        (
            "lock.operator_manifest",
            operator_manifest_is_valid(),
            "operator.v1 manifest matches the compiled lock binary",
        ),
        (
            "lock.lock_schema",
            schema_has_title(LOCK_SCHEMA, "lock.v0"),
            "lock.v0 schema is available",
        ),
        (
            "lock.verify_schema",
            schema_has_title(VERIFY_SCHEMA, "lock-verify.v0"),
            "lock-verify.v0 schema is available",
        ),
        (
            "lock.source_date_epoch",
            source_date_epoch_is_valid(),
            "SOURCE_DATE_EPOCH is accepted for reproducible creation timestamps",
        ),
    ];

    checks
        .into_iter()
        .map(|(id, ok, message)| {
            let epoch_check = id == "lock.source_date_epoch";
            let severity = if ok {
                "info"
            } else if epoch_check {
                "warning"
            } else {
                "error"
            };
            let mut finding = json!({
                "id": id,
                "severity": severity,
                "message": message,
            });
            if !ok {
                finding["fix"] = json!(if epoch_check {
                    "set SOURCE_DATE_EPOCH to an integer Unix timestamp"
                } else {
                    "inspect lock --describe and the compiled schemas"
                });
            }
            finding
        })
        .collect()
}

fn source_date_epoch_is_valid() -> bool {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .is_none_or(|value| value.parse::<i64>().is_ok())
}

fn schema_has_title(schema: &str, expected: &str) -> bool {
    serde_json::from_str::<Value>(schema)
        .ok()
        .and_then(|value| {
            value
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|title| title == expected)
}

fn operator_contract() -> Value {
    serde_json::from_str(OPERATOR_JSON).expect("operator.json must be valid JSON")
}

fn operator_manifest_is_valid() -> bool {
    let Ok(value) = serde_json::from_str::<Value>(OPERATOR_JSON) else {
        return false;
    };
    let commands_ok = ["capabilities", "robot-docs", "doctor"]
        .into_iter()
        .all(|expected| {
            value
                .get("commands")
                .and_then(Value::as_array)
                .is_some_and(|commands| {
                    commands.iter().any(|command| {
                        command.get("name").and_then(Value::as_str) == Some(expected)
                    })
                })
        });

    value.get("schema_version").and_then(Value::as_str) == Some("operator.v1")
        && value.get("name").and_then(Value::as_str) == Some("lock")
        && value.get("tool_version").and_then(Value::as_str) == Some(env!("CARGO_PKG_VERSION"))
        && value.get("contract_version").and_then(Value::as_str) == Some("1.0.0")
        && commands_ok
}

fn print_operator_json() {
    print!("{OPERATOR_JSON}");
    if !OPERATOR_JSON.ends_with('\n') {
        println!();
    }
}

fn print_health_human(report: &Value) {
    let healthy = report["healthy"].as_bool().unwrap_or(false);
    println!(
        "lock doctor: {}",
        if healthy { "healthy" } else { "unhealthy" }
    );
    if let Some(findings) = report.get("findings").and_then(Value::as_array) {
        for finding in findings {
            println!(
                "- {}: {}",
                finding["id"].as_str().unwrap_or("unknown"),
                finding["message"].as_str().unwrap_or("unknown")
            );
        }
    }
}

fn print_robot_docs(action: Option<&RobotDocsAction>) {
    match action {
        Some(RobotDocsAction::Guide) | None => {}
    }

    println!("# lock robot-docs guide");
    println!();
    println!("`lock` exposes read-only discovery surfaces for agents.");
    println!(
        "`lock --robot-triage`, `lock capabilities --json`, `lock robot-docs guide`, and `lock doctor` do not read stdin or input files, create lockfiles, verify member content, append witness records, create witness directories, write doctor artifacts, rewrite metadata, or use the network."
    );
    println!();
    println!("Commands:");
    println!("- `lock --robot-triage` for a single JSON triage payload.");
    println!("- `lock capabilities --json` for command and side-effect policy.");
    println!("- `lock robot-docs guide` for this agent-oriented guide.");
    println!("- `lock [<INPUT>] [OPTIONS]` to create a lock.v0 artifact.");
    println!("- `lock verify <LOCKFILE> --json` to verify lockfile integrity.");
    println!("- `lock doctor health` for human health output.");
    println!("- `lock doctor health --json` for machine-readable health.");
    println!("- `lock doctor capabilities --json` for command and side-effect policy.");
    println!("- `lock doctor triage --json` for machine-readable next actions.");
    println!("- `lock doctor --robot-triage` for a single JSON triage payload.");
    println!();
    println!("Composition:");
    println!(
        "- Canonical chain: `vacuum --json <ROOT>... | hashbytes | fingerprint --fp <ID> | lock --dataset-id <DATASET> > dataset.lock.json`."
    );
    println!(
        "- `lock` consumes versioned JSONL from vacuum, hashbytes, or fingerprint and emits one `lock.v0` JSON artifact."
    );
    println!(
        "- Use `pack seal dataset.lock.json --output evidence/<DATASET>/` to seal the lockfile and related reports."
    );
    println!();
    println!(
        "Repair policy: `lock doctor --fix` is unavailable and exits 2 without stdout. Use `lock --robot-triage`, `lock capabilities --json`, or `lock robot-docs guide` for read-only diagnostics."
    );
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    );
}

fn fix_unavailable() -> u8 {
    use std::io::Write;

    let mut stderr = std::io::stderr();
    let _ = writeln!(
        stderr,
        "lock doctor --fix is unavailable: diagnostics are read-only in this release."
    );
    let _ = writeln!(stderr, "Try --robot-triage: lock --robot-triage");
    let _ = writeln!(stderr, "Try capabilities --json: lock capabilities --json");
    let _ = writeln!(stderr, "Try robot-docs guide: lock robot-docs guide");
    let _ = stderr.flush();
    2
}

fn exit_for_report(report: &Value) -> u8 {
    if report["healthy"].as_bool() == Some(true) {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::exit_for_report;

    #[test]
    fn unhealthy_doctor_report_exits_one() {
        let report = json!({"healthy": false});
        assert_eq!(exit_for_report(&report), 1);
    }
}
