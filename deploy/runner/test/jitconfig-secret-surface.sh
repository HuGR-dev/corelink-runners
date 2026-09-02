#!/usr/bin/env bash
# AU3.26a — a JIT blob must not survive in PID 1 or run.sh argv/environment.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ENTRYPOINT="$HERE/../entrypoint.sh"
RUNNER_DOCKERFILE="$HERE/../Dockerfile"
SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT

REAL_PYTHON="$(command -v python3)"
TOKEN_MARKER="au3-26-jit-secret-never-in-proc"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

[[ -x "$ENTRYPOINT" ]] || fail "entrypoint.sh is not executable"
[[ -x "$REAL_PYTHON" ]] || fail "python3 is required for the pinned runner image"

# The manual materializer is deliberately tied to this exact listener version.
# A runner-version bump must review the official JIT file set in the same change.
grep -qx 'ARG RUNNER_VERSION=2.335.1' "$RUNNER_DOCKERFILE" ||
  fail "JIT materialization must be reviewed with RUNNER_VERSION changes"

make_fixture() {
  local root="$1"
  mkdir -p "$root/bin" "$root/tool-bin" "$root/shm"
  cp "$ENTRYPOINT" "$root/entrypoint.sh"
  chmod +x "$root/entrypoint.sh"

  # Presence of the pinned listener selects the production materialization path.
  printf '#!/usr/bin/env bash\nexit 99\n' > "$root/bin/Runner.Listener"
  chmod +x "$root/bin/Runner.Listener"

  # Observe the private bridge file when its decoder opens it. The production
  # decoder validates the same mode and unlinks immediately after this point.
  # shellcheck disable=SC2016  # literal lines for the generated wrapper.
  printf '%s\n' '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'secret_path="$2"' \
    'mode="$(stat -f "%Lp" "$secret_path" 2>/dev/null || stat -c "%a" "$secret_path")"' \
    'printf "%s\n%s\n" "$secret_path" "$mode" > "$JIT_OBSERVATION"' \
    'exec "$REAL_PYTHON" "$@"' > "$root/tool-bin/python3"
  chmod +x "$root/tool-bin/python3"

  # shellcheck disable=SC2016  # literal lines for the generated stub.
  printf '%s\n' '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'printf "%s\n" "$#" > "$RUN_ARGS_COUNT"' \
    'printf "%s\n" "$*" > "$RUN_ARGS"' \
    'env > "$RUN_ENV"' \
    'printf "%s\n" "$$" > "$RUN_PID"' \
    '[[ "$(cat .runner)" == "runner-config" ]]' \
    '[[ "$(cat .credentials)" == "au3-26-jit-secret-never-in-proc" ]]' \
    '[[ "$(cat .credentials_rsaparams)" == "rsa-config" ]]' \
    'printf "%s\n%s\n%s\n" "$(stat -f "%Lp" .runner 2>/dev/null || stat -c "%a" .runner)" "$(stat -f "%Lp" .credentials 2>/dev/null || stat -c "%a" .credentials)" "$(stat -f "%Lp" .credentials_rsaparams 2>/dev/null || stat -c "%a" .credentials_rsaparams)" > "$CONFIG_MODES"' \
    'sleep 2' > "$root/run.sh"
  chmod +x "$root/run.sh"
}

encoded_jit="$($REAL_PYTHON - "$TOKEN_MARKER" <<'PY'
import base64
import json
import sys

def b64(value):
    return base64.b64encode(value.encode()).decode()

payload = {
    ".runner": b64("runner-config"),
    ".credentials": b64(sys.argv[1]),
    ".credentials_rsaparams": b64("rsa-config"),
}
print(base64.b64encode(json.dumps(payload).encode()).decode(), end="")
PY
)"

fixture="$SANDBOX/good"
make_fixture "$fixture"
JIT_OBSERVATION="$fixture/jit-observation" \
RUN_ARGS_COUNT="$fixture/run-args-count" \
RUN_ARGS="$fixture/run-args" \
RUN_ENV="$fixture/run-env" \
RUN_PID="$fixture/run-pid" \
CONFIG_MODES="$fixture/config-modes" \
REAL_PYTHON="$REAL_PYTHON" \
PATH="$fixture/tool-bin:$PATH" \
CORELINK_SHM_PATH="$fixture/shm" \
RUNSH_OUT="$fixture/runsh.out" \
RUNSH_FIFO="$fixture/runsh.fifo" \
CORELINK_RUNNER_JITCONFIG="$encoded_jit" \
bash "$fixture/entrypoint.sh" > "$fixture/entrypoint.out" 2>&1 &
entrypoint_pid=$!

# Keep this tolerant of a busy shared runner: the entrypoint deliberately
# performs its sealed re-exec and setup before run.sh starts.  The old five-
# second window made the security test itself flaky under parallel CI load.
for _ in $(seq 1 400); do
  [[ -s "$fixture/run-pid" ]] && break
  kill -0 "$entrypoint_pid" 2>/dev/null || break
  sleep 0.05
done
[[ -s "$fixture/run-pid" ]] || {
  sed 's/^/  | /' "$fixture/entrypoint.out" >&2 || true
  sed 's/^/  > /' "$fixture/runsh.out" >&2 || true
  fail "run.sh fixture never started"
}

run_pid="$(cat "$fixture/run-pid")"
if [[ -r "/proc/$entrypoint_pid/environ" ]]; then
  entrypoint_env="$(tr '\0' '\n' < "/proc/$entrypoint_pid/environ")"
  [[ "$entrypoint_env" != *"$encoded_jit"* && "$entrypoint_env" != *"$TOKEN_MARKER"* ]] ||
    fail "PID 1 initial environment contains JIT secret material"
  if grep -Eq '(^|[[:space:]])(CORELINK_RUNNER_JITCONFIG|ACTIONS_RUNNER_INPUT_JITCONFIG)=' <<<"$entrypoint_env"; then
    fail "PID 1 initial environment retains a JIT-bearing variable"
  fi
else
  # macOS has no Linux procfs. CI's Linux runner must exercise the assertion.
  echo "SKIP: /proc unavailable; PID 1 environ assertion requires Linux"
fi

if [[ -r "/proc/$run_pid/cmdline" ]]; then
  proc_cmd="$(tr '\0' ' ' < "/proc/$run_pid/cmdline")"
  proc_env="$(tr '\0' '\n' < "/proc/$run_pid/environ")"
else
  proc_cmd="$(ps -p "$run_pid" -o command=)"
  proc_env="$(cat "$fixture/run-env")"
fi

[[ "$proc_cmd" != *"$encoded_jit"* && "$proc_cmd" != *"$TOKEN_MARKER"* ]] ||
  fail "run.sh command line contains JIT secret material"
[[ "$proc_env" != *"$encoded_jit"* && "$proc_env" != *"$TOKEN_MARKER"* ]] ||
  fail "run.sh environment contains JIT secret material"

wait "$entrypoint_pid"

[[ "$(cat "$fixture/run-args-count")" == "0" ]] ||
  fail "run.sh received arguments: $(cat "$fixture/run-args")"
if grep -Eq '(^|[[:space:]])(CORELINK_RUNNER_JITCONFIG|ACTIONS_RUNNER_INPUT_JITCONFIG|CORELINK_RUNNER_JITCONFIG_FILE)=' "$fixture/run-env"; then
  fail "run.sh inherited a JIT-bearing value or bridge pathname"
fi

if command -v mapfile >/dev/null 2>&1; then
  mapfile -t observation < "$fixture/jit-observation"
  mapfile -t config_modes < "$fixture/config-modes"
else
  observation=("$(sed -n '1p' "$fixture/jit-observation")" "$(sed -n '2p' "$fixture/jit-observation")")
  config_modes=("$(sed -n '1p' "$fixture/config-modes")" "$(sed -n '2p' "$fixture/config-modes")" "$(sed -n '3p' "$fixture/config-modes")")
fi
[[ "${observation[1]}" == "600" ]] ||
  fail "JIT bridge file mode was ${observation[1]}, expected 600"
[[ ! -e "${observation[0]}" ]] ||
  fail "JIT bridge pathname still exists after run.sh started"
[[ "${config_modes[0]}" == "600" && "${config_modes[1]}" == "600" && "${config_modes[2]}" == "600" ]] ||
  fail "materialized runner config modes were ${config_modes[*]}, expected 600 600 600"

# Source tripwire: neither direct JIT argv nor the runner's value-bearing input
# env may return in a refactor.
if grep -Eq '^[[:space:]]*[^#].*(--jitconfig[[:space:]]+|ACTIONS_RUNNER_INPUT_JITCONFIG=)' "$ENTRYPOINT"; then
  fail "entrypoint source contains a secret-bearing runner launch surface"
fi

# Malformed input fails closed and is still unlinked; run.sh must not start.
bad="$SANDBOX/bad"
make_fixture "$bad"
rm -f "$bad/run-pid"
set +e
JIT_OBSERVATION="$bad/jit-observation" \
RUN_ARGS_COUNT="$bad/run-args-count" \
RUN_ARGS="$bad/run-args" \
RUN_ENV="$bad/run-env" \
RUN_PID="$bad/run-pid" \
CONFIG_MODES="$bad/config-modes" \
REAL_PYTHON="$REAL_PYTHON" \
PATH="$bad/tool-bin:$PATH" \
CORELINK_SHM_PATH="$bad/shm" \
RUNSH_OUT="$bad/runsh.out" \
RUNSH_FIFO="$bad/runsh.fifo" \
CORELINK_RUNNER_JITCONFIG="not-base64" \
bash "$bad/entrypoint.sh" > "$bad/entrypoint.out" 2>&1
bad_rc=$?
set -e
[[ "$bad_rc" -ne 0 ]] || fail "malformed JIT config did not fail closed"
[[ ! -e "$bad/run-pid" ]] || fail "run.sh started after malformed JIT config"
bad_secret_path="$(sed -n '1p' "$bad/jit-observation")"
[[ -n "$bad_secret_path" && ! -e "$bad_secret_path" ]] ||
  fail "malformed JIT bridge file was not unlinked"

# A caller must not be able to skip stage one and make the sealed stage inspect
# or remove an arbitrary file.  Validation occurs before shm/clw/runner work,
# and this victim must remain byte-for-byte present.
forged_victim="$SANDBOX/forged-victim"
printf '%s' 'forged-victim-must-survive' > "$forged_victim"
chmod 0600 "$forged_victim"
set +e
env -u CORELINK_RUNNER_JITCONFIG \
  -u ACTIONS_RUNNER_INPUT_JITCONFIG \
  CORELINK_RUNNER_JITCONFIG_SEALED=1 \
  CORELINK_RUNNER_JITCONFIG_FILE="$forged_victim" \
  bash "$fixture/entrypoint.sh" > "$SANDBOX/forged.out" 2>&1
forged_rc=$?
set -e
[[ "$forged_rc" -ne 0 ]] || fail "forged sealed state did not fail closed"
[[ -f "$forged_victim" && ! -L "$forged_victim" ]] ||
  fail "forged sealed state removed or replaced its victim"
[[ "$(cat "$forged_victim")" == 'forged-victim-must-survive' ]] ||
  fail "forged sealed state modified its victim"

echo "jitconfig secret surface: all cells passed"
