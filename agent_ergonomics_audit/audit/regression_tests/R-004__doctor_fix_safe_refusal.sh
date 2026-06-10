#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TOOL="${TOOL:-$ROOT/target/debug/lock}"

set +e
stderr="$("$TOOL" doctor --fix 2>&1 >/dev/null)"
status=$?
set -e

test "$status" -eq 2
test -z "$("$TOOL" doctor --fix 2>/dev/null || true)"
grep -Fq "lock doctor --fix is unavailable" <<<"$stderr"
grep -Fq "lock --robot-triage" <<<"$stderr"
grep -Fq "lock capabilities --json" <<<"$stderr"
grep -Fq "lock robot-docs guide" <<<"$stderr"
