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

pass_count=0
fail_count=0

# The fake answers the exact two read queries made by the gate, records merge
# and API calls, and can report MERGED even when merge returns a cleanup error.
cat > "$FAKE_BIN/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  case "$*" in
    *state,headRefName,isCrossRepository*) cat "$GH_POST_FILE" ;;
    *)                                         printf '%s\n' "${GH_INITIAL_VIEW}" ;;
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
if [ "$1" = "api" ]; then
  printf 'api %s\n' "$*" >>"$GH_CALLS_FILE"
  exit "${GH_API_RC:-0}"
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
    GH_POST_FILE="$post_file" GH_CHECKS_FILE="$checks_file" \
    GH_CALLS_FILE="$calls_file" GH_MARK_MERGED=0 GH_MERGE_RC=0 GH_API_RC=0 \
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

# Initial structural refusals: each fixture reaches the intended branch.
run_case "CONFLICTING refused" 1 "CONFLICTING DIRTY OPEN false" '[]' -- "git rebase origin/main"
run_case "UNKNOWN refused" 1 "UNKNOWN UNKNOWN OPEN false" '[]' -- "do NOT read this as a green light"
run_case "closed PR refused" 1 "MERGEABLE CLEAN CLOSED false" '[]' -- "not OPEN"
run_case "draft PR refused" 1 "MERGEABLE CLEAN OPEN true" "$HEALTHY_CHECKS" -- "DRAFT"
run_case "zero checks refused" 1 "MERGEABLE CLEAN OPEN false" '[]' -- "NO checks at all"
run_case "missing always-present gate refused" 1 "MERGEABLE CLEAN OPEN false" "$MISSING_GATE_CHECKS" -- "always-present gates never ran"
run_case "lookalike gate identities refused" 1 "MERGEABLE CLEAN OPEN false" "$LOOKALIKE_GATE_CHECKS" -- "always-present gates never ran"
run_case "required gate skipped refused" 1 "MERGEABLE CLEAN OPEN false" "$SKIPPED_REQUIRED_CHECKS" -- "required gate(s) skipped"
run_case "pending check refused" 1 "MERGEABLE CLEAN OPEN false" "$PENDING_CHECKS" -- "1 pending"
run_case "cancelled check refused" 1 "MERGEABLE CLEAN OPEN false" "$CANCELLED_CHECKS" -- "bucket=cancel"
run_case "unknown bucket refused" 1 "MERGEABLE CLEAN OPEN false" "$UNKNOWN_BUCKET_CHECKS" -- "bucket=future-status"
run_case "healthy PR passes" 0 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" -- "All gates green"
run_case "authoritative workflow/job identities pass" 0 "MERGEABLE CLEAN OPEN false" "$PREFIXED_HEALTHY_CHECKS" -- "All gates green"

# CLI safety boundaries are checked without invoking gh at all.
run_case "admin reason outside merge refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --admin-reason "why" -- "only meaningful with --merge"
run_case "dry-run outside merge refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --dry-run -- "only meaningful with --merge"
run_case "bare admin refused" 2 "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" --admin -- "bare --admin"

# --merge must be exercised with a stateful fake, not merely grep'd source.
run_merge_case() {
  local name="$1" expected="$2" initial_view="$3" checks="$4" mark_merged="$5" merge_rc="$6" expected_call="$7" message="$8"
  local mode="${9:-}" admin="${10:-0}" forbidden_call="${11:-}"
  local checks_file="$WORK/merge-checks.json" calls_file="$WORK/merge-calls" post_file="$WORK/merge-post"
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
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false" "PR #999 merged"
run_merge_case "post-merge cleanup error still confirms landed" 0 \
  "MERGEABLE CLEAN OPEN false" "$HEALTHY_CHECKS" 1 1 "merge pr merge 999 --squash --delete-branch=false" "IS MERGED"
run_merge_case "admin override allows failed check" 0 \
  "MERGEABLE CLEAN OPEN false" "$FAILED_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false --admin" "--admin OVERRIDE" normal 1
run_merge_case "admin override allows cancelled check" 0 \
  "MERGEABLE CLEAN OPEN false" "$CANCELLED_CHECKS" 1 0 "merge pr merge 999 --squash --delete-branch=false --admin" "--admin OVERRIDE" normal 1
run_merge_case "draft admin override refused" 1 \
  "MERGEABLE CLEAN OPEN true" "$HEALTHY_CHECKS" 0 0 "" "NO MERGE ISSUED" normal 1 "pr merge"
run_merge_case "pending admin override refused" 1 \
  "MERGEABLE CLEAN OPEN false" "$PENDING_CHECKS" 0 0 "" "NO MERGE ISSUED" normal 0 "pr merge"

echo
echo "  $pass_count passed, $fail_count failed"
[ "$fail_count" -eq 0 ]
