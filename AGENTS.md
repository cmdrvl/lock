# AGENTS.md — lock

> Guidelines for AI coding agents working in this Rust codebase.

---

## lock — What This Project Does

`lock` creates a single self-hashed dataset lockfile from stream pipeline JSONL records. It is the **artifact tool** at the end of the pipeline:

```
vacuum → hash → fingerprint → lock → pack
```

### Quick Reference

```bash
# Core pipeline
vacuum /data/dec | hash | lock --dataset-id "raw-dec" > raw.lock.json

# With fingerprinting
vacuum /data | hash | fingerprint --fp csv.v0 \
  | lock --dataset-id "dec" > dec.lock.json

# Quality gate
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

### Source of Truth

- **Spec:** [`docs/PLAN.md`](./docs/PLAN.md) — all behavior must follow this document
- Do not invent behavior not present in the plan

### Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | CLI entry + exit code mapping |
| `src/lib.rs` | Orchestration flow |
| `src/cli/` | Argument parsing and witness subcommands |
| `src/input/` | JSONL parsing and field extraction |
| `src/lockfile/` | Member/skipped construction + sorting |
| `src/lockfile/self_hash.rs` | Canonical serialization + SHA256 |
| `src/refusal/` | Refusal envelope, codes, details |
| `src/verify/` | Lockfile verification (self-hash + member content) |
| `src/witness/` | Witness append/query behavior |
| `operator.json` | Machine-readable operator contract |

---

## Output Contract (Critical)

`lock` is an **artifact tool**, not a report tool:

- **Always** emits structured JSON to stdout — no human-report mode
- Domain outcomes are represented by exit code and JSON envelope
- On refusal, emit refusal JSON envelope with `code`/`detail`/`next_command`, not ad-hoc text

| Exit | Meaning |
|------|---------|
| `0` | `LOCK_CREATED` — all records became members |
| `1` | `LOCK_PARTIAL` — lockfile created but some records skipped |
| `2` | `REFUSAL` — no lockfile created |

---

## Core Invariants (Do Not Break)

### 1. Self-hash integrity

`lock_hash` must be computed exactly as specified:
1. Build lock JSON with `lock_hash = ""`
2. Canonical serialize (sorted keys, compact JSON, no trailing newline)
3. SHA256 those bytes
4. Set `lock_hash = "sha256:<hex>"`
5. Emit final JSON

Any change to canonicalization, key ordering, or byte serialization is a **breaking change**.

### 2. Deterministic ordering

- `members` sorted by `path` (lexicographic byte-order)
- `skipped` sorted by `path`
- Same logical input + fixed timestamp = identical output

### 3. `_skipped` protocol

- `_skipped: true` records do **not** enter `members`
- They **must** enter `skipped`
- Any skipped records → `LOCK_PARTIAL` (exit `1`)

### 4. Missing hash refusal

Non-skipped records missing `bytes_hash` must refuse with `E_MISSING_HASH`.

### 5. Version gate

Reject unknown/missing upstream `version` with `E_BAD_INPUT`.

### 6. Witness parity

Ambient witness semantics must match spine conventions (`shape`/`rvl` parity):
- Append by default
- `--no-witness` opt-out
- Witness failures do not mutate domain outcome semantics

### 7. Verify reuses existing self-hash functions

`lock verify` Level 1 calls `lockfile::self_hash::verify_lock_hash_from_json()` — the function already exists and performs the full self-hash check. Verify does not implement its own serialization or hashing. A divergence would produce false tamper detection. No `verify/self_hash.rs` file needed.

---

## Toolchain

- **Language:** Rust, Cargo only
- **Edition:** 2024 (or `rust-toolchain.toml` when present)
- **Unsafe code:** forbidden (`#![forbid(unsafe_code)]`)
- **Dependencies:** explicit versions, small and pinned

Release profile:

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

---

## Quality Gate

Run after any substantive change:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Test Coverage Areas

- JSONL parse and refusal paths
- `_skipped` handling and partial outcome
- Missing hash refusal
- Deterministic sorting
- Self-hash verification round-trip
- `tool_versions` merge behavior
- Witness append/query paths
- E2E spine compatibility (`vacuum → hash → fingerprint → lock`)

---

## Git and Release

- **Primary branch:** `main`
- **`master`** exists for legacy URL compatibility — keep synced: `git push origin main:master`
- Bump `Cargo.toml` semver appropriately on release
- Sync `Cargo.lock` before release workflows that use `--locked`

---

## Editing Rules

- **No file deletion** without explicit written user permission
- **No destructive git commands** (`reset --hard`, `clean -fd`, `rm -rf`, force push) without explicit authorization
- **No scripted mass edits** — make intentional, reviewable changes
- **No file proliferation** — edit existing files; new files for genuinely new functionality only
- **No surprise behavior** — do not invent behavior not in `docs/PLAN.md`
- **No backwards-compatibility shims** — fix the code directly

---

## RULE 0

If the user gives a direct instruction, follow it even if it conflicts with defaults in this file.

---

## Beads (`br`) Workflow

Use Beads as source of truth for task state.

```bash
br ready              # Show unblocked ready work
br list --status=open # All open issues
br show <id>          # Full issue details
br update <id> --status=in_progress
br close <id> --reason "Completed"
br sync --flush-only  # Export to JSONL (no git ops)
```

Pick unblocked beads. Mark in-progress before coding. Close with evidence when done.

---

## Agent Mail (Multi-Agent Sessions)

When Agent Mail is available:

- Register identity in this project
- Reserve only specific files you are actively editing — never entire directories
- Send start/finish updates per bead
- Poll inbox at moderate cadence (2-5 minutes)
- Acknowledge `ack_required` messages promptly
- Release reservations when done

---

## Session Completion

Before ending a session:

1. Run quality gate (`fmt` + `clippy` + `test`)
2. Confirm docs/spec alignment for behavior changes
3. Commit with precise message
4. Push `main` and sync `master`
5. Summarize: what changed, what was validated, remaining risks
