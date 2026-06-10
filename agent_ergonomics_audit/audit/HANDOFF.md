# lock Agent Ergonomics Handoff

Completed pass 1 on 2026-06-10.

## Applied

- Added top-level `lock --robot-triage`.
- Added top-level `lock capabilities --json`.
- Added top-level `lock robot-docs guide`.
- Added safe `lock doctor --fix` refusal with exact alternatives.
- Updated `operator.json`, README, AGENTS.md, and `docs/PLAN.md`.
- Bumped version to `0.5.0`.
- Hardened Homebrew formula generation.

## Validation

- `cargo check --all-targets`
- audit regression scripts R-001 through R-004
- intent corpus: 4 canonical first-try intents, 0 silent failures after patch

## Notes

The skill preflight reported missing `flock` on macOS; this pass continued single-agent. Core lock creation, verification, refusal, and witness behavior were intentionally left unchanged.
