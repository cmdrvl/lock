#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TOOL="${TOOL:-$ROOT/target/debug/lock}"

stdout="$("$TOOL" --robot-triage)"

jq -e '
  .schema_version == "lock.doctor.triage.v1" and
  .ok == true and
  .capabilities.agent_surfaces.robot_triage.command == "lock --robot-triage" and
  .capabilities.agent_surfaces.capabilities.command == "lock capabilities --json"
' >/dev/null <<<"$stdout"
