#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TOOL="${TOOL:-$ROOT/target/debug/lock}"

stdout="$("$TOOL" capabilities --json)"

jq -e '
  .schema_version == "lock.doctor.capabilities.v1" and
  .read_only == true and
  .fix_mode.available == false and
  .agent_surfaces.capabilities.command == "lock capabilities --json" and
  .agent_surfaces.robot_docs.command == "lock robot-docs guide" and
  .side_effects.by_command["lock capabilities --json"].writes_witness_ledger == false and
  .side_effects.by_command["lock capabilities --json"].uses_network == false
' >/dev/null <<<"$stdout"
