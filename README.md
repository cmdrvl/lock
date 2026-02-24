# lock

**Pin every artifact, fingerprint, and tool version for a dataset into a single immutable, self-hashed lockfile.**

`lock` is the spine's dataset lockfile tool: the final, tamper-evident artifact you can hand to humans, agents, CI, and auditors.

---

## TL;DR

**The problem:** after `vacuum | hash | fingerprint`, teams still rely on ad-hoc manifests, scattered checksums, and uncertain tool provenance.

**The solution:** one deterministic JSON artifact (`lock.v0`) that captures:
- exactly which artifacts were included,
- which records were skipped and why,
- which tool versions produced the result,
- a self-hash (`lock_hash`) to detect tampering.

If `lock_hash` verifies, the lockfile is exactly what was produced.

---

## Status

`lock` is currently spec-first in this repo.

- Source of truth: [`docs/PLAN.md`](./docs/PLAN.md)
- This README documents the intended v0 behavior from that plan.

---

## Where lock Fits

`lock` is the **artifact tool** at the end of the stream pipeline.

```bash
vacuum /data/dec | hash | lock --dataset-id "raw-dec" > raw.lock.json
```

With fingerprinting:

```bash
vacuum /data/models | hash | fingerprint --fp argus-model.v1 \
  | lock --dataset-id "argus-models-2025-12" --as-of "2025-12-31" \
  > models.lock.json
```

With annotation:

```bash
lock --dataset-id "q4-final" --note "Final delivery after restatement" \
  < fingerprinted.jsonl > q4.lock.json
```

Then seal with evidence tooling:

```bash
vacuum /data/dec | hash | fingerprint --fp csv.v0 \
  | lock --dataset-id "dec" > dec.lock.json
pack seal dec.lock.json --output evidence/dec/
```

---

## What lock Is Not

`lock` does not replace upstream tools.

- Not a scanner: use `vacuum`
- Not a hasher: use `hash`
- Not a template recognizer: use `fingerprint`
- Not an evidence packager: use `pack`
- Not a comparability gate: use `shape`

`lock` only answers: **what exact set of artifacts/hashes/fingerprints/tool-versions did this run produce?**

---

## The Three Outcomes

`lock` emits exactly one domain outcome.

### 1. `LOCK_CREATED` (exit `0`)
All input records became members. `skipped` is empty.

### 2. `LOCK_PARTIAL` (exit `1`)
Lockfile created, but at least one input record had `_skipped: true` and was excluded from `members`.

### 3. `REFUSAL` (exit `2`)
No lockfile created due to invalid/insufficient input.

---

## Quick Example Output

Representative `LOCK_CREATED` lockfile:

```json
{
  "as_of": "2025-12-31T23:59:59Z",
  "created": "2026-01-15T10:30:00Z",
  "dataset_id": "argus-models-2025-12",
  "lock_hash": "sha256:a1b2c3d4e5f6...",
  "member_count": 2,
  "members": [
    {
      "bytes_hash": "sha256:e3b0c442...",
      "fingerprint": {
        "content_hash": "blake3:9f2a...",
        "fingerprint_id": "argus-model.v1",
        "fingerprint_version": "0.3.2",
        "matched": true
      },
      "path": "model.xlsx",
      "size": 2481920
    },
    {
      "bytes_hash": "sha256:7d865e95...",
      "fingerprint": null,
      "path": "tape.csv",
      "size": 847201
    }
  ],
  "note": null,
  "profiles": [],
  "skipped": [],
  "skipped_count": 0,
  "tool_versions": {
    "fingerprint": "0.1.0",
    "hash": "0.1.0",
    "lock": "0.1.0",
    "vacuum": "0.1.0"
  },
  "version": "lock.v0"
}
```

---

## CLI Reference (Planned v0)

```bash
lock [<INPUT>] [OPTIONS]
lock witness <query|last|count> [OPTIONS]
```

### Arguments
- `[INPUT]`: JSONL manifest file. Defaults to stdin.

### Options
- `--dataset-id <ID>`: logical dataset identifier (`null` when omitted)
- `--as-of <TIMESTAMP>`: annotation timestamp (`null` when omitted)
- `--note <TEXT>`: free-text annotation (`null` when omitted)
- `--no-witness`: disable witness record append for this run
- `--describe`: print compiled `operator.json`, exit `0`
- `--schema`: print lock JSON schema, exit `0`
- `--version`: print `lock <semver>`, exit `0`

### Streams
- `stdout`: lockfile JSON (`exit 0/1`) or refusal JSON envelope (`exit 2`)
- `stderr`: process diagnostics only

---

## Input Contract

`lock` consumes newline-delimited JSON records from upstream stream tools.

Expected non-skipped fields:
- `version`
- `path`
- `relative_path`
- `bytes_hash`
- `size`
- `tool_versions`

Optional passthrough fields include `fingerprint`, `mime_guess`, `mtime`, and others from upstream.

### `_skipped` behavior

If a record has `_skipped: true`:
- it is excluded from `members`,
- entered into `skipped` with path + warnings,
- contributes to `skipped_count`,
- causes `LOCK_PARTIAL` (`exit 1`) if any skipped records exist.

If a non-skipped record lacks `bytes_hash`, `lock` refuses with `E_MISSING_HASH`.

### Version compatibility

Planned accepted record versions:
- `vacuum.v0`
- `hash.v0`
- `fingerprint.v0`

Unknown/missing versions trigger `E_BAD_INPUT`.

---

## Refusal Codes

| Code | Trigger | Next Step |
|---|---|---|
| `E_EMPTY` | no input records | provide artifacts (run upstream) |
| `E_BAD_INPUT` | malformed JSONL or unknown record version | fix upstream output |
| `E_MISSING_HASH` | non-skipped records missing `bytes_hash` | run `hash` before `lock` |

Example refusal envelope:

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

---

## Self-Hash and Tamper Evidence

`lock_hash` is computed by hashing the canonical lock object with `lock_hash` temporarily blank.

Algorithm:
1. Build full lock object with `lock_hash: ""`.
2. Canonical-serialize (sorted keys, compact JSON, no trailing newline).
3. SHA256 those bytes.
4. Set `lock_hash` to `sha256:<hex>`.
5. Emit final JSON.

Verification repeats the same process and compares computed hash with the stored `lock_hash`.

---

## Witness Ledger

`lock` follows spine witness conventions:
- default append on every run (success or refusal),
- `--no-witness` opt-out,
- ledger path: `EPISTEMIC_WITNESS` or `~/.epistemic/witness.jsonl`,
- witness failures do not alter domain outcome semantics.

Query interface:

```bash
lock witness query [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] \
  [--limit <n>] [--json]

lock witness last [--json]

lock witness count [--tool <name>] [--since <iso8601>] [--until <iso8601>] \
  [--outcome <LOCK_CREATED|LOCK_PARTIAL|REFUSAL>] [--input-hash <substring>] [--json]
```

---

## Agent / CI Integration

### Shell-friendly gating

```bash
lock manifest.jsonl --dataset-id nightly >/tmp/nightly.lock.json
case $? in
  0) echo "complete lock" ;;
  1) echo "partial lock (skipped present)" ;;
  2) echo "refusal" ;;
esac
```

### Parse outcomes safely

```bash
jq -r '.outcome // "LOCK_CREATED_OR_PARTIAL"' /tmp/nightly.lock.json
jq '.skipped_count, .member_count' /tmp/nightly.lock.json
```

### Verify lock integrity

- Load JSON
- Replace `lock_hash` with `""`
- Canonical-serialize
- SHA256
- Compare to stored `lock_hash`

Agents should treat hash mismatch as integrity failure, not a warning.

---

## Scope

### v0.1 target
- JSONL input from stdin/file
- metadata flags: `dataset_id`, `as_of`, `note`
- `_skipped` support + partial outcome
- self-hash
- tool version accumulation
- refusal envelope + concrete next command
- witness parity + witness query subcommands
- `--describe`, `--schema`, `--version`

### Deferred
- profile population
- strict mode (`refuse on any skipped`)
- lock-to-lock diff tools
- witness fabric sync
- signing layer integration (GPG/Sigstore)

---

## FAQ

### Why `LOCK_PARTIAL` as exit `1` instead of refusal?
Because the produced lockfile is still valid and self-hashed, but incomplete by design. `exit 1` forces explicit handling in automation.

### Why include `tool_versions` from skipped records?
They still reflect pipeline execution provenance; excluding them would lose important traceability.

### Is `lock_hash` the same as witness `output_hash`?
No.
- `lock_hash`: SHA256 of canonical pre-hash lock JSON (self-integrity).
- `output_hash`: BLAKE3 of emitted stdout bytes in witness record (run-level evidence chain).

---

## Spec and Development

- Spec: [`docs/PLAN.md`](./docs/PLAN.md)
- This README should stay aligned with the plan as implementation lands.

When Rust source is added, standard checks should be:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
