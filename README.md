# lock

<div align="center">

[![CI](https://github.com/cmdrvl/lock/actions/workflows/ci.yml/badge.svg)](https://github.com/cmdrvl/lock/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub release](https://img.shields.io/github/v/release/cmdrvl/lock)](https://github.com/cmdrvl/lock/releases)

**Cargo.lock for data. One tamper-evident snapshot of every artifact, hash, and tool version in your pipeline.**

```bash
brew install cmdrvl/tap/lock
```

</div>

---

You scanned, hashed, and fingerprinted 47 files. Six months from now, someone asks: "what exactly was in that delivery?" You could point them at a directory listing. Or a spreadsheet. Or you could hand them a single file that answers the question with cryptographic certainty — and proves it hasn't been altered since the day it was created.

**lock pins the entire pipeline output into one self-hashed JSON lockfile.** Every artifact, its hash, its fingerprint match, and the exact tool versions that processed it. The lockfile's own `lock_hash` is the SHA-256 of its canonical contents — if the hash verifies, the file is exactly what was produced. If anything was modified, even whitespace, the hash breaks.

### What makes this different

- **Self-hashed by construction** — `lock_hash` is computed over canonical JSON with sorted keys. Any edit to the lockfile invalidates it. No external signature needed.
- **Two-level verification** — `lock verify` checks the self-hash (was this file tampered with?). Add `--root /data/` to also verify that every member file on disk still matches.
- **Skipped records are first-class** — files that couldn't be processed aren't silently dropped; they're captured in the `skipped` array with reasons, and the exit code tells you (exit 1 = LOCK_PARTIAL).
- **Tool provenance built in** — the lockfile records which version of vacuum, hash, fingerprint, and lock produced it. Reproducibility is traceable.

---

## Quick Example

```bash
$ vacuum /data/dec | hash | lock --dataset-id "raw-dec" > raw.lock.json
```

```json
{
  "version": "lock.v0",
  "dataset_id": "raw-dec",
  "as_of": null,
  "created": "2026-01-15T10:30:00Z",
  "lock_hash": "sha256:a1b2c3d4e5f6...",
  "member_count": 2,
  "members": [
    {
      "path": "model.xlsx",
      "size": 2481920,
      "bytes_hash": "sha256:e3b0c442...",
      "fingerprint": null
    },
    {
      "path": "tape.csv",
      "size": 847201,
      "bytes_hash": "sha256:7d865e95...",
      "fingerprint": null
    }
  ],
  "skipped": [],
  "skipped_count": 0,
  "tool_versions": {
    "vacuum": "0.1.0",
    "hash": "0.1.0",
    "lock": "0.1.0"
  },
  "note": null,
  "profiles": []
}
```

Two artifacts pinned, self-hashed, tool versions recorded. Hand this to an auditor, an agent, or CI.

```bash
# With fingerprinting:
$ vacuum /data/models | hash | fingerprint --fp argus-model.v1 \
    | lock --dataset-id "argus-models-2025-12" --as-of "2025-12-31" \
    > models.lock.json

# With annotation:
$ lock --dataset-id "q4-final" --note "Final delivery after restatement" \
    < fingerprinted.jsonl > q4.lock.json

# Full pipeline into evidence pack:
$ vacuum /data/dec | hash | fingerprint --fp csv.v0 \
    | lock --dataset-id "dec" > dec.lock.json
  pack seal dec.lock.json --output evidence/dec/
```

---

## Where lock Fits

`lock` is the **artifact tool** at the end of the stream pipeline.

```
vacuum  →  hash  →  fingerprint  →  lock  →  pack
(scan)    (hash)    (template)     (pin)    (seal)
```

Each tool in the pipeline reads JSONL from stdin and emits enriched JSONL to stdout. `lock` consumes the stream and produces a single JSON lockfile.

---

## What lock Is Not

`lock` does not replace upstream tools.

| If you need... | Use |
|----------------|-----|
| Enumerate files in a directory | [`vacuum`](https://github.com/cmdrvl/vacuum) |
| Compute SHA256/BLAKE3 hashes | [`hash`](https://github.com/cmdrvl/hash) |
| Match files against template definitions | [`fingerprint`](https://github.com/cmdrvl/fingerprint) |
| Check structural comparability of CSVs | [`shape`](https://github.com/cmdrvl/shape) |
| Explain numeric changes between CSVs | [`rvl`](https://github.com/cmdrvl/rvl) |
| Bundle into immutable evidence packs | [`pack`](https://github.com/cmdrvl/pack) |

`lock` only answers: **what exact set of artifacts, hashes, fingerprints, and tool versions did this run produce?**

---

## The Three Outcomes

`lock` emits exactly one domain outcome.

### 1. LOCK_CREATED (exit `0`)

All input records became members. `skipped` is empty. The lockfile is complete.

### 2. LOCK_PARTIAL (exit `1`)

Lockfile created, but at least one input record had `_skipped: true` and was excluded from `members`. The lockfile is valid and self-hashed, but incomplete — `exit 1` forces explicit handling in automation.

### 3. REFUSAL (exit `2`)

No lockfile created. Input was invalid or insufficient.

```json
{
  "version": "lock.v0",
  "outcome": "REFUSAL",
  "refusal": {
    "code": "E_MISSING_HASH",
    "message": "3 records lack bytes_hash - run hash first",
    "detail": {
      "count": 3,
      "sample_paths": ["data/model.xlsx", "data/tape.csv", "data/readme.pdf"]
    },
    "next_command": "vacuum /data/ | hash | lock --dataset-id \"my-dataset\""
  }
}
```

Refusals always include a concrete `next_command` — never a dead end.

---

## How lock Compares

| Capability | lock | Cargo.lock / package-lock.json | Ad-hoc manifest script | Manual checksums |
|------------|------|-------------------------------|------------------------|-----------------|
| Self-hashed (tamper-evident) | ✅ SHA256 of canonical JSON | ❌ | ❌ | ❌ |
| Skipped record tracking | ✅ With reasons | ❌ | ⚠️ You write it | ❌ |
| Tool version provenance | ✅ From upstream pipeline | ❌ | ⚠️ You write it | ❌ |
| Deterministic output | ✅ Sorted members, canonical JSON | ✅ | ⚠️ Depends | ❌ |
| Stream pipeline native | ✅ stdin JSONL | ❌ | ⚠️ You write it | ❌ |
| Audit trail (witness ledger) | ✅ Built-in | ❌ | ❌ | ❌ |

**When to use lock:**
- End of a data pipeline — pin what was produced before handing off to consumers
- Audit and compliance — prove exactly what artifacts existed and which tools processed them
- CI gate — verify lockfile integrity before downstream processing

**When lock might not be ideal:**
- You need to compare data content — use `rvl` or `shape`
- You need to sign lockfiles cryptographically — signing layer is deferred in v0
- You need to diff two lockfiles — lock-to-lock diff is deferred in v0

---

## Self-Hash and Tamper Evidence

`lock_hash` makes every lockfile tamper-evident by construction.

**Algorithm:**
1. Build full lock object with `lock_hash: ""`
2. Canonical-serialize (sorted keys, compact JSON, no trailing newline)
3. SHA256 those bytes
4. Set `lock_hash` to `sha256:<hex>`
5. Emit final JSON

**Verification** repeats the same process and compares computed hash with stored `lock_hash`. If they don't match, the lockfile has been tampered with.

---

## Installation

### Homebrew (Recommended)

```bash
brew install cmdrvl/tap/lock
```

### Shell Script

```bash
curl -fsSL https://raw.githubusercontent.com/cmdrvl/lock/main/scripts/install.sh | bash
```

### From Source

```bash
cargo build --release
./target/release/lock --help
```

---

## CLI Reference

```bash
lock [<INPUT>] [OPTIONS]
lock witness <query|last|count> [OPTIONS]
```

### Arguments

- `[INPUT]`: JSONL manifest file. Defaults to stdin.

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--dataset-id <ID>` | string | `null` | Logical dataset identifier |
| `--as-of <TIMESTAMP>` | string | `null` | Annotation timestamp (ISO 8601) |
| `--note <TEXT>` | string | `null` | Free-text annotation |
| `--no-witness` | flag | `false` | Suppress witness ledger recording for this run |
| `--describe` | flag | `false` | Print compiled `operator.json` to stdout, exit `0` |
| `--schema` | flag | `false` | Print lock JSON schema, exit `0` |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | LOCK_CREATED (all records became members) |
| `1` | LOCK_PARTIAL (some records skipped) |
| `2` | REFUSAL or CLI error |

### Streams

- `stdout`: lockfile JSON (exit 0/1) or refusal JSON envelope (exit 2)
- `stderr`: process diagnostics only

---

## Input Contract

`lock` consumes newline-delimited JSON records from upstream stream tools.

**Required fields** (non-skipped records):
- `version` — upstream record version (`vacuum.v0`, `hash.v0`, `fingerprint.v0`)
- `path` — file path
- `bytes_hash` — content hash from `hash`
- `size` — file size in bytes

**Optional passthrough fields:** `fingerprint`, `mime_guess`, `mtime`, `relative_path`, and others from upstream.

### `_skipped` behavior

If a record has `_skipped: true`:
- It is excluded from `members`
- It enters `skipped` with path + warnings
- It contributes to `skipped_count`
- It causes `LOCK_PARTIAL` (exit `1`) if any skipped records exist

If a non-skipped record lacks `bytes_hash`, `lock` refuses with `E_MISSING_HASH`.

---

## Refusal Codes

| Code | Trigger | Next Step |
|------|---------|-----------|
| `E_EMPTY` | No input records | Provide artifacts (run upstream pipeline) |
| `E_BAD_INPUT` | Malformed JSONL or unknown record version | Fix upstream output |
| `E_MISSING_HASH` | Non-skipped records missing `bytes_hash` | Run `hash` before `lock` |

Every refusal includes the error code, detail, and a concrete `next_command`.

---

## Troubleshooting

### "E_MISSING_HASH" — forgot to run hash

The most common error. `lock` requires every non-skipped record to have a `bytes_hash`. You probably piped `vacuum` directly to `lock` without `hash` in between:

```bash
# Wrong:
vacuum /data | lock --dataset-id "nightly"

# Right:
vacuum /data | hash | lock --dataset-id "nightly"
```

### "E_BAD_INPUT" — unknown record version

Your upstream tool emitted records with a version `lock` doesn't recognize. Check that all pipeline tools are on compatible versions:

```bash
vacuum --version
hash --version
lock --version
```

### "E_EMPTY" — no input records

The upstream pipeline produced no output. Check that the directory you're scanning actually contains files:

```bash
vacuum /data/dec | wc -l  # should be > 0
```

### LOCK_PARTIAL but you expected LOCK_CREATED

Some records had `_skipped: true` from upstream (e.g., `fingerprint` couldn't match a template). Check the `skipped` array in the lockfile to see which files and why:

```bash
jq '.skipped[] | "\(.path): \(.warnings)"' nightly.lock.json
```

### lock_hash doesn't verify

The lockfile was modified after creation. Regenerate it from the same inputs, or investigate what changed. Any edit — even whitespace — breaks the self-hash.

---

## Agent / CI Integration

### Self-describing contract

```bash
$ lock --describe | jq '.exit_codes'
{
  "0": { "meaning": "LOCK_CREATED" },
  "1": { "meaning": "LOCK_PARTIAL" },
  "2": { "meaning": "REFUSAL" }
}

$ lock --describe | jq '.pipeline'
{
  "upstream": ["vacuum", "hash", "fingerprint"],
  "downstream": ["pack", "shape", "rvl"]
}
```

### Agent workflow: pipeline → lock → verify

```bash
# 1. Produce lockfile
vacuum /data/dec | hash | fingerprint --fp csv.v0 \
  | lock --dataset-id "dec-nightly" > dec.lock.json

case $? in
  0) echo "complete lock" ;;
  1) echo "partial lock — check skipped records"
     jq '.skipped_count' dec.lock.json ;;
  2) echo "refusal"
     jq '.refusal' dec.lock.json
     exit 1 ;;
esac

# 2. Verify integrity later
stored_hash=$(jq -r '.lock_hash' dec.lock.json)
# Agent recomputes hash and compares
```

### What makes this agent-friendly

- **Exit codes** — `0`/`1`/`2` map to complete/partial/error branching
- **Structured JSON only** — no human-mode output to parse; stdout is always JSON
- **Refusals have `next_command`** — an agent can read and retry with the suggested fix
- **`--describe`** — prints `operator.json` so an agent discovers the tool without reading docs
- **`--schema`** — prints the lockfile JSON schema for programmatic validation

---

<details>
<summary><strong>Witness Subcommands</strong></summary>

`lock` records every run to an ambient witness ledger. You can query this ledger:

```bash
# Query by tool, date range, or outcome
lock witness query --tool lock --since 2026-01-01 --outcome LOCK_CREATED --json

# Get the most recent run
lock witness last --json

# Count runs matching a filter
lock witness count --since 2026-02-01
```

### Subcommand Reference

```bash
lock witness query [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] \
  [--limit <n>] [--json]

lock witness last [--json]

lock witness count [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] [--json]
```

### Exit Codes (witness subcommands)

| Code | Meaning |
|------|---------|
| `0` | One or more matching records returned |
| `1` | No matches (or empty ledger for `last`) |
| `2` | CLI parse error or witness internal error |

### Ledger Location

- Default: `~/.epistemic/witness.jsonl`
- Override: set `EPISTEMIC_WITNESS` environment variable
- Malformed ledger lines are skipped; valid lines continue to be processed.

</details>

---

## Verify

`lock verify` checks whether a lockfile is untampered and whether the files it describes still match what's on disk.

### Quick Examples

```bash
# Level 1: Is this lockfile untampered?
$ lock verify dec.lock.json
✓ dec.lock.json — self-hash valid (sha256:a1b2c3d4...)

# Level 2: Do the files on disk still match?
$ lock verify dec.lock.json --root /data/dec
✓ dec.lock.json — self-hash valid, 5/5 members verified
  root: /data/dec

# JSON output for CI/agents
$ lock verify dec.lock.json --root /data/dec --json
```

### What Gets Checked

| Level | Flag | Checks |
|-------|------|--------|
| **1** | (default) | Re-derives `lock_hash` using canonical serialization. No filesystem access. |
| **2** | `--root <DIR>` | Level 1 + resolves each member path, checks file existence, size, and content hash. |

If the self-hash fails, member verification is skipped — the lockfile data is untrustworthy.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | `VERIFY_OK` — self-hash valid, members verified (or no `--root`) |
| `1` | `VERIFY_FAILED` — tampered or members drifted; or `VERIFY_PARTIAL` — some members unreadable |
| `2` | `REFUSAL` — lockfile unreadable, malformed, or root not found |

### Output Examples

#### VERIFY_OK

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

#### VERIFY_FAILED (member drift)

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_FAILED",
  "lockfile": "dec.lock.json",
  "lock_hash": { "stored": "sha256:a1b2...", "computed": "sha256:a1b2...", "valid": true },
  "members": {
    "root": "/data/dec",
    "checked": 3,
    "verified": 1,
    "failed": 2,
    "skipped": 0,
    "failures": [
      { "path": "tape.csv", "reason": "HASH_MISMATCH", "expected": "sha256:7d86...", "actual": "sha256:a3f1...", "expected_size": 847201, "actual_size": 851003 },
      { "path": "draft.xlsx", "reason": "MISSING", "expected": "sha256:9d2e...", "actual": null, "expected_size": 12048, "actual_size": null }
    ],
    "skips": []
  },
  "tool_versions": { "lock": "0.1.0" }
}
```

#### TAMPERED (self-hash mismatch)

```json
{
  "version": "lock-verify.v0",
  "outcome": "VERIFY_FAILED",
  "lockfile": "dec.lock.json",
  "lock_hash": {
    "stored": "sha256:a1b2c3d4...",
    "computed": "sha256:ff001122...",
    "valid": false
  },
  "members": null,
  "tool_versions": { "lock": "0.1.0" }
}
```

When `lock_hash.valid` is `false`, `members` is always `null` — member data from a tampered lockfile cannot be trusted.

### CLI Reference

```bash
lock verify <LOCKFILE> [--root <DIR>] [--json] [--no-witness] [--strict]
```

| Flag | Description |
|------|-------------|
| `--root <DIR>` | Enable member verification against this directory |
| `--json` | Structured JSON output (default is human-readable) |
| `--strict` | Promote `VERIFY_PARTIAL` → `VERIFY_FAILED` |
| `--no-witness` | Suppress witness ledger recording |

### Pipeline Integration

#### CI gate

```bash
# Verify before downstream processing
lock verify dec.lock.json --root /data/dec --strict --json
if [ $? -ne 0 ]; then
  echo "Lockfile verification failed"
  exit 1
fi
```

#### Audit workflow

```bash
# Received a lockfile from a vendor — is it intact?
lock verify vendor-delivery.lock.json

# Re-verify against the data directory
lock verify vendor-delivery.lock.json --root /mnt/vendor/2026-q1
```

#### SLA enforcement

```bash
# Nightly check: does the locked dataset still exist?
lock verify production.lock.json --root /data/production --strict --json \
  | jq -e '.outcome == "VERIFY_OK"' || alert "Production data drift detected"
```

---

## Limitations

| Limitation | Detail |
|------------|--------|
| **No lock-to-lock diff** | Can't compare two lockfiles for changes yet — deferred in v0 |
| **No signing** | No GPG/Sigstore integration yet — self-hash provides tamper evidence but not identity |
| **No strict mode** | Can't refuse on any skipped record — `LOCK_PARTIAL` is the only signal |
| **No profile population** | `profiles` field exists but is always empty in v0 |
| **In-memory** | All input records are collected before emitting the lockfile |

---

## FAQ

### Why "lock"?

Same concept as `Cargo.lock` or `package-lock.json` — it pins the exact state of a dataset so you can reproduce or verify it later.

### Why is LOCK_PARTIAL exit `1` instead of a refusal?

Because the lockfile is still valid and self-hashed. It's incomplete, not wrong. `exit 1` forces automation to handle it explicitly rather than silently accepting an incomplete snapshot.

### Why include tool_versions from skipped records?

They still reflect pipeline execution provenance. Excluding them would lose traceability about what tools ran, even if some artifacts couldn't be processed.

### Is lock_hash the same as witness output_hash?

No.
- `lock_hash`: SHA256 of canonical pre-hash lock JSON (self-integrity)
- `output_hash`: BLAKE3 of emitted stdout bytes in witness record (run-level evidence chain)

### Can I add metadata after the fact?

No. Any modification breaks `lock_hash`. If you need to annotate, regenerate the lockfile with `--note` or `--as-of`.

### How do I verify a lockfile?

```bash
# Self-hash only (was it tampered?)
lock verify dec.lock.json

# Self-hash + member content (do files still match?)
lock verify dec.lock.json --root /data/dec
```

Level 1 needs no filesystem access — it re-derives the self-hash. Level 2 additionally checks each member file on disk.

### What's the difference between `lock verify` and `pack verify`?

`lock verify` checks a lockfile's self-hash and optionally its members against the filesystem. It answers: *is this lockfile valid, and do the files it describes still match?*

`pack verify` (future) will check an evidence pack's integrity — including the lockfile bundled inside, plus the pack's own manifest and signatures. Lock verify is for the data layer; pack verify is for the evidence layer.

### How does lock relate to pack?

`lock` pins artifacts into a lockfile. [`pack`](https://github.com/cmdrvl/pack) bundles lockfiles, reports, and tool versions into immutable evidence packs. Lock is the input; pack is the seal.

---

## Spec and Development

The full specification is [`docs/PLAN.md`](./docs/PLAN.md). This README covers intended v0 behavior; the spec adds implementation details, edge-case definitions, and testing requirements.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
