#!/usr/bin/env bash
# pre-merge-gate-check.selftest.sh
# =============================================================================
# Proves scripts/pre-merge-gate-check.sh actually refuses the states it claims
# to refuse, and actually passes the state it claims is safe. A safety script
# that has never been shown to refuse is not evidence of anything.
#
# Mechanism: build a fake `gh` on PATH that answers `gh pr view` / `gh pr
# checks` from a fixture, then run the real gate-check script against it and
# assert the exit code (and, where it matters, a message fragment).
#
#   bash scripts/pre-merge-gate-check.selftest.sh
#
# Exits 0 only if all five directions behave as specified; prints a per-case
# PASS/FAIL and a non-zero exit on any failure.
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

# args: name expected_exit fixture_mergeable fixture_mergestatus fixture_checks_json
run_case() {
  local name="$1" expect_exit="$2" mergeable="$3" mergestatus="$4" checks_json="$5"
  local grep_for="${6:-}"

  cat > "$FAKE_BIN/gh" <<EOF
#!/usr/bin/env bash
# Fake gh — answers only what pre-merge-gate-check.sh asks of it.
if [ "\$1" = "pr" ] && [ "\$2" = "view" ]; then
  echo "$mergeable $mergestatus OPEN false"
  exit 0
fi
if [ "\$1" = "pr" ] && [ "\$2" = "checks" ]; then
  cat <<'JSON'
$checks_json
JSON
  exit 0
fi
echo "fake gh: unhandled invocation: \$*" >&2
exit 1
EOF
  chmod +x "$FAKE_BIN/gh"

  set +e
  out="$(PATH="$FAKE_BIN:$PATH" bash "$TARGET" 999 2>&1)"
  actual_exit=$?
  set -e

  local ok=1
  if [ "$actual_exit" -eq "$expect_exit" ]; then
    :
  else
    ok=0
  fi
  if [ -n "$grep_for" ] && ! grep -qF "$grep_for" <<<"$out"; then
    ok=0
  fi

  if [ "$ok" -eq 1 ]; then
    echo "  PASS  $name  (exit=$actual_exit)"
    pass_count=$((pass_count + 1))
  else
    echo "  FAIL  $name  (exit=$actual_exit, expected=$expect_exit)"
    echo "        --- output ---"
    sed 's/^/        /' <<<"$out"
    echo "        --------------"
    fail_count=$((fail_count + 1))
  fi
}

HEALTHY_CHECKS='[
  {"name":"gates","bucket":"pass","link":"https://x/gates"},
  {"name":"dco","bucket":"pass","link":"https://x/dco"},
  {"name":"spawn-worker-ci","bucket":"pass","link":"https://x/swc"}
]'

MISSING_GATE_CHECKS='[
  {"name":"dco","bucket":"pass","link":"https://x/dco"},
  {"name":"spawn-worker-ci","bucket":"pass","link":"https://x/swc"}
]'

# 1. CONFLICTING -> non-zero, with the rebase instruction.
run_case "CONFLICTING refused" 1 "CONFLICTING" "DIRTY" "[]" "git rebase origin/main"

# 2. UNKNOWN -> non-zero. GitHub hasn't finished computing it; never read as fine.
run_case "UNKNOWN refused" 1 "UNKNOWN" "UNKNOWN" "[]" "do NOT read this as a green light"

# 3. Zero checks -> non-zero. Absence of checks is a reason to stop, not merge.
run_case "zero checks refused" 1 "MERGEABLE" "CLEAN" "[]" "NO checks at all"

# 4. An always-present gate missing from the list -> non-zero.
run_case "missing always-present gate refused" 1 "MERGEABLE" "CLEAN" "$MISSING_GATE_CHECKS" "always-present gates never ran"

# 5. Healthy: mergeable + all gates present + all green -> exit 0.
run_case "healthy PR passes" 0 "MERGEABLE" "CLEAN" "$HEALTHY_CHECKS" "All gates green"

echo
echo "  $pass_count passed, $fail_count failed"
if [ "$fail_count" -ne 0 ]; then
  exit 1
fi
