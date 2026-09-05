#!/usr/bin/env bash
# pre-merge-gate-check.selftest.sh
# =============================================================================
# Exercises scripts/pre-merge-gate-check.sh with a fake gh that records every
# mutation attempt. The fixture covers report mode and the safety-critical
# --merge path, including admin override and post-merge state confirmation.
# No GitHub request or merge is made.
#
#   bash scripts/pre-merge-gate-check.selftest.sh
#
# Exits 0 only when every baseline and planted refusal behaves as specified.
# =============================================================================
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="$HERE/pre-merge-gate-check.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FAKE_BIN="$WORK/bin"
mkdir -p "$FAKE_BIN"
HEAD_SHA='1111111111111111111111111111111111111111'
BASE_SHA='3333333333333333333333333333333333333333'
MERGE_SHA='2222222222222222222222222222222222222222'

pass_count=0
fail_count=0

# The fake answers the exact two read queries made by the gate, records merge
# and API calls, and can report MERGED even when merge returns a cleanup error.
cat > "$FAKE_BIN/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

if [ "$1" = "repo" ] && [ "$2" = "view" ]; then
  printf '%s\n' "${GH_REPOSITORY:-owner/repo}"
  exit 0
fi
if [ "$1" = "api" ]; then
  if [ "${GH_API_RC:-0}" -ne 0 ]; then
    echo "fake gh: simulated API failure" >&2
    exit "${GH_API_RC}"
  fi
  path="${@: -1}"
  case "$path" in
    */contents/.github/workflows/ci.yml*) printf '%s\n' '{"sha":"ae9b11926d8418f5044db92ee4b9f97d8886d2c8"}' ;;
    */contents/.github/workflows/dco.yml*) printf '%s\n' '{"sha":"aa510af1234a2cb3127a9249cb1e644fb0a182be"}' ;;
    */contents/.github/workflows/plan-integrity.yml*) printf '%s\n' '{"sha":"c97bb503768fc6fb1efc6a89e3c08ca7eb075a3e"}' ;;
    */pulls/*/files*) printf '%s\n' "${GH_API_FILES}" ;;
    */actions/runs/*/jobs*) printf '%s\n' "${GH_API_JOBS}" ;;
    */actions/runs*) printf '%s\n' "${GH_API_RUNS}" ;;
    *) echo "fake gh: unhandled API path: $path" >&2; exit 97 ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  case "$*" in
    *"--json mergeable,mergeStateStatus,state,isDraft,headRefOid,baseRefOid,potentialMergeCommit"*)
      count=0
      if [ -n "${GH_IDENTITY_COUNT_FILE:-}" ]; then
        count="$(cat "$GH_IDENTITY_COUNT_FILE" 2>/dev/null || echo 0)"
        count=$((count + 1)); printf '%s\n' "$count" >"$GH_IDENTITY_COUNT_FILE"
      fi
      head="${GH_HEAD_SHA}"; base="${GH_BASE_SHA}"; merge="${GH_MERGE_SHA}"
      if [ "${GH_RACE_AT:-0}" -gt 0 ] && [ "$count" -ge "${GH_RACE_AT}" ]; then
        head="${GH_RECHECK_HEAD_SHA:-$head}"; base="${GH_RECHECK_BASE_SHA:-$base}"; merge="${GH_RECHECK_MERGE_SHA:-$merge}"
      fi
      printf '%s %s %s %s %s %s %s\n' ${GH_INITIAL_VIEW} "$head" "$base" "$merge" ;;
    *"--json headRefOid,baseRefOid,potentialMergeCommit"*)
      count=0
      if [ -n "${GH_IDENTITY_COUNT_FILE:-}" ]; then
        count="$(cat "$GH_IDENTITY_COUNT_FILE" 2>/dev/null || echo 0)"
        count=$((count + 1)); printf '%s\n' "$count" >"$GH_IDENTITY_COUNT_FILE"
      fi
      head="${GH_HEAD_SHA}"; base="${GH_BASE_SHA}"; merge="${GH_MERGE_SHA}"
      if [ "${GH_RACE_AT:-0}" -gt 0 ] && [ "$count" -ge "${GH_RACE_AT}" ]; then
        head="${GH_RECHECK_HEAD_SHA:-$head}"; base="${GH_RECHECK_BASE_SHA:-$base}"; merge="${GH_RECHECK_MERGE_SHA:-$merge}"
      fi
      printf '%s %s %s\n' "$head" "$base" "$merge" ;;
    *"--json headRefOid"*)                    printf '%s\n' "${GH_HEAD_SHA}" ;;
    *state,headRefName,isCrossRepository*) cat "$GH_POST_FILE" ;;
    *)                                         printf '%s %s\n' "${GH_INITIAL_VIEW}" "${GH_HEAD_SHA}" ;;
  esac
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "checks" ]; then
  cat "$GH_CHECKS_FILE"
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "merge" ]; then
  printf 'merge %s\n' "$*" >>"$GH_CALLS_FILE"
  if [ "${GH_MARK_MERGED:-0}" = "1" ]; then
    printf '%s\n' 'MERGED feature/test false' >"$GH_POST_FILE"
  fi
  if [ "${GH_MERGE_RC:-0}" -ne 0 ]; then
    echo "fake gh: simulated post-merge cleanup failure" >&2
  fi
  exit "${GH_MERGE_RC:-0}"
fi
echo "fake gh: unhandled invocation: $*" >&2
exit 97
FAKE_GH
chmod +x "$FAKE_BIN/gh"

HEALTHY_CHECKS='[
  {"name":"gates","bucket":"pass","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"},
  {"name":"spawn-worker-ci","bucket":"skipping","link":"https://x/swc"}
]'
PREFIXED_HEALTHY_CHECKS='[
  {"name":"CI / gates","bucket":"pass","link":"https://x/gates"},
  {"name":"DCO / dco","bucket":"pass","link":"https://x/dco"}
]'
MISSING_GATE_CHECKS='[
  {"name":"dco","bucket":"pass","link":"https://x/dco"},
  {"name":"spawn-worker-ci","bucket":"pass","link":"https://x/swc"}
]'
LOOKALIKE_GATE_CHECKS='[
  {"name":"fake-gates","bucket":"pass","link":"https://x/fake-gates"},
  {"name":"fake-dco","bucket":"pass","link":"https://x/fake-dco"}
]'
SKIPPED_REQUIRED_CHECKS='[
  {"name":"gates","bucket":"skipping","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"}
]'
FAILED_CHECKS='[
  {"name":"gates","bucket":"fail","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"}
]'
CANCELLED_CHECKS='[
  {"name":"gates","bucket":"cancel","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"}
]'
PENDING_CHECKS='[
  {"name":"gates","bucket":"pending","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"}
]'
UNKNOWN_BUCKET_CHECKS='[
  {"name":"gates","bucket":"future-status","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"}
]'
API_FILES_NORMAL='[[{"filename":"README.md"}]]'
API_FILES_CONTRACT='[[{"filename":"README.md"}],[{"filename":"docs/plan/contracts/T3-W17.md"}]]'
API_FILES_WORKFLOW='[[{"filename":".github/workflows/ci.yml"}]]'
API_RUNS_OTHER_PR='[{"total_count":2,"workflow_runs":[{"id":101,"path":".github/workflows/ci.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":1000}],"status":"completed","conclusion":"success"},{"id":102,"path":".github/workflows/dco.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":1000}],"status":"completed","conclusion":"success"}]}]'
API_RUNS_WRONG_IDENTITY='[{"total_count":2,"workflow_runs":[{"id":101,"path":".github/workflows/ci.yml","event":"push","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"},{"id":102,"path":".github/workflows/dco.yml","event":"pull_request","head_sha":"1111111111111111111111111111111111111111","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"}]}]'
API_RUNS_BASE='[{"total_count":2,"workflow_runs":[{"id":101,"path":".github/workflows/ci.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"},{"id":102,"path":".github/workflows/dco.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"}]}]'
API_RUNS_DCO='[{"total_count":1,"workflow_runs":[{"id":102,"path":".github/workflows/dco.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"}]}]'
API_RUNS_WITH_PLAN='[{"total_count":3,"workflow_runs":[{"id":101,"path":".github/workflows/ci.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"},{"id":102,"path":".github/workflows/dco.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"},{"id":103,"path":".github/workflows/plan-integrity.yml","event":"pull_request","head_sha":"2222222222222222222222222222222222222222","pull_requests":[{"number":999}],"status":"completed","conclusion":"success"}]}]'
API_JOBS_BASE='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"success","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_FAILED='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"failure","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_CANCELLED='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"cancelled","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_PENDING='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"in_progress","conclusion":null,"run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_SKIPPED='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"skipped","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_UNKNOWN='[{"total_count":2,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"future-status","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_WITH_PLAN='[{"total_count":3,"jobs":[{"id":201,"name":"gates","status":"completed","conclusion":"success","run_attempt":1},{"id":202,"name":"dco","status":"completed","conclusion":"success","run_attempt":1},{"id":203,"name":"Coverage, WP, and AU structure","status":"completed","conclusion":"success","run_attempt":1}]}]'
API_JOBS_LOOKALIKE='[{"total_count":2,"jobs":[{"id":201,"name":"CI / gates","status":"completed","conclusion":"success","run_attempt":1},{"id":202,"name":"DCO / dco","status":"completed","conclusion":"success","run_attempt":1}]}]'

# run_case name expected initial-view checks [gate arguments...] -- [message]
run_case() {
  local name="$1" expected="$2" initial_view="$3" checks="$4"
  shift 4
  local args=() message="" separator=0
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--" ]; then separator=1; shift; continue; fi
    if [ "$separator" -eq 1 ]; then message="$1"; shift; continue; fi
    args+=("$1")
    shift
  done
  args+=(999)
  local api_files="${GH_API_FILES_CASE:-$API_FILES_NORMAL}"
  local api_runs="${GH_API_RUNS_CASE:-$API_RUNS_BASE}"
  local api_jobs="${GH_API_JOBS_CASE:-$API_JOBS_BASE}"
  if [ "$checks" = "$MISSING_GATE_CHECKS" ] || [ "$checks" = "$LOOKALIKE_GATE_CHECKS" ]; then
    api_runs="$API_RUNS_DCO"
  elif [ "$checks" = "$FAILED_CHECKS" ]; then
    api_jobs="$API_JOBS_FAILED"
  elif [ "$checks" = "$CANCELLED_CHECKS" ]; then
    api_jobs="$API_JOBS_CANCELLED"
  elif [ "$checks" = "$PENDING_CHECKS" ]; then
    api_jobs="$API_JOBS_PENDING"
  elif [ "$checks" = "$SKIPPED_REQUIRED_CHECKS" ]; then
    api_jobs="$API_JOBS_SKIPPED"
  elif [ "$checks" = "$UNKNOWN_BUCKET_CHECKS" ]; then
    api_jobs="$API_JOBS_UNKNOWN"
  fi

  local checks_file="$WORK/checks.json"
  local calls_file="$WORK/calls"
  local post_file="$WORK/post"
  printf '%s\n' "$checks" >"$checks_file"
  : >"$calls_file"
  printf '%s\n' 'OPEN feature/test false' >"$post_file"

  local out rc
  set +e
  out="$(env PATH="$FAKE_BIN:$PATH" \
    GH_INITIAL_VIEW="$initial_view" GH_POST_VIEW="$(cat "$post_file")" \
    GH_HEAD_SHA="$HEAD_SHA" GH_BASE_SHA="$BASE_SHA" GH_MERGE_SHA="$MERGE_SHA" GH_REPOSITORY=owner/repo \
    GH_API_FILES="$api_files" GH_API_RUNS="$api_runs" GH_API_JOBS="$api_jobs" \
    GH_POST_FILE="$post_file" GH_CHECKS_FILE="$checks_file" \
    GH_CALLS_FILE="$calls_file" GH_MARK_MERGED=0 GH_MERGE_RC=0 GH_API_RC="${GH_API_RC_CASE:-0}" \
    bash "$TARGET" "${args[@]}" 2>&1)"
  rc=$?
  set -e

  local ok=1
  [ "$rc" -eq "$expected" ] || ok=0
  if [ -n "$message" ] && ! grep -qF -- "$message" <<<"$out"; then ok=0; fi

  if [ "$ok" -eq 1 ]; then
    echo "  PASS  $name  (exit=$rc)"
    pass_count=$((pass_count + 1))
  else
    echo "  FAIL  $name  (exit=$rc, expected=$expected)"
    while IFS= read -r line; do printf '        %s\n' "$line"; done <<<"$out"
    fail_count=$((fail_count + 1))
  fi
}

# A contract-only change must require the path-filtered plan-integrity check;
# this fixture is deliberately green everywhere else and omits that check.
GH_API_FILES_CASE="$API_FILES_CONTRACT"
GH_API_RUNS_CASE="$API_RUNS_BASE"
GH_API_JOBS_CASE="$API_JOBS_BASE"
run_case "contract-only PR missing plan-integrity refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "Coverage, WP, and AU structure"
GH_API_FILES_CASE="$API_FILES_CONTRACT"
GH_API_RUNS_CASE="$API_RUNS_WITH_PLAN"
GH_API_JOBS_CASE="$API_JOBS_WITH_PLAN"
run_case "contract-only PR with plan-integrity passes" 0 \
  "MERGEABLE CLEAN OPEN false" '[
  {"name":"gates","bucket":"pass","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"},
  {"name":"Plan integrity / Coverage, WP, and AU structure","bucket":"pass","link":"https://x/plan"}
]' -- "All gates green"
GH_API_FILES_CASE=""
GH_API_RUNS_CASE=""
GH_API_JOBS_CASE=""
GH_API_FILES_CASE="$API_FILES_WORKFLOW"
run_case "canonical workflow mutation requires protected bootstrap" 1 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "canonical workflow changed"
GH_API_FILES_CASE=""

# Initial structural refusals: each fixture reaches the intended branch.
run_case "CONFLICTING refused" 1 "CONFLICTING DIRTY OPEN false" '[]' -- "git rebase origin/main"
run_case "UNKNOWN refused" 1 "UNKNOWN UNKNOWN OPEN false" '[]' -- "do NOT read this as a green light"
run_case "closed PR refused" 1 "MERGEABLE CLEAN CLOSED false" '[]' -- "not OPEN"
run_case "draft PR refused" 1 "MERGEABLE CLEAN OPEN true" "$HEALTHY_CHECKS" -- "DRAFT"
run_case "zero checks refused" 1 "MERGEABLE CLEAN OPEN false" '[]' -- "NO checks at all"
run_case "missing always-present gate refused" 1 "MERGEABLE CLEAN OPEN false" "$MISSING_GATE_CHECKS" -- "gates  .github/workflows/ci.yml"
run_case "lookalike gate identities refused" 1 "MERGEABLE CLEAN OPEN false" "$LOOKALIKE_GATE_CHECKS" -- "gates  .github/workflows/ci.yml"
run_case "required gate skipped refused" 1 "MERGEABLE CLEAN OPEN false" "$SKIPPED_REQUIRED_CHECKS" -- "required gate(s) skipped"
run_case "pending check refused" 1 "MERGEABLE CLEAN OPEN false" "$PENDING_CHECKS" -- "1 pending"
run_case "cancelled check refused" 1 "MERGEABLE CLEAN OPEN false" "$CANCELLED_CHECKS" -- "gates  .github/workflows/ci.yml"
run_case "unknown bucket refused" 1 "MERGEABLE CLEAN OPEN false" "$UNKNOWN_BUCKET_CHECKS" -- "gates  .github/workflows/ci.yml"
run_case "healthy PR passes" 0 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "All gates green"
run_case "authoritative workflow/job identities pass" 0 "MERGEABLE CLEAN OPEN false" "$PREFIXED_HEALTHY_CHECKS" -- "All gates green"
GH_API_RUNS_CASE="$API_RUNS_OTHER_PR"
run_case "workflow run for another PR refused" 1 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "gates  .github/workflows/ci.yml"
GH_API_RUNS_CASE="$API_RUNS_WRONG_IDENTITY"
run_case "workflow path/event/head provenance refused" 1 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "gates  .github/workflows/ci.yml"
GH_API_RUNS_CASE=""
GH_API_JOBS_CASE="$API_JOBS_LOOKALIKE"
run_case "display-name-only job identity refused" 1 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "gates  .github/workflows/ci.yml"
GH_API_JOBS_CASE=""
GH_API_RC_CASE=7
run_case "authoritative API failure refused" 1 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "authoritative GitHub API query failed"
GH_API_RC_CASE=0

# CLI safety boundaries are checked without invoking gh at all.
run_case "admin reason outside merge refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --admin-reason "why" -- "only meaningful with --merge"
run_case "dry-run outside merge refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --dry-run -- "only meaningful with --merge"
run_case "bare admin refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --admin -- "bare --admin"

# --merge must be exercised with a stateful fake, not merely grep'd source.
run_merge_case() {
  local name="$1" expected="$2" initial_view="$3" checks="$4" mark_merged="$5" merge_rc="$6" expected_call="$7" message="$8"
  local mode="${9:-}" admin="${10:-0}" forbidden_call="${11:-}"
  local checks_file="$WORK/merge-checks.json" calls_file="$WORK/merge-calls" post_file="$WORK/merge-post"
  local api_jobs="$API_JOBS_BASE"
  : >"$WORK/identity-count"
  [ "$checks" = "$FAILED_CHECKS" ] && api_jobs="$API_JOBS_FAILED"
  [ "$checks" = "$CANCELLED_CHECKS" ] && api_jobs="$API_JOBS_CANCELLED"
  [ "$checks" = "$PENDING_CHECKS" ] && api_jobs="$API_JOBS_PENDING"
  printf '%s\n' "$checks" >"$checks_file"
  : >"$calls_file"
  printf '%s\n' 'OPEN feature/test false' >"$post_file"
  local out rc
  set +e
  local merge_args=(--merge)
  [ "$mode" = "dry-run" ] && merge_args+=(--dry-run)
  [ "$admin" -eq 1 ] && merge_args+=(--admin-reason "documented fake runner flake")
  out="$(env PATH="$FAKE_BIN:$PATH" \
    GH_INITIAL_VIEW="$initial_view" GH_POST_FILE="$post_file" \
    GH_POST_VIEW="$(cat "$post_file")" GH_CHECKS_FILE="$checks_file" \
    GH_HEAD_SHA="$HEAD_SHA" GH_BASE_SHA="$BASE_SHA" GH_MERGE_SHA="$MERGE_SHA" GH_REPOSITORY=owner/repo \
    GH_API_FILES="$API_FILES_NORMAL" GH_API_RUNS="$API_RUNS_BASE" GH_API_JOBS="$api_jobs" \
    GH_IDENTITY_COUNT_FILE="$WORK/identity-count" GH_RACE_AT="${RACE_AT_CASE:-0}" \
    GH_RECHECK_HEAD_SHA="${RECHECK_HEAD_CASE:-$HEAD_SHA}" GH_RECHECK_BASE_SHA="${RECHECK_BASE_CASE:-$BASE_SHA}" \
    GH_RECHECK_MERGE_SHA="${RECHECK_MERGE_CASE:-$MERGE_SHA}" \
    GH_CALLS_FILE="$calls_file" GH_MARK_MERGED="$mark_merged" GH_MERGE_RC="$merge_rc" GH_API_RC=0 \
    bash "$TARGET" "${merge_args[@]}" 999 2>&1)"
  rc=$?
  set -e

  local ok=1
  [ "$rc" -eq "$expected" ] || ok=0
  [ -z "$expected_call" ] || grep -qF "$expected_call" "$calls_file" || ok=0
  [ -z "$forbidden_call" ] || ! grep -qF -- "$forbidden_call" "$calls_file" || ok=0
  [ -z "$message" ] || grep -qF -- "$message" <<<"$out" || ok=0
  if [ "$ok" -eq 1 ]; then
    echo "  PASS  $name  (exit=$rc)"
    pass_count=$((pass_count + 1))
  else
    echo "  FAIL  $name  (exit=$rc, expected=$expected)"
    while IFS= read -r line; do printf '        %s\n' "$line"; done <<<"$out"
    echo "        calls: $(tr '\n' ' ' <"$calls_file")"
    fail_count=$((fail_count + 1))
  fi
}

run_merge_case "healthy --merge dry-run does not mutate" 0 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 0 0 "" "NOT executed" dry-run 0 "pr merge"
run_merge_case "healthy --merge lands" 0 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false --match-head-commit $HEAD_SHA" "PR #999 merged"
run_merge_case "post-merge cleanup error still confirms landed" 0 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 1 1 "merge pr merge 999 --squash --delete-branch=false" "IS MERGED"
run_merge_case "admin override allows failed check" 0 \
  "MERGEABLE CLEAN OPEN false" "$FAILED_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false --match-head-commit $HEAD_SHA --admin" "--admin OVERRIDE" normal 1
run_merge_case "admin override allows cancelled check" 0 \
  "MERGEABLE CLEAN OPEN false" "$CANCELLED_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false --match-head-commit $HEAD_SHA --admin" "--admin OVERRIDE" normal 1
run_merge_case "draft admin override refused" 1 \
  "MERGEABLE CLEAN OPEN true" "$HEALTHY_CHECKS" 0 0 "" "NO MERGE ISSUED" normal 1 "pr merge"
run_merge_case "pending admin override refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$PENDING_CHECKS" 0 0 "" "NO MERGE ISSUED" normal 0 "pr merge"
RACE_AT_CASE=3
RECHECK_BASE_CASE='4444444444444444444444444444444444444444'
run_merge_case "base OID race refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 0 0 "" "source/base/merge OID changed" normal 0 "pr merge"
RACE_AT_CASE=3
RECHECK_BASE_CASE="$BASE_SHA"
RECHECK_MERGE_CASE='5555555555555555555555555555555555555555'
run_merge_case "potential merge OID race refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 0 0 "" "source/base/merge OID changed" normal 0 "pr merge"
RACE_AT_CASE=3
RECHECK_HEAD_CASE='6666666666666666666666666666666666666666'
run_merge_case "source head OID race refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 0 0 "" "source/base/merge OID changed" normal 0 "pr merge"
RACE_AT_CASE=0
RECHECK_HEAD_CASE=""
RECHECK_BASE_CASE=""
RECHECK_MERGE_CASE=""

echo
echo "  $pass_count passed, $fail_count failed"
[ "$fail_count" -eq 0 ]
