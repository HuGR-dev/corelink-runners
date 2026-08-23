#!/usr/bin/env bash
# pre-merge-gate-check.sh <PR-number>
# =============================================================================
# MANDATORY pre-merge gate. Run this BEFORE every `gh pr merge`.
#
#   bash scripts/pre-merge-gate-check.sh <PR>
#
# Exits 0 ONLY when the PR is mergeable AND its real gates ran AND every
# non-skipping check is green. Exits 1 (and prints why) otherwise — in which
# case DO NOT MERGE.
#
# Ported from corelink-server's scripts/pre-merge-gate-check.sh (2026-08-22),
# after the same failure mode bit this family TWICE in one family of repos in
# two hours: a PR with a merge conflict produces ZERO `pull_request` check
# runs — not queued, not failed, ABSENT — because GitHub cannot build the
# merge ref. `pull_request_target` workflows still fire on the base branch, so
# `gh pr checks` reports "nothing failing", which reads as green to both a
# human and a script. The most dangerous PR state produces the most
# reassuring output.
#
# corelink-runners does not carry the server's heavy nightly-gate split (no
# coverage/CodeQL/ffi-matrix/OKF here) — this repo's PR-gated lanes are just
# `ci.yml` (job `gates`: fmt/clippy/test/deny/audit on the self-hosted
# `corelink` fleet) and `dco.yml` (job `dco`: Signed-off-by enforcement), both
# bare `on: pull_request` with no `paths:` filter, so both run on EVERY PR.
# `spawn-worker-ci.yml` also triggers on `pull_request` but is `paths:`
# filtered to `deploy/cloudflare/**`, so it is deliberately excluded from the
# always-present list below — a PR that doesn't touch that path legitimately
# has no such check, and requiring it would turn this script into fail-noisy
# instead of fail-closed (see the comment above REQUIRED_PRESENT).
#
# ⚠️ Branch protection has `required checks = []`, so THIS SCRIPT is the last
# line of defense. It must never fail OPEN.
#
# Three defenses below, in order of how badly the source incident needed them:
#   1. mergeable must be MERGEABLE (not CONFLICTING/UNKNOWN)
#   2. the always-present gates must actually appear in the check list
#   3. "no checks at all" is a FAILURE, not "nothing to gate"
# =============================================================================
set -euo pipefail

PR="${1:?usage: bash scripts/pre-merge-gate-check.sh <PR-number>}"

# ── Defense 1: mergeability ───────────────────────────────────────────────────
# GitHub computes `mergeable` asynchronously, so UNKNOWN means "ask again", not
# "fine". Both non-MERGEABLE states are refused: on a conflict the check list is
# actively misleading (see the header), and on UNKNOWN we cannot yet tell.
state="$(gh pr view "$PR" --json mergeable,mergeStateStatus,state \
  -q '"\(.mergeable) \(.mergeStateStatus) \(.state)"' 2>/dev/null || echo "ERROR ERROR ERROR")"
mergeable="${state%% *}"
rest="${state#* }"
mergestatus="${rest%% *}"
prstate="${rest#* }"

if [ "$prstate" != "OPEN" ]; then
  echo "  ⛔ DO NOT MERGE PR #$PR — the PR is $prstate, not OPEN."
  exit 1
fi

if [ "$mergeable" != "MERGEABLE" ]; then
  echo "  ⛔ DO NOT MERGE PR #$PR — mergeable=$mergeable ($mergestatus)."
  echo
  if [ "$mergeable" = "CONFLICTING" ]; then
    echo "     A CONFLICTING PR gets NO \`pull_request\` checks at all: those run on"
    echo "     the merge ref, which GitHub cannot build. Any green you see below is"
    echo "     \`pull_request_target\` metadata jobs only — it proves NOTHING."
    echo
    echo "     Fix:  git rebase origin/main && git push --force-with-lease"
    echo "     Then re-run this script and wait for the real checks to appear."
  else
    echo "     GitHub has not finished computing mergeability. Re-run in a moment;"
    echo "     do NOT read this as a green light."
  fi
  exit 1
fi

# ── Defense 3 (ordering: checked before the per-check loop) ───────────────────
# "No checks reported" used to `exit 0` with "nothing to gate". On a repo where
# every PR runs gates + dco unconditionally, no checks means the workflows did
# not fire — which is a reason to STOP, not to merge.
json="$(gh pr checks "$PR" --json name,bucket,link 2>/dev/null || true)"
if [ -z "$json" ] || [ "$json" = "[]" ]; then
  echo "  ⛔ DO NOT MERGE PR #$PR — the PR reports NO checks at all."
  echo "     That is not 'nothing to gate': every PR here runs gates + dco"
  echo "     unconditionally, so zero checks means the workflows never fired."
  exit 1
fi

GATE_JSON="$json" python3 - "$PR" <<'PY'
import os, sys, json
pr = sys.argv[1]
data = json.loads(os.environ["GATE_JSON"])
# Bucket handling is ALLOWLIST-based, not denylist-based, and that is the whole
# point. Testing `b == "fail"` / `b == "pending"` and letting every other
# bucket fall through as green would let a **cancelled** check print as
# `? cancel  gates` and the script still conclude "all gates green". A
# cancelled gate has not run; it has proven nothing.
#
# Anything that is not an explicit PASS or an explicit SKIP is now BLOCKING,
# including a bucket name GitHub has not invented yet. Unknown => blocking is
# the fail-CLOSED direction, which is the only acceptable one here.
PASS_BUCKETS = {"pass"}
SKIP_BUCKETS = {"skipping"}
order = {"fail": 0, "cancel": 0, "pending": 1, "skipping": 2, "pass": 3}
mark = {"pass": "✓", "fail": "✗", "cancel": "✗", "pending": "…", "skipping": "-"}
fails, pends = [], []
for c in sorted(data, key=lambda x: order.get(x.get("bucket"), 0)):
    b = c.get("bucket")
    print(f"  {mark.get(b,'✗')} {str(b):9} {c.get('name')}")
    if b in PASS_BUCKETS or b in SKIP_BUCKETS:
        continue
    if b == "pending":
        pends.append(c)
    else:
        # fail, cancel, or anything unrecognised.
        fails.append(c)
print()

# ── Defense 2: the real gates must be PRESENT, not merely not-failing ─────────
# Chosen because both are triggered by a bare `on: pull_request` with NO paths
# filter (ci.yml job `gates`, dco.yml job `dco`), so they run for every PR
# regardless of what it touches. If either is missing, the `pull_request`
# workflows did not fire and the rest of this list is metadata jobs that gate
# nothing. Substring match keeps this robust against job-name edits.
#
# Keep this list SMALL and path-filter-free. `spawn-worker-ci.yml` is
# deliberately excluded — it is paths-filtered to `deploy/cloudflare/**`, so a
# PR that doesn't touch that path legitimately has no such check. Adding a
# path-filtered gate here would make the script fail on PRs that legitimately
# skip it — turning a fail-open into a fail-noisy, which gets the whole check
# disabled by the next person in a hurry.
REQUIRED_PRESENT = ["gates", "dco"]
names = " ".join(c.get("name", "").lower() for c in data)
missing = [g for g in REQUIRED_PRESENT if g not in names]
if missing:
    print(f"  ⛔ DO NOT MERGE PR #{pr} — the always-present gates never ran: "
          f"{', '.join(missing)}.")
    print("     Every PR triggers these unconditionally (bare `on: pull_request`,")
    print("     no paths filter), so their ABSENCE means the pull_request")
    print("     workflows did not fire for this head sha. The checks listed above")
    print("     may be pull_request_target metadata jobs; they gate nothing.")
    print("     Usual cause: the PR conflicts, or the head sha was force-pushed")
    print("     while runs were being created. Rebase, push, and re-run.")
    sys.exit(1)

if fails or pends:
    print(f"  ⛔ DO NOT MERGE PR #{pr} — {len(fails)} not-green, {len(pends)} pending.")
    for c in fails:
        b = c.get("bucket")
        why = "" if b == "fail" else f"  [bucket={b} — not a pass; it did not run to a verdict]"
        print(f"     ✗ {c.get('name')}  {c.get('link','')}{why}")
    print("     Fix or re-run until green. Use --admin ONLY for a documented,")
    print("     non-blocking infra/flake reason you state explicitly.")
    sys.exit(1)
print(f"  ✅ All gates green — OK to merge PR #{pr}.")
PY
