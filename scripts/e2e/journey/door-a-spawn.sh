#!/usr/bin/env bash
# TS-2 · Door-A box-spawn journey (E3 live-smoke) — the headline real-user path.
#
# A `runs-on: corelink` job can land ONLY on a freshly-minted ephemeral runner (the
# autoscaler spawns a cache-warm box on the webhook, the job runs, the box self-deregisters).
# So a SUCCESS conclusion on this job IS end-to-end proof of: webhook -> admit -> spawn
# cache-warm box -> register runner -> execute -> teardown. This SPAWNS A REAL BOX (cost);
# it is a deliberate, budget-bounded run, not part of the cheap no-spawn suite.
#
# Usage: scripts/e2e/journey/door-a-spawn.sh            # dispatch + prove a fresh run
#        scripts/e2e/journey/door-a-spawn.sh <run-id>   # prove an already-dispatched run
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
wf="corelink-smoke.yml"

rid="${1:-}"
if [ -z "$rid" ]; then
  echo "dispatching $wf (runs-on: corelink) ..."
  gh workflow run "$wf" --ref main
  sleep 6
  rid="$(gh run list --workflow="$wf" --limit 1 --json databaseId -q '.[0].databaseId')"
fi
echo "Door-A run: $rid — watching to completion (box spawn + job) ..."
set +e
gh run watch "$rid" --interval 20 --exit-status >/dev/null 2>&1
set -e

read -r status conclusion title < <(gh run view "$rid" --json status,conclusion,displayTitle \
  -q '"\(.status) \(.conclusion) \(.displayTitle)"')
pass=$([ "$conclusion" = "success" ] && echo true || echo false)

# Emit the G2 evidence record for the Door-A journey.
run_id="${E2E_RUN_ID:-door-a-$rid}"
dir="$repo/docs/validation/evidence/$run_id"
mkdir -p "$dir"
url="https://github.com/HumanGuardrail/corelink-runners/actions/runs/$rid"
cat > "$dir/TS2-door-a-spawn.json" <<JSON
{
  "cell": "TS2-door-a-spawn",
  "atoms": ["F-2.1", "F-7.1", "F-5.8", "F-4.3", "F-5.9", "F-6.1", "F-4.1", "F-5.5"],
  "direction": "happy",
  "grade": "E3",
  "suite": "TS-2·door-a",
  "stimulus": "gh workflow run $wf (a real runs-on: corelink job)",
  "assertion": "a corelink-labelled job lands only on a freshly-minted ephemeral cache-warm box; SUCCESS proves webhook -> admit -> spawn -> register -> execute -> teardown end-to-end",
  "pass": $pass,
  "artifact": { "runId": "$rid", "status": "$status", "conclusion": "$conclusion", "url": "$url" },
  "ts": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON

echo "[evidence] Door-A conclusion=$conclusion -> $dir/TS2-door-a-spawn.json"
[ "$pass" = true ] || { echo "Door-A did NOT succeed (conclusion=$conclusion)" >&2; exit 1; }
echo "Door-A journey PROVEN (run $rid)."
