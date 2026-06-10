#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TOOL="${TOOL:-$ROOT/target/debug/lock}"

stdout="$("$TOOL" robot-docs guide)"

grep -Fq "lock --robot-triage" <<<"$stdout"
grep -Fq "lock capabilities --json" <<<"$stdout"
grep -Fq "lock robot-docs guide" <<<"$stdout"
grep -Fq "lock doctor --fix" <<<"$stdout"
