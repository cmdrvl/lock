# AGENTS.md - lock

> Guidelines for AI coding agents working in this Rust codebase.

---

## RULE 0 - THE FUNDAMENTAL OVERRIDE PREROGATIVE

If the user gives a direct instruction, follow it even if it conflicts with defaults in this file.

---

## RULE NUMBER 1: NO FILE DELETION

**Never delete files or folders without explicit written user permission.**

This includes files you created during the session.

---

## Irreversible Git & Filesystem Actions - DO NOT BREAK GLASS

1. Forbidden without explicit user authorization in the same message: `git reset --hard`, `git clean -fd`, `rm -rf`, force pushes, or any destructive overwrite.
2. If command impact is ambiguous, stop and ask.
3. Prefer non-destructive alternatives first (`git status`, `git diff`, backups, new commits).
4. If destructive action is explicitly authorized, restate command + impact before running.
5. Record exactly what was authorized and executed in the final response.

---

## Git Branch: Use `main`, Keep `master` Synced

- Primary branch is `main`.
- `master` exists for legacy compatibility and must mirror `main`.
- After landing changes:

```bash
git push origin main
git push origin main:master
```

---

## Toolchain: Rust & Cargo

Use Cargo only.

- Rust edition: 2024 (or `rust-toolchain.toml` when present)
- Unsafe code: forbidden (`#![forbid(unsafe_code)]`)
- Prefer explicit dependency versions

Target release profile (once crate exists):

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

---

## Code Editing Discipline

### No Scripted Mass Edits

Do not run repo-wide code transformation scripts. Make intentional, reviewable edits.

### No File Proliferation

Prefer editing existing files. New files are for genuinely new functionality.

### No Surprise Behavior

Do not invent behavior not present in [`docs/PLAN.md`](./docs/PLAN.md).

---

## Backward Compatibility

Prioritize correct architecture over temporary compatibility shims.

- No wrapper layers for deprecated behavior
- No legacy mode branches unless explicitly required by the user

---

## Output Contract (lock-specific, critical)

`lock` is an **artifact tool**:

- Always emits structured JSON to stdout
- No human-report mode for main command
- Domain outcomes are represented by exit code and JSON envelope

Exit semantics:
- `0`: `LOCK_CREATED`
- `1`: `LOCK_PARTIAL`
- `2`: `REFUSAL` (or CLI/process-level failure)

On refusal, emit refusal JSON envelope with code/detail/next_command, not ad-hoc text blocks.

---

## Core Invariants (Do Not Break)

All behavior must follow [`docs/PLAN.md`](./docs/PLAN.md).

### 1. Self-hash integrity

`lock_hash` must be computed exactly as specified:
1. build lock JSON with `lock_hash = ""`
2. canonical serialize
3. SHA256 bytes
4. set `lock_hash = "sha256:<hex>"`
5. emit final JSON

Any change to canonicalization, key ordering, or byte serialization is a breaking change.

### 2. Deterministic ordering

- `members` sorted by `path` (lexicographic byte-order)
- `skipped` sorted by `path`
- stable deterministic output for same logical input + fixed timestamp

### 3. `_skipped` protocol

- `_skipped: true` records do **not** enter `members`
- they **must** enter `skipped`
- any skipped records -> `LOCK_PARTIAL` (exit `1`)

### 4. Missing hash refusal

Non-skipped records missing `bytes_hash` must refuse with `E_MISSING_HASH`.

### 5. Version gate

Reject unknown/missing upstream `version` with `E_BAD_INPUT`.

### 6. Witness parity

Ambient witness semantics must match spine conventions (`shape`/`rvl` parity):
- append by default
- `--no-witness` opt-out
- witness failures do not mutate domain outcome semantics

---

## Compiler & Lint Checks

After substantive changes, run:

```bash
cargo check --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Fix warnings rather than muting them.

---

## Testing

Preferred test commands:

```bash
cargo test
cargo test -- --nocapture
```

If tests do not exist yet, state that clearly in your final response.

Minimum coverage areas (from plan):
- JSONL parse and refusal paths
- `_skipped` handling and partial outcome
- missing hash refusal
- deterministic sorting
- self-hash verification round-trip
- tool_versions merge behavior
- witness append/query paths

---

## CI/CD Expectations

Align local checks with `.github/workflows` once present.

Default local gate:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

---

## Release Process

When landing releasable changes:

1. Verify local gate passes.
2. `git add -A && git commit` with concrete bullet points.
3. Bump `Cargo.toml` semver appropriately.
4. Sync `Cargo.lock` before release workflows that use `--locked`.
5. Push:

```bash
git push origin main
git push origin main:master
```

6. If release workflow exists, verify with `gh run` / `gh release view`.

---

## Third-Party Library Usage

If uncertain about a crate API, check upstream docs and examples before implementing.

---

## lock - This Project

`lock` creates a single self-hashed dataset lockfile from stream pipeline records.

### Source of truth

- [`docs/PLAN.md`](./docs/PLAN.md)

### Core behavior (v0)

- Input: JSONL from stdin or optional file arg
- Output: one lock JSON object or refusal JSON envelope
- Outcomes: `LOCK_CREATED`, `LOCK_PARTIAL`, `REFUSAL`
- Supports witness query subcommands (`query`, `last`, `count`)

### Planned key files

| File | Purpose |
|---|---|
| `src/main.rs` | CLI entry + exit code mapping |
| `src/lib.rs` | orchestration flow |
| `src/cli/` | argument parsing and witness subcommands |
| `src/input/` | JSONL parsing and field extraction |
| `src/lockfile/` | member/skipped construction + sorting |
| `src/lockfile/self_hash.rs` | canonical serialization + SHA256 |
| `src/refusal/` | refusal envelope, codes, details |
| `src/witness/` | witness append/query behavior |
| `operator.json` | machine-readable operator contract |

### Performance posture

- Stream input lines
- Avoid unnecessary allocations
- Preserve deterministic ordering and output

---

## MCP Agent Mail - Multi-Agent Coordination

Use MCP Agent Mail when available for parallel agent work.

### Session start

1. Register identity in this project.
2. Introduce yourself to active agents.
3. Poll inbox regularly and acknowledge `ack_required` messages.

### File reservations

- Reserve only specific files/patterns you are actively editing.
- **Never reserve entire trees** (no `/**`, no `src/**` unless explicitly instructed).
- Release reservations promptly when done.

### Collaboration norms

- Send start/finish updates on each bead or task.
- Keep thread IDs aligned with issue IDs where possible.
- Do not stall in communication loops; pick unblocked work proactively.

---

## Beads (`br`) Workflow

Use Beads as source of truth for task state.

1. Pick work with:

```bash
br ready --json
```

2. Move selected issue to in-progress.
3. Keep issue notes updated as you implement.
4. Close issue with concise evidence once done.

When idle: pick the next unblocked bead you can execute now.

---

## UBS (Ultimate Bug Scanner)

If requested, run UBS scans and convert confirmed findings into actionable beads.

Rules:
- Do not file speculative issues without reproduction details.
- Link findings to concrete file/line evidence.
- Prioritize correctness and reliability issues first.

---

## Landing the Plane (Session Completion)

Before ending a session:

1. Run formatting, linting, and tests.
2. Confirm docs/spec alignment for behavior changes.
3. Commit with precise message.
4. Push `main` and sync `master`.
5. Verify release pipeline status when version changed.
6. Summarize:
   - what changed,
   - what was validated,
   - any remaining risks/follow-ups.

---

## Note on Built-in TODO Tracking

The coding assistant's internal TODO/planner is not the project source of truth.

- Use `br` for task status.
- Use Agent Mail for coordination/audit trail.
- Keep final state synchronized across code, beads, and messages.
