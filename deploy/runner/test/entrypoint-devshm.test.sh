#!/usr/bin/env bash
# entrypoint-devshm.test.sh — regression test for the /dev/shm provisioning block.
#
# WHY THIS EXISTS
#   The runner image ships no /dev/shm and the fabric does not mount one. Bazel's
#   linux-sandbox opens it unconditionally, so every sandboxed Bazel action on a
#   `runs-on: corelink` box died with
#     I/O exception during sandboxed execution: [unix_jni.cc:382] /dev/shm
#       (No such file or directory)
#   That reads like a Bazel bug. It is a missing mount, and anything expecting
#   POSIX shared memory hits it (pytest-xdist, Chrome, some JVMs).
#
# WHAT IT ASSERTS
#   The block has three branches and only one of them runs in production today.
#   A test that only covered the happy path would not notice the fallback
#   silently disappearing, which is the branch this fabric actually takes.
#
#   1. path already present and writable  -> reported, nothing created
#   2. path absent, tmpfs mount refused   -> plain directory created, mode 1777,
#                                            and the log says LOUDLY that it is
#                                            not shared memory
#   3. the block is reachable before the runner starts, not after
#
#   `CORELINK_SHM_PATH` is the seam: it lets all three run without root and
#   without touching the real /dev. It is never set in production.
#
# Run: bash deploy/runner/test/entrypoint-devshm.test.sh
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ENTRYPOINT="$HERE/../entrypoint.sh"
[[ -f "$ENTRYPOINT" ]] || { echo "FATAL: entrypoint.sh not found at $ENTRYPOINT" >&2; exit 1; }

SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

fails=0
pass() { echo "  PASS  $1"; }
fail() { echo "  FAIL  $1" >&2; fails=$((fails + 1)); }

# Extract just the /dev/shm block so the test does not need a JIT config, a
# network, or the whole runner. Grepping the real file is deliberate: a copy
# would drift and keep passing after the original changed.
block="$(awk '/^SHM_PATH=/{f=1} f{print} f&&/^fi$/{exit}' "$ENTRYPOINT")"
if [[ -z "$block" ]]; then
  echo "FAIL: could not find the SHM_PATH block in entrypoint.sh — either it was" >&2
  echo "      removed or renamed, and this test can no longer see what it guards." >&2
  exit 1
fi

echo "cell 1: path already present and writable"
target="$SANDBOX/present"
mkdir -p "$target"
out="$(CORELINK_SHM_PATH="$target" bash -c "$block" 2>&1)"
if [[ "$out" == *"present and writable"* ]]; then pass "reported as present"; else fail "expected the present branch, got: $out"; fi

echo "cell 2: path absent, tmpfs refused -> plain directory, loudly"
target="$SANDBOX/absent"
out="$(CORELINK_SHM_PATH="$target" bash -c "$block" 2>&1)"
if [[ -d "$target" ]]; then pass "directory created"; else fail "directory was not created"; fi
# Check the two halves separately: `stat`'s low permission field drops the
# sticky bit on macOS (%Lp prints 777 for a 1777 directory), so asserting the
# string "1777" would fail on the machine most likely to run this by hand.
mode="$(stat -f '%Lp' "$target" 2>/dev/null || stat -c '%a' "$target" 2>/dev/null)"
if [[ "$mode" == *"777" ]]; then pass "world-writable (${mode})"; else fail "expected 777 bits, got ${mode:-none}"; fi
if [[ -k "$target" ]]; then pass "sticky bit set"; else fail "sticky bit missing — a world-writable /dev/shm without it lets one job delete another's files"; fi
if [[ "$out" == *"PLAIN DIRECTORY"* && "$out" == *"disk-backed"* ]]; then
  pass "fallback says out loud that it is not shared memory"
else
  fail "fallback was silent about being disk-backed, got: $out"
fi

echo "cell 3: the block runs BEFORE the runner is launched"
# A provisioning step that runs after the job starts is useless. Assert ordering
# in the file itself.
shm_line="$(grep -n '^SHM_PATH=' "$ENTRYPOINT" | head -1 | cut -d: -f1)"
run_line="$(grep -n 'Runner.Listener\|run.sh' "$ENTRYPOINT" | head -1 | cut -d: -f1)"
if [[ -n "$shm_line" && -n "$run_line" && "$shm_line" -lt "$run_line" ]]; then
  pass "provisioned at line $shm_line, runner starts at line $run_line"
else
  fail "ordering not established (shm=${shm_line:-?} runner=${run_line:-?})"
fi

echo
if [[ "$fails" -eq 0 ]]; then echo "entrypoint-devshm: all cells passed"; exit 0; fi
echo "entrypoint-devshm: $fails cell(s) FAILED" >&2
exit 1
