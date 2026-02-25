use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Dataset lockfile tool: pins artifacts, fingerprints, and tool versions
/// into a single immutable, self-hashed lockfile.
#[derive(Debug, Parser)]
#[command(name = "lock", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// JSONL manifest file (default: stdin)
    pub input: Option<PathBuf>,

    /// Logical dataset identifier
    #[arg(long)]
    pub dataset_id: Option<String>,

    /// Point-in-time for this lock (ISO 8601)
    #[arg(long)]
    pub as_of: Option<String>,

    /// Free-text annotation
    #[arg(long)]
    pub note: Option<String>,

    /// Suppress witness ledger recording for this run
    #[arg(long)]
    pub no_witness: bool,

    /// Print compiled operator.json and exit
    #[arg(long)]
    pub describe: bool,

    /// Print lock.v0 JSON Schema and exit
    #[arg(long)]
    pub schema: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Query the witness ledger
    Witness {
        #[command(subcommand)]
        action: WitnessAction,
    },
}

/// Witness query filter flags shared by `query` and `count` subcommands.
#[derive(Debug, clap::Args, Clone, Default)]
pub struct WitnessFilters {
    /// Filter by tool name
    #[arg(long)]
    pub tool: Option<String>,

    /// Include records after this timestamp (ISO 8601)
    #[arg(long)]
    pub since: Option<String>,

    /// Include records before this timestamp (ISO 8601)
    #[arg(long)]
    pub until: Option<String>,

    /// Filter by outcome (LOCK_CREATED, LOCK_PARTIAL, or REFUSAL)
    #[arg(long)]
    pub outcome: Option<String>,

    /// Filter by input hash substring
    #[arg(long)]
    pub input_hash: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum WitnessAction {
    /// Search witness records by filters
    Query {
        #[command(flatten)]
        filters: WitnessFilters,

        /// Maximum number of records to return
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// Output as JSON instead of human-readable
        #[arg(long)]
        json: bool,
    },

    /// Show the most recent witness record
    Last {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Count matching witness records
    Count {
        #[command(flatten)]
        filters: WitnessFilters,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Parse CLI arguments and dispatch to the appropriate handler.
///
/// Returns the process exit code:
/// - `0`: success (LOCK_CREATED, --describe, --schema, --version, witness queries)
/// - `1`: LOCK_PARTIAL or witness query with no matches
/// - `2`: REFUSAL or CLI parse error
pub fn run() -> u8 {
    let cli = Cli::parse();

    // Witness subcommands dispatch first (before input validation).
    if let Some(Command::Witness { action }) = &cli.command {
        return dispatch_witness(action);
    }

    // --describe and --schema are checked before input is opened.
    if cli.describe {
        return dispatch_describe();
    }
    if cli.schema {
        return dispatch_schema();
    }

    // Main lock flow — delegates to orchestration (bd-1ab).
    dispatch_lock(&cli)
}

/// Dispatch witness subcommands to the witness module.
fn dispatch_witness(action: &WitnessAction) -> u8 {
    use crate::witness;

    match action {
        WitnessAction::Query {
            filters,
            limit,
            json,
        } => witness::dispatch_query(filters, *limit, *json),
        WitnessAction::Last { json } => witness::dispatch_last(*json),
        WitnessAction::Count { filters, json } => witness::dispatch_count(filters, *json),
    }
}

/// Compiled-in operator contract, embedded at build time.
const OPERATOR_JSON: &str = include_str!("../../operator.json");

/// Compiled-in lock.v0 JSON Schema, embedded at build time.
const LOCK_SCHEMA: &str = include_str!("../../schemas/lock-v0.schema.json");

/// Emit the compiled-in operator.json to stdout and exit 0.
fn dispatch_describe() -> u8 {
    print!("{OPERATOR_JSON}");
    0
}

/// Emit the lock.v0 JSON Schema to stdout and exit 0.
fn dispatch_schema() -> u8 {
    print!("{LOCK_SCHEMA}");
    0
}

/// Run the main lock flow: read input, classify records, build lockfile.
fn dispatch_lock(cli: &Cli) -> u8 {
    crate::run_lock(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_args() {
        let cli = Cli::try_parse_from(["lock"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.input.is_none());
        assert!(cli.dataset_id.is_none());
        assert!(cli.as_of.is_none());
        assert!(cli.note.is_none());
        assert!(!cli.no_witness);
        assert!(!cli.describe);
        assert!(!cli.schema);
    }

    #[test]
    fn parse_input_file() {
        let cli = Cli::try_parse_from(["lock", "manifest.jsonl"]).unwrap();
        assert_eq!(cli.input, Some(PathBuf::from("manifest.jsonl")));
    }

    #[test]
    fn parse_all_metadata_flags() {
        let cli = Cli::try_parse_from([
            "lock",
            "--dataset-id",
            "raw-dec",
            "--as-of",
            "2025-12-31T23:59:59Z",
            "--note",
            "Final delivery",
            "input.jsonl",
        ])
        .unwrap();
        assert_eq!(cli.dataset_id.as_deref(), Some("raw-dec"));
        assert_eq!(cli.as_of.as_deref(), Some("2025-12-31T23:59:59Z"));
        assert_eq!(cli.note.as_deref(), Some("Final delivery"));
        assert_eq!(cli.input, Some(PathBuf::from("input.jsonl")));
    }

    #[test]
    fn parse_no_witness_flag() {
        let cli = Cli::try_parse_from(["lock", "--no-witness"]).unwrap();
        assert!(cli.no_witness);
    }

    #[test]
    fn parse_describe_flag() {
        let cli = Cli::try_parse_from(["lock", "--describe"]).unwrap();
        assert!(cli.describe);
    }

    #[test]
    fn parse_schema_flag() {
        let cli = Cli::try_parse_from(["lock", "--schema"]).unwrap();
        assert!(cli.schema);
    }

    #[test]
    fn parse_witness_query() {
        let cli = Cli::try_parse_from([
            "lock",
            "witness",
            "query",
            "--tool",
            "lock",
            "--since",
            "2026-01-01T00:00:00Z",
            "--outcome",
            "LOCK_CREATED",
            "--limit",
            "10",
            "--json",
        ])
        .unwrap();
        match &cli.command {
            Some(Command::Witness {
                action:
                    WitnessAction::Query {
                        filters,
                        limit,
                        json,
                    },
            }) => {
                assert_eq!(filters.tool.as_deref(), Some("lock"));
                assert_eq!(filters.since.as_deref(), Some("2026-01-01T00:00:00Z"));
                assert_eq!(filters.outcome.as_deref(), Some("LOCK_CREATED"));
                assert_eq!(*limit, 10);
                assert!(*json);
            }
            other => panic!("expected Witness/Query, got {other:?}"),
        }
    }

    #[test]
    fn parse_witness_last() {
        let cli = Cli::try_parse_from(["lock", "witness", "last", "--json"]).unwrap();
        match &cli.command {
            Some(Command::Witness {
                action: WitnessAction::Last { json },
            }) => {
                assert!(*json);
            }
            other => panic!("expected Witness/Last, got {other:?}"),
        }
    }

    #[test]
    fn parse_witness_count() {
        let cli = Cli::try_parse_from([
            "lock",
            "witness",
            "count",
            "--tool",
            "hash",
            "--input-hash",
            "a1b2c3",
            "--json",
        ])
        .unwrap();
        match &cli.command {
            Some(Command::Witness {
                action: WitnessAction::Count { filters, json },
            }) => {
                assert_eq!(filters.tool.as_deref(), Some("hash"));
                assert_eq!(filters.input_hash.as_deref(), Some("a1b2c3"));
                assert!(*json);
            }
            other => panic!("expected Witness/Count, got {other:?}"),
        }
    }

    #[test]
    fn parse_witness_query_with_until() {
        let cli = Cli::try_parse_from([
            "lock",
            "witness",
            "query",
            "--until",
            "2026-02-01T00:00:00Z",
        ])
        .unwrap();
        match &cli.command {
            Some(Command::Witness {
                action: WitnessAction::Query { filters, .. },
            }) => {
                assert_eq!(filters.until.as_deref(), Some("2026-02-01T00:00:00Z"));
            }
            other => panic!("expected Witness/Query, got {other:?}"),
        }
    }

    #[test]
    fn reject_unknown_flag() {
        let result = Cli::try_parse_from(["lock", "--bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn operator_json_is_valid_json() {
        let parsed: serde_json::Value =
            serde_json::from_str(OPERATOR_JSON).expect("operator.json must be valid JSON");
        assert_eq!(parsed["name"], "lock");
        assert_eq!(parsed["schema_version"], "operator.v0");
        assert_eq!(parsed["version"], "0.1.0");
    }

    #[test]
    fn lock_schema_is_valid_json() {
        let parsed: serde_json::Value =
            serde_json::from_str(LOCK_SCHEMA).expect("lock schema must be valid JSON");
        assert_eq!(parsed["title"], "lock.v0");
        assert!(parsed["properties"]["version"].is_object());
        assert!(parsed["properties"]["lock_hash"].is_object());
        assert!(parsed["properties"]["members"].is_object());
        assert!(parsed["properties"]["skipped"].is_object());
    }

    #[test]
    fn dispatch_describe_returns_zero() {
        assert_eq!(dispatch_describe(), 0);
    }

    #[test]
    fn dispatch_schema_returns_zero() {
        assert_eq!(dispatch_schema(), 0);
    }

    #[test]
    fn unknown_word_parsed_as_input_file() {
        // An unknown word is treated as the positional INPUT argument, not a subcommand.
        let cli = Cli::try_parse_from(["lock", "frobnicate"]).unwrap();
        assert_eq!(cli.input, Some(PathBuf::from("frobnicate")));
        assert!(cli.command.is_none());
    }
}
