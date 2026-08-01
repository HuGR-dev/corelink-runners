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

# Audit F1: SUCCESS alone does NOT prove a fresh ephemeral cache-warm spawn — a job could land
# on a stale/persistent runner carrying the label. Pull the ACTUAL runner + machine from the
# job log and prove ephemerality from the evidence, not from a comment.
log="$(gh run view "$rid" --log 2>/dev/null || true)"
runner="$(printf '%s' "$log" | grep -oE "Runner name: '[^']+'" | head -1 | sed -E "s/Runner name: '([^']+)'/\1/")"
machine="$(printf '%s' "$log" | grep -oE "Machine name: '[^']+'" | head -1 | sed -E "s/Machine name: '([^']+)'/\1/")"
# A CF-spawned ephemeral runner is named cf-runner-* on machine 'cloudchamber' (CF Containers).
ephemeral=$([[ "$runner" == cf-runner-* ]] && echo true || echo false)
cf_machine=$([[ "$machine" == cloudchamber* ]] && echo true || echo false)
# Cache-warm is ONLY proven by a [clw] cache hit line — do not claim it if absent.
cache_warm=$(printf '%s' "$log" | grep -qiE 'clw.*cache hit|\[clw\] cache hit' && echo true || echo false)

# The proven claim = spawned a fresh CF ephemeral box + ran + succeeded. cache-warm + teardown
# are recorded as observed-or-not, never assumed.
pass=$([ "$conclusion" = "success" ] && [ "$ephemeral" = true ] && [ "$cf_machine" = true ] && echo true || echo false)

run_id="${E2E_RUN_ID:-door-a-$rid}"
dir="$repo/docs/validation/evidence/$run_id"
mkdir -p "$dir"
url="https://github.com/HuGR-Labs/corelink-runners/actions/runs/$rid"
cat > "$dir/TS2-door-a-spawn.json" <<JSON
{
  "cell": "TS2-door-a-spawn",
  "atoms": ["F-2.1", "F-7.1", "F-5.8", "F-6.1", "F-4.1"],
  "direction": "happy",
  "grade": "E3",
  "suite": "TS-2·door-a",
  "stimulus": "gh workflow run $wf (a real runs-on: corelink job)",
  "assertion": "a corelink job ran on a FRESHLY-SPAWNED CF ephemeral box (runner cf-runner-* on machine 'cloudchamber') and succeeded — proves webhook -> admit -> spawn -> register -> execute. cache-warm hit and teardown are recorded as observed-or-not, NOT assumed",
  "pass": $pass,
  "artifact": {
    "runId": "$rid", "conclusion": "$conclusion",
    "runnerName": "$runner", "machine": "$machine",
    "ephemeralRunner": $ephemeral, "cfContainerMachine": $cf_machine,
    "cacheWarmHitObserved": $cache_warm,
    "teardownCaptured": false,
    "url": "$url"
  },
  "ts": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON

echo "[evidence] Door-A conclusion=$conclusion runner=$runner machine=$machine cacheWarm=$cache_warm -> $dir/TS2-door-a-spawn.json"
[ "$pass" = true ] || { echo "Door-A spawn NOT proven (conclusion=$conclusion runner=$runner machine=$machine)" >&2; exit 1; }
echo "Door-A journey PROVEN: fresh CF ephemeral box $runner on $machine (cache-warm hit observed=$cache_warm)."
