#!/usr/bin/env bash
# entrypoint-signal.test.sh — regression test for the 2026-08-23 leaked-box incident.
#
# WHAT THIS PINS, AND WHY IT EXISTS
#   Three boxes stayed alive 10.5 h against a 15-minute idle window because the
#   container's PID 1 ignored SIGTERM: entrypoint.sh ran `./run.sh ... | tee` in
#   the FOREGROUND with no `trap`, so the shell sat in an uninterruptible wait.
#   The platform's soft stop (@cloudflare/containers `stop()`) is SIGTERM-only and
#   never escalates to SIGKILL, so nothing else could ever stop the box.
#
#   The pre-existing keep-alive test asserted that an idle runner STOPS BEING
#   RENEWED. That is a statement about our bookkeeping, not about the box, and it
#   passed happily through the entire incident. This test asserts the other thing:
#   that the process actually dies when signalled.
#
#   Cell 3 is a NEGATIVE CONTROL and it is subtle, so read this before changing it.
#   The container-side mechanism is the PID 1 signal rule: the kernel does not
#   deliver a default-disposition signal to PID 1, so an untrapped PID 1 discards
#   SIGTERM outright. That rule CANNOT be reproduced here — this test does not run
#   as PID 1, and outside a PID namespace the old shape's shell simply dies. So the
#   negative control asserts the OTHER half of the old defect, which does reproduce
#   anywhere: in a foreground pipeline, `run.sh` and `tee` are separate processes,
#   so when the shell goes away they are ORPHANED and keep running — the box
#   survives regardless. If this cell ever reports the orphan died, the harness has
#   stopped discriminating and cells 1-2 prove nothing.
#
#   Cells 6 and 7 close that gap where the platform allows it: on Linux with user
#   namespaces they re-run both the fix and the old shape as LITERAL PID 1 inside a
#   PID namespace, which is the only place the kernel rule actually applies. They
#   SKIP loudly (never silently pass) where namespaces are unavailable — notably
#   macOS, where this file is often run during development.
#
# Run: bash deploy/runner/test/entrypoint-signal.test.sh
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ENTRYPOINT="$HERE/../entrypoint.sh"
[[ -f "$ENTRYPOINT" ]] || { echo "FATAL: entrypoint.sh not found at $ENTRYPOINT" >&2; exit 1; }

SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

fails=0
pass() { echo "  PASS  $1"; }
fail() { echo "  FAIL  $1"; fails=$((fails + 1)); }

# A stub run.sh standing in for the GitHub Actions runner: it traps TERM and
# exits 143 like the real one, and it would otherwise outlive any test.
make_stub() {
  cat > "$SANDBOX/run.sh" <<'STUB'
#!/usr/bin/env bash
trap 'echo "stub: caught TERM"; exit 143' TERM
echo "$$" > "$STUB_PIDFILE"
echo "stub: started"
for _ in $(seq 1 600); do sleep 0.5; done
echo "stub: timed out"
STUB
  chmod +x "$SANDBOX/run.sh"
}

# Wait until the stub has actually started, so a SIGTERM sent before the child
# exists cannot make a broken entrypoint look responsive.
wait_for_start() {
  local out="$1" i
  for i in $(seq 1 100); do
    grep -q "stub: started" "$out" 2>/dev/null && return 0
    sleep 0.1
  done
  return 1
}

# Send TERM to $pid and report how long it took to exit, or "alive" if it outlived
# the deadline. Deadline is generous: the fix should react in well under a second.
term_and_time() {
  local pid="$1" deadline_ds="$2" i
  kill -TERM "$pid" 2>/dev/null
  for i in $(seq 1 "$deadline_ds"); do
    kill -0 "$pid" 2>/dev/null || { echo "$i"; return 0; }
    sleep 0.1
  done
  echo "alive"
  return 1
}

# `unshare --fork` makes its CHILD the namespace's PID 1 while `unshare` itself
# stays behind in the parent namespace, and it does not forward signals. Signalling
# the wrapper therefore tests nothing about PID 1 — it must be the child.
ns_init_of() {
  local wrapper="$1" i kid
  for i in $(seq 1 50); do
    kid="$(pgrep -P "$wrapper" 2>/dev/null | head -1)"
    [[ -n "$kid" ]] && { echo "$kid"; return 0; }
    sleep 0.1
  done
  return 1
}

echo "entrypoint signal discipline"

# ── Cell 1 — SIGTERM to PID 1 terminates the entrypoint ──────────────────────
make_stub
cp "$ENTRYPOINT" "$SANDBOX/entrypoint.sh"
OUT1="$SANDBOX/out1"
rm -f "$SANDBOX/stub1.pid"
STUB_PIDFILE="$SANDBOX/stub1.pid" RUNSH_OUT="$SANDBOX/runsh1.out" RUNSH_FIFO="$SANDBOX/fifo1" \
  CORELINK_RUNNER_JITCONFIG="test-jit" \
  bash "$SANDBOX/entrypoint.sh" > "$OUT1" 2>&1 &
PID1=$!
if wait_for_start "$SANDBOX/runsh1.out"; then
  took="$(term_and_time "$PID1" 50)"
  if [[ "$took" == "alive" ]]; then
    fail "entrypoint survived SIGTERM for 5s — this is the incident defect"
    kill -KILL "$PID1" 2>/dev/null
  else
    pass "entrypoint exited $((took))00ms after SIGTERM"
  fi
else
  fail "stub run.sh never started (harness problem, not a verdict)"
  kill -KILL "$PID1" 2>/dev/null
fi
wait "$PID1" 2>/dev/null

# ── Cell 2 — the signal reaches run.sh, which shuts down gracefully ──────────
# The runner must get a chance to deregister itself. If PID 1 exited without
# forwarding, the stub's handler would never run and the box would be abandoned
# while still registered with GitHub.
if grep -q "stub: caught TERM" "$SANDBOX/runsh1.out" 2>/dev/null; then
  pass "SIGTERM was forwarded to run.sh (graceful deregistration possible)"
else
  fail "run.sh never received TERM — PID 1 exited without forwarding it"
fi

# ── Cell 2b — and run.sh is actually GONE, not merely signalled ─────────────
# The incident's whole shape was a process that outlived the thing tracking it.
STUB1="$(cat "$SANDBOX/stub1.pid" 2>/dev/null || echo 0)"
if [[ "$STUB1" != "0" ]] && kill -0 "$STUB1" 2>/dev/null; then
  fail "run.sh is STILL RUNNING after the entrypoint exited — the box would leak"
  kill -KILL "$STUB1" 2>/dev/null
else
  pass "run.sh is gone once the entrypoint has exited (no orphan)"
fi

# ── Cell 3 — NEGATIVE CONTROL: the old shape orphans its children ───────────
make_stub
cat > "$SANDBOX/old_shape.sh" <<'OLD'
#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
./run.sh --jitconfig "x" 2>&1 | tee "$RUNSH_OUT"
exit "${PIPESTATUS[0]}"
OLD
chmod +x "$SANDBOX/old_shape.sh"
rm -f "$SANDBOX/stub3.pid"
STUB_PIDFILE="$SANDBOX/stub3.pid" RUNSH_OUT="$SANDBOX/runsh3.out" \
  bash "$SANDBOX/old_shape.sh" > /dev/null 2>&1 &
PID3=$!
if wait_for_start "$SANDBOX/runsh3.out"; then
  STUB3="$(cat "$SANDBOX/stub3.pid" 2>/dev/null || echo 0)"
  kill -TERM "$PID3" 2>/dev/null
  # Give the shell time to go away; the orphan should outlive it.
  for _ in $(seq 1 20); do kill -0 "$PID3" 2>/dev/null || break; sleep 0.1; done
  if [[ "$STUB3" != "0" ]] && kill -0 "$STUB3" 2>/dev/null; then
    pass "negative control: old shape orphans run.sh, which survives (as in the incident)"
  else
    fail "negative control's orphan died — the harness no longer discriminates; cells 1-2 are not trustworthy"
  fi
  kill -KILL "$STUB3" 2>/dev/null
else
  fail "negative control stub never started (harness problem)"
fi
kill -KILL "$PID3" 2>/dev/null
wait "$PID3" 2>/dev/null

# ── Cell 4 — the diagnostic tee is preserved ────────────────────────────────
# `exec ./run.sh` would have fixed the signal path in one line, but it would have
# destroyed the only visibility this container has into a JIT-registration
# failure. The fix is only correct if BOTH properties hold at once.
if grep -q "stub: started" "$SANDBOX/runsh1.out" 2>/dev/null; then
  pass "run.sh output was captured to the diagnostic file"
else
  fail "diagnostic capture lost — the tee is no longer working"
fi

# ── Cell 5 — no trap, no fix: the source must actually carry the handler ────
# Cheap structural guard against a refactor that reintroduces the foreground
# pipeline while the behavioural cells above happen to be skipped or flaky.
if grep -qE '^\s*trap .*(TERM|INT)' "$ENTRYPOINT"; then
  pass "entrypoint.sh installs a TERM/INT trap"
else
  fail "entrypoint.sh has no TERM/INT trap on PID 1"
fi
if grep -qE '^\s*\./run\.sh .*\| *tee' "$ENTRYPOINT"; then
  fail "entrypoint.sh runs run.sh in a foreground pipeline again (the incident shape)"
else
  pass "run.sh is not launched as a foreground pipeline"
fi

# ── Cells 6 & 7 — the real thing: the script running as LITERAL PID 1 ───────
# Everything above runs the entrypoint as an ordinary child, so it can only prove
# the signal-handling logic is structurally right. The defect itself lives in the
# kernel's PID 1 rule: a signal whose disposition is DEFAULT is not delivered to
# PID 1 at all. `unshare --pid --fork` puts the script in that exact position, so
# these two cells test the actual production condition. SIGKILL/SIGSTOP are the
# only signals an ancestor namespace can force on an init process; SIGTERM still
# obeys the handler rule, which is precisely what makes cell 7 meaningful.
if command -v unshare >/dev/null 2>&1 && unshare -r --pid --fork --mount-proc true >/dev/null 2>&1; then
  # Cell 6 — the FIX, as PID 1: must die on SIGTERM.
  make_stub
  rm -f "$SANDBOX/stub6.pid"
  unshare -r --pid --fork --mount-proc \
    env STUB_PIDFILE="$SANDBOX/stub6.pid" RUNSH_OUT="$SANDBOX/runsh6.out" \
        RUNSH_FIFO="$SANDBOX/fifo6" CORELINK_RUNNER_JITCONFIG="test-jit" \
        bash "$SANDBOX/entrypoint.sh" > /dev/null 2>&1 &
  PID6=$!
  NSINIT6="$(ns_init_of "$PID6" || echo "")"
  if [[ -n "$NSINIT6" ]] && wait_for_start "$SANDBOX/runsh6.out"; then
    took6="$(term_and_time "$NSINIT6" 50)"
    if [[ "$took6" == "alive" ]]; then
      fail "as PID 1 the fixed entrypoint STILL ignores SIGTERM — the incident is not actually fixed"
      kill -KILL "$NSINIT6" "$PID6" 2>/dev/null
    else
      pass "as literal PID 1, the fixed entrypoint exits $((took6))00ms after SIGTERM"
    fi
  else
    fail "PID-1 cell: stub never started, or the namespace init could not be resolved (harness problem)"
    kill -KILL "$PID6" 2>/dev/null
  fi
  wait "$PID6" 2>/dev/null

  # Cell 7 — the OLD shape, as PID 1: must SURVIVE SIGTERM. This is the incident
  # reproduced exactly. If it dies, the kernel rule is not in force here and
  # cell 6 is not testing what it claims to test.
  make_stub
  cat > "$SANDBOX/old_pid1.sh" <<'OLD1'
#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
./run.sh --jitconfig "x" 2>&1 | tee "$RUNSH_OUT"
exit "${PIPESTATUS[0]}"
OLD1
  chmod +x "$SANDBOX/old_pid1.sh"
  unshare -r --pid --fork --mount-proc \
    env STUB_PIDFILE="$SANDBOX/stub7.pid" RUNSH_OUT="$SANDBOX/runsh7.out" \
        bash "$SANDBOX/old_pid1.sh" > /dev/null 2>&1 &
  PID7=$!
  NSINIT7="$(ns_init_of "$PID7" || echo "")"
  if [[ -n "$NSINIT7" ]] && wait_for_start "$SANDBOX/runsh7.out"; then
    took7="$(term_and_time "$NSINIT7" 20)"
    if [[ "$took7" == "alive" ]]; then
      pass "as literal PID 1, the old shape discards SIGTERM (the incident, reproduced)"
    else
      fail "PID-1 negative control DIED — the kernel PID 1 rule is not in force; cell 6 proves nothing"
    fi
  else
    fail "PID-1 negative control: stub never started, or the namespace init could not be resolved (harness problem)"
  fi
  kill -KILL "${NSINIT7:-0}" "$PID7" 2>/dev/null
  wait "$PID7" 2>/dev/null
else
  # Loud, never silent. A skipped cell must never read as a passing one.
  echo "  SKIP  PID-1 cells: no usable PID namespace here (expected on macOS; on the"
  echo "        Linux CI fleet this SKIP means unshare/userns regressed and the"
  echo "        strongest cells in this file are not running)"
fi

echo
if [[ "$fails" -eq 0 ]]; then
  echo "entrypoint signal discipline: all cells passed"
  exit 0
fi
echo "entrypoint signal discipline: $fails cell(s) failed"
exit 1
