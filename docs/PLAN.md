# lock — Dataset Lockfile

## One-line promise

**Pin every artifact, fingerprint, and tool version for a dataset into a single immutable, self-hashed lockfile.**

Like `Cargo.lock` for data. If the lockfile hash matches, the dataset is exactly what you think it is.

Second promise: **Make pipeline reproducibility a file, not a prayer.**

---

## Problem (clearly understood)

You've scanned, hashed, and fingerprinted a dataset. Now you need to answer: what exactly was in this delivery? Today this means:

- Directory listings and ad-hoc checksums
- No single source of truth for which files, hashes, and template matches were observed
- No record of which tool versions produced the pipeline
- No structured accounting for files that failed along the way
- No way to verify the lockfile itself hasn't been tampered with

`lock` replaces that with **one deterministic, self-verifiable artifact** that pins the complete state of a dataset pipeline run.

---

## Non-goals (explicit)

`lock` is NOT:

- A scanner (that's `vacuum`)
- A hasher (that's `hash`)
- A template recognizer (that's `fingerprint`)
- An evidence bundle (that's `pack`)
- A comparability gate (that's `shape`)

It does not tell you *what's in* any file.
It tells you *exactly which files, hashes, fingerprints, and tool versions were observed*, in a tamper-evident envelope.

---

## Relationship to the pipeline

`lock` is the terminal tool in the stream pipeline. It consumes the JSONL stream and produces a single JSON artifact:

```bash
vacuum /data/dec/ | hash | lock --dataset-id "raw-dec" > raw.lock.json
```

With fingerprinting:

```bash
vacuum /data/models/ | hash | fingerprint --fp argus-model.v1 \
  | lock --dataset-id "argus-models-2025-12" --as-of "2025-12-31" > models.lock.json
```

With annotation:

```bash
lock --dataset-id "q4-final" --note "Final delivery after restatement" < fingerprinted.jsonl > q4.lock.json
```

Lock then seal as evidence:

```bash
vacuum /data/dec/ | hash | fingerprint --fp csv.v0 \
  | lock --dataset-id "dec" > dec.lock.json && pack seal dec.lock.json --output evidence/dec/
```

All spine pipeline tools share:

- The same `_skipped` / `_warnings` protocol (evidence-grade pipeline degradation)
- The same `tool_versions` accumulation pattern (versions travel with data)
- The same refusal system (`E_UPPERCASE` codes, concrete next steps)
- The same exit code conventions (0 = positive, 1 = domain-negative, 2 = refusal)
- The same witness protocol (append-only proof ledger)

---

## Tool category

`lock` is an **artifact tool**. It always produces a single JSON object to stdout. There is no human/JSON toggle — `lock` output is always structured JSON.

This distinguishes it from:
- **Stream tools** (vacuum, hash, fingerprint): JSONL to stdout
- **Report tools** (shape, rvl, verify, compare): human or `--json` mode

---

## CLI (v0)

```bash
lock [<INPUT>] [OPTIONS]
lock witness <query|last|count> [OPTIONS]
```

### Arguments

- `[INPUT]`: JSONL manifest file (default: stdin)

### Flags

- `--dataset-id <ID>`: logical dataset identifier. Recorded in the lockfile. Optional — null if not provided.
- `--as-of <TIMESTAMP>`: point-in-time for this lock (ISO 8601). Recorded, not interpreted. Optional — null if not provided.
- `--note <TEXT>`: free-text annotation. Recorded, not interpreted. Optional — null if not provided.
- `--no-witness`: suppress witness ledger recording for this run.
- `--describe`: print the compiled-in `operator.json` to stdout and exit 0. Checked before input is validated, so `lock --describe` works with no arguments.
- `--schema`: print the JSON Schema for `lock.v0` output to stdout and exit 0. Like `--describe`, checked before input is validated.
- `--version`: print `lock <semver>` to stdout and exit 0.

### Exit codes

- `0`: LOCK_CREATED — all input records are members. No records were skipped.
- `1`: LOCK_PARTIAL — lock created, but one or more input records were skipped (recorded in `skipped` array). The lock is valid but incomplete.
- `2`: REFUSAL / CLI error — lock was NOT created.

### Streams

- stdout: single JSON object (the lockfile) for exit 0 or 1. Refusal JSON envelope for exit 2.
- stderr: process-level diagnostics only. Not a system of record — all evidence-grade information is in the JSON output.

### Witness ledger (epistemic spine parity)

`lock` follows the same ambient witness protocol as `rvl` and `shape`:

- Default behavior: every lock run (success or refusal) appends exactly one `witness.v0` record.
- Opt-out: `--no-witness`.
- Ledger path resolution:
  1. `EPISTEMIC_WITNESS` env var, if set
  2. `~/.epistemic/witness.jsonl` otherwise
- Witness failures never change the domain exit code. If append/query fails, print a warning to stderr and preserve domain result semantics.
- `outcome` in the witness record: `"LOCK_CREATED"` (exit 0), `"LOCK_PARTIAL"` (exit 1), or `"REFUSAL"` (exit 2).

Witness query subcommands (same shape as rvl):

```bash
lock witness query [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] \
  [--limit <n>] [--json]

lock witness last [--json]
lock witness count [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] [--json]
```

`lock witness` is read/query-only. It does not mutate ledger state.

---

## Outcomes (exactly two)

### 1. LOCK_CREATED (exit 0)

Every input record became a member. No records were skipped. The lockfile is complete.

### 2. LOCK_PARTIAL (exit 1)

Lock created, but some input records were skipped due to upstream `_skipped: true` markers. The `skipped` array records exactly which records were excluded and why. The lock is valid (self-hash checks out) but does not cover the full input set. Note: non-skipped records missing required fields (e.g., `bytes_hash`) trigger a refusal (`E_MISSING_HASH`), not a partial lock.

### 3. REFUSAL (exit 2)

Lock was NOT created. The input was empty, unparseable, or lacked required fields across all records.

---

## Input contract

### JSONL from stdin or file

`lock` reads newline-delimited JSON (one record per line) from stdin or a named file. Each record is expected to be the output of `hash` or `fingerprint`.

### Required fields

Every non-skipped input record MUST have:

| Field | Required by | Notes |
|-------|-------------|-------|
| `version` | all stream tools | Record schema version (e.g., `hash.v0`, `fingerprint.v0`). Used for version compatibility check. |
| `path` | `vacuum` | Absolute path to the artifact |
| `relative_path` | `vacuum` | Path relative to scan root (used as member key) |
| `bytes_hash` | `hash` | Content hash in `<algorithm>:<hex>` format |
| `size` | `vacuum` | File size in bytes |
| `tool_versions` | all stream tools | Accumulated tool version map |

### Optional fields (preserved if present)

| Field | Source | Notes |
|-------|--------|-------|
| `root` | `vacuum` | Scan root directory |
| `mtime` | `vacuum` | Last modified timestamp |
| `extension` | `vacuum` | File extension |
| `mime_guess` | `vacuum` | MIME type guess |
| `hash_algorithm` | `hash` | Algorithm used |
| `fingerprint` | `fingerprint` | Fingerprint result object |

### Handling `_skipped` records

Records with `_skipped: true` are NOT added to the `members` array. Instead:

1. Their `relative_path` (or `path` if `relative_path` absent) and accumulated `_warnings` are recorded in the `skipped` array.
2. The lockfile's `skipped_count` reflects how many were excluded.
3. Exit code is `1` (partial) when any records are skipped.

Records missing `bytes_hash` (without `_skipped: true`) trigger a refusal (`E_MISSING_HASH`).

### Version compatibility

- `lock` accepts records with `version` fields `vacuum.v0`, `hash.v0`, or `fingerprint.v0`.
- Records with a missing or unrecognized `version` field (e.g., `hash.v2`, or no `version` key at all) cause a refusal (`E_BAD_INPUT`).

---

## Refusal codes

| Code | Trigger | Next step |
|------|---------|-----------|
| `E_EMPTY` | No input records (stdin was empty or file is empty) | Provide artifacts — run `vacuum` first |
| `E_BAD_INPUT` | Invalid JSONL (parse error) or unknown record version | Check upstream tool output |
| `E_MISSING_HASH` | One or more non-skipped records lack `bytes_hash` | Run `hash` first |

### Refusal JSON envelope

```json
{
  "version": "lock.v0",
  "outcome": "REFUSAL",
  "refusal": {
    "code": "E_MISSING_HASH",
    "message": "3 records lack bytes_hash — run hash first",
    "detail": {
      "count": 3,
      "sample_paths": ["data/model.xlsx", "data/tape.csv", "data/readme.pdf"]
    },
    "next_command": "vacuum /data/ | hash | lock --dataset-id \"my-dataset\""
  }
}
```

The `next_command` field provides a literal copy/paste command for mechanical recovery. Agents use it directly; humans copy/paste.

### Refusal detail schemas

```
E_EMPTY:
  { }

E_BAD_INPUT (parse error):
  { "line": 42, "error": "expected value at line 1 column 1" }

E_BAD_INPUT (unknown version):
  { "line": 3, "version": "hash.v2" }

E_MISSING_HASH:
  { "count": 3, "sample_paths": ["data/model.xlsx", "data/tape.csv", "data/readme.pdf"] }
```

---

## JSON output schema

### Top-level fields

| Field | Type | Nullable | Notes |
|-------|------|----------|-------|
| `version` | string | no | `"lock.v0"` — always present |
| `lock_hash` | string | no | Self-hash (see Self-hash below) |
| `dataset_id` | string | yes | From `--dataset-id`; null if not provided |
| `as_of` | string | yes | ISO 8601 from `--as-of`; null if not provided |
| `note` | string | yes | From `--note`; null if not provided |
| `created` | string | no | ISO 8601, UTC — time the lock was created |
| `tool_versions` | object | no | Map of tool name to semver for all tools that touched these records (merged from input `tool_versions` + lock's own version) |
| `profiles` | string[] | no | Deduplicated list of profile IDs. Always `[]` in v0 — stream pipeline tools don't use profiles. Reserved for future use. |
| `skipped` | object[] | no | Sorted by `path`; records excluded from members. Empty array when no records were skipped. |
| `members` | object[] | no | Sorted by `path` (lexicographic, byte-order). The locked artifacts. |
| `skipped_count` | u64 | no | Length of `skipped` array |
| `member_count` | u64 | no | Length of `members` array |

### Member object

Each entry in `members` represents one successfully processed artifact:

| Field | Type | Nullable | Notes |
|-------|------|----------|-------|
| `path` | string | no | `relative_path` from input (forward-slash normalized) |
| `bytes_hash` | string | no | `"<algorithm>:<hex>"` |
| `size` | u64 | no | File size in bytes |
| `fingerprint` | object | yes | Fingerprint result; null if `fingerprint` was not in the pipeline |

When `fingerprint` is present:

| Field | Type | Nullable | Notes |
|-------|------|----------|-------|
| `fingerprint_id` | string | no | Which fingerprint matched |
| `fingerprint_version` | string | no | Fingerprint crate version |
| `matched` | bool | no | Whether the fingerprint matched |
| `content_hash` | string | yes | BLAKE3 of matched content; null if not matched |

### Skipped entry object

Each entry in `skipped` represents a record that was excluded:

| Field | Type | Notes |
|-------|------|-------|
| `path` | string | `relative_path` (or `path` if absent) from the input record |
| `warnings` | object[] | Accumulated `_warnings` from the stream pipeline |

Warning object shape:

```json
{
  "tool": "hash",
  "code": "E_IO",
  "message": "Cannot read file",
  "detail": {}
}
```

> **Note:** The examples below show keys in logical reading order for clarity. The actual canonical output has all keys sorted alphabetically at every level (e.g., `as_of` before `created` before `dataset_id` …). See **Canonical JSON serialization** for the implementation constraint.

### Full example — LOCK_CREATED (exit 0)

```json
{
  "version": "lock.v0",
  "lock_hash": "sha256:a1b2c3d4e5f6...",
  "dataset_id": "argus-models-2025-12",
  "as_of": "2025-12-31T23:59:59Z",
  "note": "Q4 2025 final delivery",
  "created": "2026-01-15T10:30:00Z",
  "tool_versions": {
    "vacuum": "0.1.0",
    "hash": "0.1.0",
    "fingerprint": "0.1.0",
    "lock": "0.1.0"
  },
  "profiles": [],
  "skipped": [],
  "members": [
    {
      "path": "model.xlsx",
      "bytes_hash": "sha256:e3b0c44298fc1c149afbf4c8996fb924...",
      "size": 2481920,
      "fingerprint": {
        "fingerprint_id": "argus-model.v1",
        "fingerprint_version": "0.3.2",
        "matched": true,
        "content_hash": "blake3:9f2a..."
      }
    },
    {
      "path": "tape.csv",
      "bytes_hash": "sha256:7d865e959b2466918c9863afca942d0f...",
      "size": 847201,
      "fingerprint": {
        "fingerprint_id": "csv.v0",
        "fingerprint_version": "0.1.0",
        "matched": true,
        "content_hash": "blake3:4e1c..."
      }
    }
  ],
  "skipped_count": 0,
  "member_count": 2
}
```

### Full example — LOCK_PARTIAL (exit 1)

```json
{
  "version": "lock.v0",
  "lock_hash": "sha256:f9e8d7c6b5a4...",
  "dataset_id": "dec-delivery",
  "as_of": null,
  "note": null,
  "created": "2026-01-20T14:00:00Z",
  "tool_versions": {
    "vacuum": "0.1.0",
    "hash": "0.1.0",
    "lock": "0.1.0"
  },
  "profiles": [],
  "skipped": [
    {
      "path": "corrupt.pdf",
      "warnings": [
        {
          "tool": "hash",
          "code": "E_IO",
          "message": "Cannot read file: permission denied",
          "detail": {}
        }
      ]
    }
  ],
  "members": [
    {
      "path": "model.xlsx",
      "bytes_hash": "sha256:e3b0c44298fc1c149afbf4c8996fb924...",
      "size": 2481920,
      "fingerprint": null
    },
    {
      "path": "tape.csv",
      "bytes_hash": "sha256:7d865e959b2466918c9863afca942d0f...",
      "size": 847201,
      "fingerprint": null
    }
  ],
  "skipped_count": 1,
  "member_count": 2
}
```

---

## Self-hash

The `lock_hash` field makes the lockfile self-verifiable. The algorithm:

1. Build the complete lockfile JSON with `lock_hash` set to `""` (empty string).
2. Serialize to **canonical form**: sorted keys, compact representation (no unnecessary whitespace), no trailing newline, floats in their shortest round-trip representation.
3. Compute SHA256 of the canonical byte sequence.
4. Set `lock_hash` to `"sha256:<hex>"`.
5. Re-serialize with the real `lock_hash` value — this is the final output.

The canonical serialization is what the tool emits. Verifiers must reproduce this exact byte sequence (with `lock_hash` set to `""`) to check integrity.

### Verification algorithm

```
1. Read lockfile JSON
2. Extract lock_hash value
3. Set lock_hash to ""
4. Serialize to canonical JSON (sorted keys, compact, no trailing newline)
5. SHA256 of the result
6. Compare "sha256:<hex>" with extracted lock_hash
7. Match → verified. Mismatch → tampered.
```

### Cross-platform determinism

Lock determinism depends on:

- **Canonical relative paths** (forward-slash normalized, from `relative_path`)
- **Content hashes** (algorithm-prefixed hex)
- **Sorted members** (lexicographic byte-order on `path`)
- **Canonical JSON serialization** (sorted keys, compact)

Given the same input records and the same `created` timestamp, a lockfile produced on macOS and one produced on Windows should have the same `lock_hash`. The `created` field is the only non-deterministic input — all other fields are derived from the input stream (normalized relative paths, content hashes, sorted members).

---

## Witness Record

lock's witness record follows the standard `witness.v0` schema:

```json
{
  "id": "blake3:...",
  "tool": "lock",
  "version": "0.1.0",
  "binary_hash": "blake3:...",
  "inputs": [
    { "path": "stdin", "hash": null, "bytes": null }
  ],
  "params": { "dataset_id": "dec-delivery", "as_of": null, "note": null },
  "outcome": "LOCK_CREATED",
  "exit_code": 0,
  "output_hash": "blake3:...",
  "prev": "blake3:...",
  "ts": "2026-02-24T10:00:00Z"
}
```

For lock, `inputs` describes the JSONL source: `"stdin"` when piped, or the file path when a positional argument is given. `inputs[].hash` and `inputs[].bytes` are `null` because stdin cannot be pre-hashed (it is consumed during reading). When a file argument is provided, `hash` and `bytes` can be populated after reading. The `output_hash` is BLAKE3 of the final stdout output (the lockfile JSON or refusal envelope).

---

## Implementation notes

### Execution flow

```
 1. Parse CLI (clap)           → exit 2 on bad args; --version handled here by clap
 2. If witness subcommand: dispatch to witness query/last/count, exit
 3. If --describe / --schema: emit and exit 0
 4. Open input (file or stdin)
 5. For each JSONL line:
    a. Parse JSON → on parse failure: refuse E_BAD_INPUT immediately
    b. Check version field → on unknown version: refuse E_BAD_INPUT
    c. Merge tool_versions from record into accumulated map (all records, including skipped)
    d. If _skipped: true → collect into skipped list
    e. If missing bytes_hash (and not _skipped) → collect for E_MISSING_HASH refusal
    f. Otherwise → collect into members list
 6. If no records at all → refuse E_EMPTY
 7. If any non-skipped records lack bytes_hash → refuse E_MISSING_HASH
    → On refusal (steps 5a/5b/6/7): emit refusal envelope to stdout, append
      witness record with outcome "REFUSAL" (if not --no-witness), exit 2
 8. Sort members by path (lexicographic, byte-order)
 9. Sort skipped by path
10. Build lockfile JSON with lock_hash = ""
11. Serialize to canonical JSON
12. SHA256 → lock_hash
13. Re-serialize with real lock_hash
14. Write to stdout
15. Append witness record (unless --no-witness); output_hash is
    BLAKE3 of the final stdout output (per spine witness protocol).
    Note: this is different from lock_hash (SHA256 of canonical JSON
    with lock_hash=""). The witness hashes the final emitted bytes;
    the self-hash covers the pre-hash canonical form.
16. Exit 0 (all members) or 1 (has skipped)
```

### Core data structures

```rust
/// Top-level lockfile
struct Lockfile {
    version: String,              // "lock.v0"
    lock_hash: String,            // "sha256:..." or "" during computation
    dataset_id: Option<String>,
    as_of: Option<String>,
    note: Option<String>,
    created: String,              // ISO 8601 UTC
    tool_versions: BTreeMap<String, String>,
    profiles: Vec<String>,        // always empty in v0
    skipped: Vec<SkippedEntry>,
    members: Vec<Member>,
    skipped_count: u64,
    member_count: u64,
}

/// A successfully processed artifact
struct Member {
    path: String,                 // relative_path, forward-slash normalized
    bytes_hash: String,           // "<algorithm>:<hex>"
    size: u64,
    fingerprint: Option<FingerprintResult>,
}

/// Fingerprint result from upstream
struct FingerprintResult {
    fingerprint_id: String,
    fingerprint_version: String,
    matched: bool,
    content_hash: Option<String>, // blake3 of matched content
}

/// A record excluded from members
struct SkippedEntry {
    path: String,
    warnings: Vec<Warning>,
}

/// Structured warning from upstream pipeline
struct Warning {
    tool: String,
    code: String,
    message: String,
    detail: serde_json::Value,
}

// === CLI ===

#[derive(Parser)]
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

    /// Suppress witness ledger recording
    #[arg(long)]
    pub no_witness: bool,

    /// Print operator.json and exit
    #[arg(long)]
    pub describe: bool,

    /// Print JSON Schema and exit
    #[arg(long)]
    pub schema: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Query the witness ledger
    Witness {
        #[command(subcommand)]
        action: WitnessAction,
    },
}

#[derive(Subcommand)]
pub enum WitnessAction {
    Query { /* filter flags */ },
    Last,
    Count { /* filter flags */ },
}
```

### Canonical JSON serialization

The self-hash requires deterministic serialization. Options:

- **serde_json with sorted keys**: Use `BTreeMap` for all map types (not `HashMap`). Serialize with `serde_json::to_string` (compact, no trailing newline). BTreeMap guarantees sorted keys.
- **Explicit canonical form**: If needed, use a dedicated canonical JSON serializer (e.g., `json-canonicalization` crate implementing RFC 8785).

For v0, `BTreeMap` + `serde_json::to_string` is sufficient for nested maps. However, **the top-level `Lockfile` struct must not be serialized directly** — serde serializes struct fields in declaration order, not alphabetically. To produce sorted keys at the top level, serialize via `serde_json::to_value()` first (which converts structs to `Value::Object(Map)`), then use `serde_json::to_string()` on the `Value`. `serde_json::Map` preserves insertion order, but `to_value()` from a struct with `BTreeMap` fields will produce sorted nested keys. For the top level: either (a) manually construct a `BTreeMap<String, Value>`, or (b) use the `json-canonicalization` crate (RFC 8785) which handles key sorting at all levels. Option (a) is simpler for v0.

Float representation: `serde_json` uses shortest round-trip representation for f64. This matches the canonical form requirement. (In practice, lockfiles contain no floats — sizes are u64, hashes are strings.)

### Module structure

```
lock/
├── src/
│   ├── main.rs          # Minimal: calls lock::run(), maps to ExitCode
│   ├── lib.rs           # pub fn run() → u8 (handles errors internally, returns exit code)
│   ├── cli/
│   │   └── mod.rs       # clap derive Cli / Command / WitnessAction
│   ├── input/
│   │   └── mod.rs       # JSONL reader, record parsing, field extraction
│   ├── lockfile/
│   │   ├── mod.rs       # Lockfile construction, member/skipped sorting
│   │   └── self_hash.rs # Canonical serialization and SHA256 self-hash
│   ├── output/
│   │   └── mod.rs       # JSON output (lockfile or refusal envelope)
│   ├── refusal/
│   │   └── mod.rs       # E_EMPTY, E_BAD_INPUT, E_MISSING_HASH
│   └── witness/
│       └── mod.rs       # Witness record append, query subcommands
├── operator.json        # Machine-readable tool descriptor
├── Cargo.toml
└── docs/
    └── PLAN.md          # This file
```

### `main.rs` (≤15 lines)

```rust
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let code = lock::run();
    std::process::ExitCode::from(code)
}
```

### Key dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `serde` + `serde_json` | JSON serialization/deserialization |
| `sha2` | SHA256 for self-hash |
| `blake3` | BLAKE3 for witness record hashing |
| `chrono` | ISO 8601 timestamp generation |

### tool_versions accumulation

`lock` reads `tool_versions` from every input record and merges them into a single map:

1. Start with an empty `BTreeMap<String, String>`.
2. For each input record (both members and skipped), merge its `tool_versions` into the accumulator. Skipped records still carry upstream tool versions and must be included — otherwise an all-skipped input would lose upstream version information. If the same tool appears with different versions across records (shouldn't happen in a single pipeline run), keep the first version seen.
3. Add `{ "lock": "<lock's own semver>" }` to the merged map.
4. This becomes the lockfile's `tool_versions`.

The versions travel through the pipeline on every record — lock doesn't need to spawn subprocesses or query tool binaries.

---

## operator.json

```json
{
  "schema_version": "operator.v0",
  "name": "lock",
  "version": "0.1.0",
  "description": "Dataset lockfile — pins artifacts, fingerprints, and tool versions into a self-hashed, immutable JSON artifact",
  "repository": "https://github.com/cmdrvl/lock",
  "license": "MIT",

  "invocation": {
    "binary": "lock",
    "output_mode": "artifact",
    "output_schema": "lock.v0",
    "json_flag": null
  },

  "arguments": [
    { "name": "input", "type": "file_path", "required": false, "position": 0, "description": "JSONL manifest file (default: stdin)" }
  ],

  "options": [
    { "name": "dataset_id", "flag": "--dataset-id", "type": "string", "description": "Logical dataset identifier" },
    { "name": "as_of", "flag": "--as-of", "type": "string", "description": "Point-in-time for this lock (ISO 8601)" },
    { "name": "note", "flag": "--note", "type": "string", "description": "Free-text annotation" }
  ],

  "exit_codes": {
    "0": { "meaning": "LOCK_CREATED", "domain": "positive" },
    "1": { "meaning": "LOCK_PARTIAL", "domain": "negative" },
    "2": { "meaning": "REFUSAL", "domain": "error" }
  },

  "refusals": [
    { "code": "E_EMPTY", "message": "No input records", "action": "run_upstream", "tool": "vacuum" },
    { "code": "E_BAD_INPUT", "message": "Invalid JSONL or unknown record version", "action": "escalate" },
    { "code": "E_MISSING_HASH", "message": "Records lack bytes_hash", "action": "run_upstream", "tool": "hash" }
  ],

  "capabilities": {
    "formats": ["jsonl"],
    "profile_aware": false,
    "streaming": false
  },

  "pipeline": {
    "upstream": ["hash", "fingerprint"],
    "downstream": ["pack"]
  }
}
```

---

## Testing requirements

### Unit tests

| Area | Tests |
|------|-------|
| JSONL parsing | Valid records, malformed JSON, empty input, records with unknown fields (ignored) |
| `_skipped` handling | Skipped records go to `skipped` array, not `members`; exit code 1 |
| `E_MISSING_HASH` | Non-skipped records without `bytes_hash` trigger refusal |
| Member sorting | Members sorted by path (lexicographic, byte-order); edge cases with Unicode paths |
| `tool_versions` merge | Versions from multiple records merged correctly; lock adds its own |
| Self-hash | Compute → verify round-trip; canonical JSON determinism |
| Nullable fields | `dataset_id`, `as_of`, `note` all null when not provided |
| Fingerprint passthrough | Members with and without fingerprint results |

### Integration tests

| Scenario | Assertion |
|----------|-----------|
| `vacuum \| hash \| lock` | Exit 0, all files in members, skipped empty |
| `vacuum \| hash \| fingerprint \| lock` | Exit 0, fingerprint results in member objects |
| Pipeline with one unreadable file | Exit 1, skipped contains the file with warnings |
| Empty stdin | Exit 2, `E_EMPTY` refusal |
| Pipe from `vacuum` only (no hash) | Exit 2, `E_MISSING_HASH` refusal |
| Self-hash verification | Parse output, blank `lock_hash`, re-serialize, SHA256 matches |
| Determinism | Same input with fixed `created` timestamp → identical `lock_hash`. Tests must fixture `created` (e.g., via injectable clock) because the real timestamp varies per run. |
| Cross-platform paths | Forward-slash normalization in member paths |

### Witness tests

| Scenario | Assertion |
|----------|-----------|
| Default run | Witness record appended to `~/.epistemic/witness.jsonl` |
| `--no-witness` | No witness record written |
| `lock witness query` | Returns matching records from ledger |
| `lock witness last` | Returns most recent lock record |
| `lock witness count` | Returns correct count for filters |

---

## Scope

### v0.1 (ship this)

- JSONL input from stdin or file
- `--dataset-id`, `--as-of`, `--note` metadata
- `_skipped` record handling with exit code 1
- Self-hash (SHA256 of canonical JSON)
- `tool_versions` accumulation from pipeline
- Refusal codes: `E_EMPTY`, `E_BAD_INPUT`, `E_MISSING_HASH`
- Refusal JSON envelope with `next_command`
- `--describe`, `--schema`, `--version`
- Witness ledger protocol
- `operator.json`

### Defer

- `profiles` field population (depends on profile tool)
- `--strict` mode (refuse on any `_skipped` instead of exit 1)
- Lock comparison / diff tooling
- Witness-to-data-fabric sync (`lock push`)
- Cryptographic signing (GPG, Sigstore) — service-layer concern
- Incremental locking (delta from previous lock via witness ledger queries)

### Open questions

None. Lock is the simplest artifact tool in the spine — its design is fully determined by the stream protocol and self-hash convention.

---

## `lock verify` — Lockfile Verification

### One-line promise

**Verify that a lockfile is untampered and that the artifacts it describes still exist on disk.**

`lock verify` closes the evidence loop: lockfiles created by `lock` become testable guarantees, not inert snapshots.

---

### Two verification levels

#### Level 1: Self-hash only (is the lockfile untampered?)

```bash
lock verify dec.lock.json
```

Re-derives `lock_hash` using the canonical serialization algorithm (see [Self-hash](#self-hash)). No filesystem access required. If the computed hash matches the stored `lock_hash`, the lockfile has not been modified since creation.

#### Level 2: Self-hash + member verification (do files on disk still match?)

```bash
lock verify dec.lock.json --root /data/dec
```

After passing Level 1, resolves each member's `path` against the `--root` directory and verifies:

1. **Existence** — the file exists at `<root>/<member.path>`
2. **Size** — `stat` size matches `member.size`
3. **Content hash** — re-reads the file and computes the hash using the algorithm prefix from the stored `bytes_hash` (e.g., `sha256:` → SHA256, `blake3:` → BLAKE3). Compares with the stored value.

If the self-hash fails (Level 1), member verification is skipped entirely — the lockfile data is untrustworthy.

---

### CLI

```bash
lock verify <LOCKFILE> [--root <DIR>] [--json] [--no-witness] [--strict]
```

#### Arguments

- `<LOCKFILE>`: path to the lockfile to verify (required, positional)

#### Flags

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--root <DIR>` | path | — | Root directory for member verification. Without this, only self-hash is checked. |
| `--json` | flag | `false` | Emit structured JSON output. Default is human-readable. |
| `--no-witness` | flag | `false` | Suppress witness ledger recording for this run. |
| `--strict` | flag | `false` | Promote `VERIFY_PARTIAL` to `VERIFY_FAILED` (exit 1). |

---

### Exit codes

| Code | Outcome | Condition |
|------|---------|-----------|
| `0` | `VERIFY_OK` | Self-hash valid AND (no `--root`, or all members verified) |
| `1` | `VERIFY_FAILED` | Self-hash invalid, or one or more members drifted/missing |
| `1` | `VERIFY_PARTIAL` | All checked members pass, but some couldn't be read (I/O errors). Only without `--strict`. |
| `2` | `REFUSAL` | Lockfile unreadable/malformed, root not found, unsupported version |

Note: `VERIFY_PARTIAL` and `VERIFY_FAILED` share exit code `1`. The `outcome` field in JSON output distinguishes them. `--strict` collapses `VERIFY_PARTIAL` into `VERIFY_FAILED`.

---

### Outcomes (exactly four)

#### 1. VERIFY_OK (exit 0)

Self-hash is valid. If `--root` was provided, all members exist on disk with matching size and content hash.

#### 2. VERIFY_FAILED (exit 1)

Either:
- Self-hash does not match (lockfile tampered), OR
- One or more members have drifted (hash mismatch, size mismatch) or are missing

When self-hash fails, `members` in the output is `null` — member data is untrustworthy, so verification is not attempted.

#### 3. VERIFY_PARTIAL (exit 1)

All members that could be read passed verification, but one or more members could not be read due to I/O errors (permission denied, locked file, etc.). The lockfile is not proven wrong, but not fully proven right either. With `--strict`, this becomes `VERIFY_FAILED`.

#### 4. REFUSAL (exit 2)

Verification was not attempted. The lockfile could not be read, parsed, or is structurally invalid.

---

### Streams

- `stdout`: verification result — human-readable (default) or JSON (`--json`). Refusal JSON envelope for exit 2.
- `stderr`: process-level diagnostics only.

---

### Output schema (JSON mode)

#### VERIFY_OK — self-hash only (no `--root`)

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_OK",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4e5f6...",
    "computed": "sha256:a1b2c3d4e5f6...",
    "valid": true
  },
  "members": null,
  "tool_versions": { "lock": "0.1.0" }
}
```

#### VERIFY_OK — with `--root` (all members pass)

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_OK",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4e5f6...",
    "computed": "sha256:a1b2c3d4e5f6...",
    "valid": true
  },
  "members": {
    "root": "/data/dec",
    "checked": 5,
    "verified": 5,
    "failed": 0,
    "skipped": 0,
    "failures": [],
    "skips": []
  },
  "tool_versions": { "lock": "0.1.0" }
}
```

#### VERIFY_FAILED — member drift

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_FAILED",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4e5f6...",
    "computed": "sha256:a1b2c3d4e5f6...",
    "valid": true
  },
  "members": {
    "root": "/data/dec",
    "checked": 5,
    "verified": 3,
    "failed": 2,
    "skipped": 0,
    "failures": [
      {
        "path": "tape.csv",
        "reason": "HASH_MISMATCH",
        "expected": "sha256:7d865e95...",
        "actual": "sha256:a3f19b02...",
        "expected_size": 847201,
        "actual_size": 851003
      },
      {
        "path": "draft.xlsx",
        "reason": "MISSING",
        "expected": "sha256:9d2e4f1a...",
        "actual": null,
        "expected_size": 12048,
        "actual_size": null
      }
    ],
    "skips": []
  },
  "tool_versions": { "lock": "0.1.0" }
}
```

#### VERIFY_FAILED — tampered lockfile (self-hash mismatch)

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_FAILED",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4e5f6...",
    "computed": "sha256:ff00112233...",
    "valid": false
  },
  "members": null,
  "tool_versions": { "lock": "0.1.0" }
}
```

When `lock_hash.valid` is `false`, `members` is always `null` — member data from a tampered lockfile cannot be trusted.

#### VERIFY_PARTIAL — I/O errors on some members

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_PARTIAL",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4e5f6...",
    "computed": "sha256:a1b2c3d4e5f6...",
    "valid": true
  },
  "members": {
    "root": "/data/dec",
    "checked": 5,
    "verified": 4,
    "failed": 0,
    "skipped": 1,
    "failures": [],
    "skips": [
      {
        "path": "archive.tar.gz",
        "reason": "IO_ERROR",
        "detail": "permission denied"
      }
    ]
  },
  "tool_versions": { "lock": "0.1.0" }
}
```

#### Output field reference

| Field | Type | Notes |
|-------|------|-------|
| `version` | string | `"lock-verify.v0"` — always present |
| `outcome` | string | `VERIFY_OK`, `VERIFY_FAILED`, `VERIFY_PARTIAL` |
| `lockfile` | string | Path to the lockfile as provided on CLI |
| `lock_hash` | object | Self-hash verification result |
| `lock_hash.stored` | string | `lock_hash` value from the lockfile |
| `lock_hash.computed` | string | Re-derived hash |
| `lock_hash.valid` | bool | Whether stored matches computed |
| `members` | object or null | `null` when no `--root` or when self-hash fails |
| `members.root` | string | Absolute path to root directory |
| `members.checked` | u64 | Total members in lockfile |
| `members.verified` | u64 | Members that passed verification |
| `members.failed` | u64 | Members with hash/size mismatch or missing |
| `members.skipped` | u64 | Members that could not be checked (I/O) |
| `members.failures` | array | Details of each failed member |
| `members.skips` | array | Details of each skipped member |
| `tool_versions` | object | `{ "lock": "<semver>" }` — lock's own version |

#### Failure entry

| Field | Type | Notes |
|-------|------|-------|
| `path` | string | Member path from lockfile |
| `reason` | string | `HASH_MISMATCH`, `SIZE_MISMATCH`, `MISSING` |
| `expected` | string or null | Expected `bytes_hash` from lockfile |
| `actual` | string or null | Computed hash from disk; `null` if missing |
| `expected_size` | u64 or null | Expected size from lockfile |
| `actual_size` | u64 or null | Actual size from disk; `null` if missing |

`SIZE_MISMATCH` is reported when size differs but hash was not recomputed (optimization: if size differs, hash will differ too). `HASH_MISMATCH` is reported when size matches but hash differs, or when both differ (hash is authoritative). `MISSING` is reported when the file does not exist at the resolved path.

#### Skip entry

| Field | Type | Notes |
|-------|------|-------|
| `path` | string | Member path from lockfile |
| `reason` | string | `IO_ERROR` |
| `detail` | string | OS-level error message |

---

### Human-readable output (default)

When `--json` is not specified, `lock verify` emits concise human-readable output to stdout.

#### VERIFY_OK (self-hash only)

```
✓ dec.lock.json — self-hash valid (sha256:a1b2c3d4...)
```

#### VERIFY_OK (with --root)

```
✓ dec.lock.json — self-hash valid, 5/5 members verified
  root: /data/dec
```

#### VERIFY_FAILED (tampered)

```
✗ dec.lock.json — TAMPERED
  stored:   sha256:a1b2c3d4e5f6...
  computed: sha256:ff00112233...
```

#### VERIFY_FAILED (member drift)

```
✗ dec.lock.json — self-hash valid, 2 of 5 members failed
  root: /data/dec
  HASH_MISMATCH  tape.csv       expected sha256:7d86... got sha256:a3f1...
  MISSING        draft.xlsx
```

#### VERIFY_PARTIAL

```
⚠ dec.lock.json — self-hash valid, 4/5 verified, 1 skipped
  root: /data/dec
  IO_ERROR  archive.tar.gz  permission denied
```

---

### Refusal codes

| Code | Trigger | Next step |
|------|---------|-----------|
| `E_IO` | Cannot read lockfile from disk | Check file path and permissions |
| `E_BAD_LOCKFILE` | Malformed JSON, missing required fields (`lock_hash`, `members`, `version`), absolute member paths, `..` path traversal | Regenerate the lockfile |
| `E_UNSUPPORTED_VERSION` | `version` field is not `lock.v0` | Use a compatible version of `lock` |
| `E_ROOT_NOT_FOUND` | `--root` path does not exist or is not a directory | Check the root path |
| `E_UNKNOWN_ALGORITHM` | Member `bytes_hash` has an unrecognized algorithm prefix (not `sha256` or `blake3`) | Regenerate the lockfile with a supported hash algorithm |

#### Refusal JSON envelope

```json
{
  "version": "lock-verify.v0",
  "outcome": "REFUSAL",
  "refusal": {
    "code": "E_BAD_LOCKFILE",
    "message": "lockfile missing required field: members",
    "detail": {
      "path": "dec.lock.json",
      "missing_fields": ["members"]
    },
    "next_command": "vacuum /data/dec | hash | lock --dataset-id \"dec\" > dec.lock.json"
  }
}
```

#### Refusal detail schemas

```
E_IO:
  { "path": "dec.lock.json", "error": "No such file or directory" }

E_BAD_LOCKFILE (parse error):
  { "path": "dec.lock.json", "error": "expected value at line 1 column 1" }

E_BAD_LOCKFILE (missing fields):
  { "path": "dec.lock.json", "missing_fields": ["members"] }

E_BAD_LOCKFILE (absolute paths):
  { "path": "dec.lock.json", "member_index": 2, "member_path": "/etc/passwd" }

E_BAD_LOCKFILE (path traversal):
  { "path": "dec.lock.json", "member_index": 1, "member_path": "../../etc/shadow" }

E_UNSUPPORTED_VERSION:
  { "path": "dec.lock.json", "version": "lock.v3" }

E_ROOT_NOT_FOUND:
  { "path": "/data/dec", "error": "No such file or directory" }

E_UNKNOWN_ALGORITHM:
  { "path": "dec.lock.json", "member_path": "tape.csv", "algorithm": "md5" }
```

---

### Execution flow

```
 1. Parse CLI (clap) → exit 2 on bad args
 2. If --describe / --schema / --version: handled by parent, not by verify
 3. Read lockfile from disk
    → on failure: refuse E_IO, exit 2
 4. Parse lockfile as JSON
    → on parse failure: refuse E_BAD_LOCKFILE, exit 2
 5. Validate lockfile structure:
    a. Must have "version" field → if missing or not "lock.v0": refuse E_UNSUPPORTED_VERSION or E_BAD_LOCKFILE
    b. Must have "lock_hash" field (string) → if missing: refuse E_BAD_LOCKFILE
    c. Must have "members" field (array) → if missing: refuse E_BAD_LOCKFILE
    d. Each member must have "path" (string), "bytes_hash" (string), "size" (u64)
    e. No member path may be absolute → if found: refuse E_BAD_LOCKFILE
    f. No member path may contain ".." → if found: refuse E_BAD_LOCKFILE
    g. Each member bytes_hash algorithm prefix must be recognized (sha256, blake3) → if not: refuse E_UNKNOWN_ALGORITHM
 6. Extract stored lock_hash
 7. Set lock_hash to "" and serialize to canonical JSON (same algorithm as lock creation)
 8. Compute SHA256 of canonical bytes
 9. Compare "sha256:<hex>" with stored lock_hash → record valid/invalid
10. If self-hash invalid: set outcome to VERIFY_FAILED, members to null, skip to step 14
11. If --root provided:
    a. Verify root exists and is a directory → if not: refuse E_ROOT_NOT_FOUND, exit 2
    b. For each member:
       i.   Resolve path: <root>/<member.path>
       ii.  Check file existence → MISSING if not found
       iii. Stat file for size → SIZE_MISMATCH if different (skip hash)
       iv.  Read file and compute hash using algorithm from stored bytes_hash
            → IO_ERROR if read fails (add to skips)
       v.   Compare computed hash with stored bytes_hash → HASH_MISMATCH if different
    c. Tally verified/failed/skipped counts
    d. Determine outcome:
       - All verified → VERIFY_OK
       - Any failed → VERIFY_FAILED
       - No failed but some skipped → VERIFY_PARTIAL (or VERIFY_FAILED if --strict)
12. If --root not provided: set members to null, outcome is VERIFY_OK (self-hash passed)
13. Build output (VerifyResult)
14. Emit output to stdout (JSON or human-readable)
15. Append witness record (unless --no-witness)
16. Exit with appropriate code
```

---

### Edge cases

#### Absolute member paths

Members in a lockfile should have relative paths (from `relative_path` in the pipeline). If a member `path` is absolute (starts with `/` or a Windows drive letter), `lock verify` refuses with `E_BAD_LOCKFILE`. This prevents unintended filesystem traversal.

#### Path traversal (`..` components)

If any member path contains `..` segments, `lock verify` refuses with `E_BAD_LOCKFILE`. Members must be confined within the root.

#### Empty members array

A lockfile with `members: []` and valid self-hash returns `VERIFY_OK` for Level 1. With `--root`, also `VERIFY_OK` — there's nothing to check.

#### Mixed hash algorithms

Different members may use different algorithms (e.g., some `sha256:`, others `blake3:`). `lock verify` uses the algorithm prefix from each member's `bytes_hash` individually. This matches how `hash` may be configured differently across pipeline runs.

#### Skipped entries in lockfile

The `skipped` array in the lockfile is not verified by `lock verify`. Skipped entries represent records that were excluded during lock creation — they have no `bytes_hash` to verify against.

#### Lockfile with `lock_hash: ""`

If `lock_hash` is an empty string, `lock verify` computes the expected hash and compares against `""`. This will always be a mismatch → `VERIFY_FAILED`.

#### Symlinks

Member paths that resolve to symlinks are followed. `lock verify` hashes the target file content. If the symlink is broken → `MISSING`.

#### Large files

`lock verify` reads files sequentially and hashes in a streaming fashion (not loading entire files into memory). No `--jobs` flag in v0 — parallel member hashing is deferred.

---

### Witness recording

`lock verify` records witness entries with the same ambient protocol as `lock`:

```json
{
  "id": "blake3:...",
  "tool": "lock",
  "version": "0.1.0",
  "binary_hash": "blake3:...",
  "inputs": [
    { "path": "dec.lock.json", "hash": "blake3:...", "bytes": 4821 }
  ],
  "params": { "subcommand": "verify", "root": "/data/dec", "strict": false },
  "outcome": "VERIFY_OK",
  "exit_code": 0,
  "output_hash": "blake3:...",
  "prev": "blake3:...",
  "ts": "2026-02-24T10:00:00Z"
}
```

- `tool`: always `"lock"` (the binary name)
- `params.subcommand`: `"verify"` — distinguishes from lock creation
- `params.root`: the `--root` value, or `null` if not provided
- `params.strict`: whether `--strict` was set
- `outcome`: `VERIFY_OK`, `VERIFY_FAILED`, `VERIFY_PARTIAL`, or `REFUSAL`
- `inputs[0]`: the lockfile path, with hash and size (lockfile can be pre-hashed since it's a file, not stdin)

Witness query subcommands (`lock witness query`, `lock witness last`, `lock witness count`) return verify records alongside lock creation records. Filter with `--outcome VERIFY_OK` etc.

---

### Relationship to `pack verify` and `--lock` on report tools

`lock verify` checks a lockfile's self-hash and optionally its members against the filesystem. It answers: **is this lockfile valid, and do the files it describes still match?**

`pack verify` (deferred) will check an evidence pack's integrity — including the lockfile bundled inside it, plus the pack's own manifest and signatures.

Report tools like `rvl` and `shape` may accept `--lock <lockfile>` to gate execution: refuse to run unless the lockfile verifies first. This makes lockfile verification a precondition for downstream analysis, not just a standalone check.

In a CI pipeline:

```bash
# Verify lockfile integrity before downstream processing
lock verify dec.lock.json --root /data/dec --strict
if [ $? -ne 0 ]; then
  echo "Lockfile verification failed — aborting pipeline"
  exit 1
fi

# Safe to proceed with analysis
shape dec.lock.json --lock dec.lock.json
rvl previous.lock.json dec.lock.json
```

---

### Core data structures

```rust
/// CLI arguments for verify subcommand
struct VerifyArgs {
    lockfile: PathBuf,                // positional: path to lockfile
    root: Option<PathBuf>,            // --root
    json: bool,                       // --json
    no_witness: bool,                 // --no-witness
    strict: bool,                     // --strict
}

/// Top-level verify result
struct VerifyResult {
    version: String,                  // "lock-verify.v0"
    outcome: String,                  // VERIFY_OK, VERIFY_FAILED, VERIFY_PARTIAL
    lockfile: String,                 // path as provided
    lock_hash: LockHashResult,
    members: Option<MembersResult>,   // null when no --root or self-hash failed
    tool_versions: BTreeMap<String, String>,
}

/// Self-hash verification
struct LockHashResult {
    stored: String,                   // lock_hash from file
    computed: String,                 // re-derived hash
    valid: bool,                      // stored == computed
}

/// Member verification results
struct MembersResult {
    root: String,                     // absolute root path
    checked: u64,                     // total members
    verified: u64,                    // passed verification
    failed: u64,                      // hash/size mismatch or missing
    skipped: u64,                     // I/O errors
    failures: Vec<MemberFailure>,
    skips: Vec<MemberSkip>,
}

/// A member that failed verification
struct MemberFailure {
    path: String,
    reason: String,                   // HASH_MISMATCH, SIZE_MISMATCH, MISSING
    expected: Option<String>,         // expected bytes_hash
    actual: Option<String>,           // actual computed hash
    expected_size: Option<u64>,
    actual_size: Option<u64>,
}

/// A member that could not be checked
struct MemberSkip {
    path: String,
    reason: String,                   // IO_ERROR
    detail: String,                   // OS error message
}
```

---

### Module structure addition

```
lock/
├── src/
│   ├── ...
│   ├── verify/
│   │   ├── mod.rs          # pub fn run_verify(args: VerifyArgs) → u8
│   │   ├── self_hash.rs    # Re-export or call lockfile::self_hash canonical serialization
│   │   └── members.rs      # Member-by-member filesystem verification
│   ...
```

`verify/self_hash.rs` must reuse the exact canonical serialization from `lockfile/self_hash.rs`. The serialization algorithm is shared — verify does not implement its own. Either:
- (a) Extract canonical serialization into a shared module used by both `lockfile/self_hash.rs` and `verify/self_hash.rs`, or
- (b) `verify/self_hash.rs` calls `lockfile::self_hash::canonical_serialize()` directly.

Option (b) is simpler for v0.

---

### Testing requirements

#### Unit tests

| Area | Tests |
|------|-------|
| Self-hash verification | Valid lockfile round-trip: create → verify → VERIFY_OK |
| Tampered lockfile | Modify any field after creation → VERIFY_FAILED |
| Missing `lock_hash` | Lockfile without `lock_hash` → E_BAD_LOCKFILE refusal |
| Missing `members` | Lockfile without `members` → E_BAD_LOCKFILE refusal |
| Unknown version | `"version": "lock.v99"` → E_UNSUPPORTED_VERSION refusal |
| Absolute member path | Member path `/etc/passwd` → E_BAD_LOCKFILE refusal |
| Path traversal | Member path `../../etc/shadow` → E_BAD_LOCKFILE refusal |
| Empty members | `members: []` with valid self-hash → VERIFY_OK |
| Unknown algorithm | Member `bytes_hash` with `md5:...` prefix → E_UNKNOWN_ALGORITHM refusal |

#### Integration tests

| Scenario | Assertion |
|----------|-----------|
| Create lockfile, verify Level 1 | `lock verify out.lock.json` → exit 0, VERIFY_OK |
| Create lockfile, verify Level 2 | `lock verify out.lock.json --root /data` → exit 0, VERIFY_OK, all members verified |
| Modify a member file, verify | → exit 1, VERIFY_FAILED, HASH_MISMATCH |
| Delete a member file, verify | → exit 1, VERIFY_FAILED, MISSING |
| Lockfile tampered (edit JSON), verify | → exit 1, VERIFY_FAILED, `lock_hash.valid: false`, `members: null` |
| Root not found | `--root /nonexistent` → exit 2, E_ROOT_NOT_FOUND |
| `--strict` with I/O error | → exit 1, VERIFY_FAILED (not VERIFY_PARTIAL) |
| `--json` vs default | JSON output parses as valid JSON; human output has ✓/✗/⚠ markers |

#### Witness tests

| Scenario | Assertion |
|----------|-----------|
| Default verify run | Witness record with `params.subcommand: "verify"` appended |
| `--no-witness` | No witness record written |
| Verify outcome in witness | `outcome` matches verify result (VERIFY_OK/VERIFY_FAILED/etc.) |
| Witness query finds verify records | `lock witness query --outcome VERIFY_OK` includes verify runs |

---

### v0.1 scope for verify

**Ship this:**
- Level 1: self-hash verification
- Level 2: member verification with `--root`
- Human-readable and `--json` output
- `--strict` flag
- Refusal codes: `E_IO`, `E_BAD_LOCKFILE`, `E_UNSUPPORTED_VERSION`, `E_ROOT_NOT_FOUND`, `E_UNKNOWN_ALGORITHM`
- Witness recording with `params.subcommand: "verify"`

**Defer:**
- Parallel member hashing (`--jobs`)
- Fingerprint `content_hash` re-verification
- `tool_versions` verification against installed binaries
- `profiles` entry verification
- Integration with `pack verify`
- `--output <FILE>` for writing results to a file
