# Coverage Parity Audit: `lock` vs `rvl`

Date: 2026-02-25
Owner: `bd-1ke` (`MagentaLake`)

## Scope

Audit parity depth across these layers:
- Unit
- Integration
- Smoke/process
- Golden/regression

Reference baseline:
- `lock` repo (current `main` workspace)
- `rvl` repo at `/Users/zac/Source/cmdrvl/rvl`

## Method

Commands used for inventory:
- `rg -n "^\s*#\[test\]" src tests`
- `rg -n "^\s*#\[test\]" /Users/zac/Source/cmdrvl/rvl/src /Users/zac/Source/cmdrvl/rvl/tests`
- file-level pattern diff for smoke/golden/witness/schema tests.

## Current Test Inventory

### `lock`

- Total tests: **223** (updated 2026-02-25)
- Core files:
  - Unit (102 tests) in module files (`src/cli/mod.rs`, `src/input/mod.rs`, `src/lockfile/mod.rs`, `src/lockfile/self_hash.rs`, `src/output/mod.rs`, `src/refusal/mod.rs`, `src/witness/mod.rs`, `src/lib.rs`)
  - Integration (83 tests) in:
    - `tests/unit_parsing.rs` (33)
    - `tests/integration_outcomes.rs` (37)
    - `tests/e2e_spine.rs` (13)
  - Smoke/E2E (20 tests) in:
    - `tests/witness_dispatch.rs` (13)
    - `tests/cli_smoke.rs` (7) — added per P0 gap
  - Golden/Regression (8 tests) in:
    - `tests/golden_outputs.rs` (2) — added per P1 gap
    - `tests/witness_schema.rs` (6) — added per P1 gap
  - Fixtures:
    - `tests/fixtures/golden/lock_created.json`
    - `tests/fixtures/golden/refusal_missing_hash.json`
    - `schemas/witness-v0.schema.json`

Strengths:
- Strong refusal/outcome contract coverage.
- Strong deterministic/self-hash invariants.
- Strong witness module filtering and append semantics.
- Binary process smoke tests covering all three exit code domains.
- Golden fixture regression tests for lockfile and refusal output stability.
- Witness schema validation ensuring ledger record shape stability.

### `rvl` (reference depth)

- Total tests observed: **324**
- Notable parity patterns:
  - Process smoke/CLI routing (`tests/cli_exit.rs`, `tests/exit_routing.rs`)
  - Golden outputs + fixture-based regression (`tests/output_golden.rs`, `tests/regression.rs`, `tests/fixtures/**`)
  - Witness process-path tests (`tests/witness.rs`, `tests/witness_query.rs`)
  - Witness schema validation suite (`tests/witness_schema.rs`)

## Gap Matrix (Parity View)

| Layer | `lock` status | `rvl` pattern | Gap | Priority |
|---|---|---|---|---|
| Unit | Strong | Strong | No critical gap | — |
| Integration (library orchestration) | Strong | Strong | Minor fixture-depth gap | P2 |
| Smoke/process CLI (binary-level) | Strong | Strong | `tests/cli_smoke.rs` covers LOCK_CREATED/LOCK_PARTIAL/REFUSAL + witness subcommands | **Closed** |
| Golden output fixtures | Strong | Strong | `tests/golden_outputs.rs` + `tests/fixtures/golden/` covers lockfile + refusal | **Closed** |
| Witness schema conformance | Strong | Strong | `tests/witness_schema.rs` + `schemas/witness-v0.schema.json` covers valid/invalid records | **Closed** |

## Prioritized Gaps

### P0 — Closed

1. **Binary process smoke coverage for domain routing** — Implemented in `tests/cli_smoke.rs` (7 tests).
   - Covers: LOCK_CREATED/LOCK_PARTIAL/REFUSAL exit codes + stdout contracts.
   - Covers: witness query/last/count exit behavior and JSON mode.

### P1 — Closed

2. **Golden fixture suite for stdout contract stability** — Implemented in `tests/golden_outputs.rs` (2 tests) + `tests/fixtures/golden/`.
   - Covers: canonical lockfile render + refusal envelope byte-stable snapshots.

3. **Witness schema validation suite** — Implemented in `tests/witness_schema.rs` (6 tests) + `schemas/witness-v0.schema.json`.
   - Covers: valid record validation, missing required fields, invalid values.

### P2 (defer)

4. **Broader corpus/regression fixture expansion**
   - Add malformed JSONL corpus and edge-path fixtures for parser stress and long-tail compatibility.

## Implementation Routing

Follow-up implementation bead: **`bd-1ez`**

File-level implementation plan for `bd-1ez`:
- P0 — **Done**:
  - `tests/cli_smoke.rs`: binary process-path routing tests for main + witness (7 tests).
- P1 — **Done**:
  - `tests/golden_outputs.rs` + `tests/fixtures/golden/*.json`: golden fixture regression (2 tests).
  - `tests/witness_schema.rs` + `schemas/witness-v0.schema.json`: witness schema validation (6 tests).
- P2 (later bead):
  - `tests/fixtures/corpus/*` + expanded parser regression tests.

## Acceptance Mapping (`bd-1ke`)

- Written gap report: **done** (`docs/coverage-parity-audit.md`).
- Prioritized missing tests (P0/P1/P2): **done**.
- Follow-up bead linkage: **done** (mapped to `bd-1ez`, with file-level scope).
- P0/P1 gaps implemented: **done** (223 tests, all layers at parity).
- Remaining gap: P2 corpus/regression expansion (deferred).
