#!/usr/bin/env bash
# validate.sh — lightweight self-check for integrations/github-actions/action.yml
#
# Asserts:
#   1. action.yml is valid YAML (python3 stdlib; falls back to structural grep
#      if pyyaml is absent).
#   2. Every input declared in action.yml is referenced in the run steps.
#   3. Every output declared in action.yml is set somewhere in the steps.
#   4. The PAT input is never echoed (no `echo "${{ inputs.pat }}"` pattern).
#   5. The `--json` flag is present (required for output parsing).
#   6. `$GITHUB_OUTPUT` is written (output wiring exists).
#
# Dependency-light: python3 stdlib only.  pyyaml is tried first; if absent
# the script falls back to structural grep assertions.
#
# Exit 0 = all assertions passed.
# Exit 1 = one or more assertions failed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ACTION_FILE="$SCRIPT_DIR/../action.yml"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

PASS=0
FAIL=0

assert_true() {
  local label="$1"
  local result="$2"   # 0 = true, non-zero = false
  if [[ "$result" -eq 0 ]]; then
    echo -e "${GREEN}PASS${NC}  $label"
    ((PASS++)) || true
  else
    echo -e "${RED}FAIL${NC}  $label"
    ((FAIL++)) || true
  fi
}

# ---------------------------------------------------------------------------
# 1. YAML validity
# ---------------------------------------------------------------------------
YAML_PARSE_OK=0
if python3 -c "import yaml" 2>/dev/null; then
  python3 -c "
import yaml, sys
try:
    data = yaml.safe_load(open('$ACTION_FILE'))
    if not isinstance(data, dict):
        print('ERROR: parsed value is not a dict', file=sys.stderr)
        sys.exit(1)
    # Minimal structure checks
    assert 'name' in data,    'missing top-level name'
    assert 'inputs' in data,  'missing inputs'
    assert 'outputs' in data, 'missing outputs'
    assert 'runs' in data,    'missing runs'
    assert data['runs'].get('using') == 'composite', 'runs.using must be composite'
    print('YAML structure OK')
except Exception as e:
    print(f'ERROR: {e}', file=sys.stderr)
    sys.exit(1)
" 2>&1 || YAML_PARSE_OK=1
else
  # Fallback: structural grep — ensure key sections are present
  grep -q "^name:" "$ACTION_FILE"            || YAML_PARSE_OK=1
  grep -q "^inputs:"  "$ACTION_FILE"         || YAML_PARSE_OK=1
  grep -q "^outputs:" "$ACTION_FILE"         || YAML_PARSE_OK=1
  grep -q "^runs:"    "$ACTION_FILE"         || YAML_PARSE_OK=1
  grep -q "using: composite" "$ACTION_FILE"  || YAML_PARSE_OK=1
fi
assert_true "action.yml parses as valid YAML with required sections" "$YAML_PARSE_OK"

# ---------------------------------------------------------------------------
# 2. All declared inputs referenced in run steps
# ---------------------------------------------------------------------------
# Extract input names (lines matching "  <name>:" under inputs:, indented 2 sp)
declare -a INPUTS
while IFS= read -r line; do
  name=$(echo "$line" | sed 's/^  //' | sed 's/:.*//')
  INPUTS+=("$name")
done < <(python3 -c "
import yaml, sys
data = yaml.safe_load(open('$ACTION_FILE'))
for k in data.get('inputs', {}).keys():
    print(f'  {k}:')
" 2>/dev/null || grep -E "^  [a-z]" "$ACTION_FILE" | grep -v "^  #" | sed 's/:.*//' | head -20)

INPUTS_OK=0
for input_name in "${INPUTS[@]}"; do
  trimmed="${input_name//[[:space:]]/}"
  if ! grep -q "inputs\.$trimmed" "$ACTION_FILE"; then
    echo "    input '$trimmed' declared but not referenced in steps"
    INPUTS_OK=1
  fi
done
assert_true "All declared inputs are referenced in the run steps" "$INPUTS_OK"

# ---------------------------------------------------------------------------
# 3. Declared outputs are set via $GITHUB_OUTPUT
# ---------------------------------------------------------------------------
OUTPUTS_OK=0
# Check that the key output fields appear in GITHUB_OUTPUT writes
for field in exit verified lease_id; do
  if ! grep -q "\"$field=" "$ACTION_FILE" && ! grep -q "'$field=" "$ACTION_FILE" && ! grep -q "${field}=" "$ACTION_FILE"; then
    echo "    output field '$field' not found in GITHUB_OUTPUT writes"
    OUTPUTS_OK=1
  fi
done
assert_true "Declared outputs are wired to \$GITHUB_OUTPUT" "$OUTPUTS_OK"

# ---------------------------------------------------------------------------
# 4. PAT is never echoed
# ---------------------------------------------------------------------------
PAT_ECHO_OK=0
# Check for echo of the pat input value
if grep -E 'echo[[:space:]].*inputs\.pat' "$ACTION_FILE" | grep -v '#'; then
  PAT_ECHO_OK=1
fi
# Also check for raw variable echo
if grep -E 'echo[[:space:]].*CORELINK_PAT' "$ACTION_FILE" | grep -v '#'; then
  PAT_ECHO_OK=1
fi
assert_true "PAT (\${{ inputs.pat }}) is never echoed in run steps" "$PAT_ECHO_OK"

# ---------------------------------------------------------------------------
# 5. --json flag is present (required for output parsing)
# ---------------------------------------------------------------------------
JSON_FLAG_OK=0
grep -q -- "--json" "$ACTION_FILE" || JSON_FLAG_OK=1
assert_true "--json flag present in corelink invocation" "$JSON_FLAG_OK"

# ---------------------------------------------------------------------------
# 6. GITHUB_OUTPUT is referenced (output wiring exists)
# ---------------------------------------------------------------------------
GITHUB_OUTPUT_OK=0
grep -q 'GITHUB_OUTPUT' "$ACTION_FILE" || GITHUB_OUTPUT_OK=1
assert_true "\$GITHUB_OUTPUT is written in steps" "$GITHUB_OUTPUT_OK"

# ---------------------------------------------------------------------------
# 7. Exit-2 (attestation/wire error) causes hard failure
# ---------------------------------------------------------------------------
EXIT2_OK=0
grep -q 'RAW_EXIT.*-eq 2\|eq 2.*RAW_EXIT' "$ACTION_FILE" || EXIT2_OK=1
assert_true "exit-2 from corelink (attestation/wire error) causes hard failure" "$EXIT2_OK"

# ---------------------------------------------------------------------------
# 8. verified=false with verify=true causes hard failure
# ---------------------------------------------------------------------------
VERIFY_FAIL_OK=0
grep -q 'verified.*!=.*true\|verified != .true' "$ACTION_FILE" || VERIFY_FAIL_OK=1
assert_true "verified=false with verify=true causes hard step failure" "$VERIFY_FAIL_OK"

# ---------------------------------------------------------------------------
# 9. AU5.11 — input interpolation and execution-surface checks
# ---------------------------------------------------------------------------
# Composite-action expressions are safe in step env maps, but unsafe when
# embedded in a shell/Python run body. Keep this structural assertion separate
# from the execution proof below: a future edit cannot pass by merely retaining
# the expected strings in comments.
INPUT_SURFACE_OK=0
if python3 -c "import yaml" 2>/dev/null; then
  if ! python3 - "$ACTION_FILE" <<'PY'
import re
import sys
import yaml

data = yaml.safe_load(open(sys.argv[1]))
inputs = set(data.get("inputs", {}))
steps = data.get("runs", {}).get("steps", [])
seen_env = set()
bad = []
for step in steps:
    env = step.get("env", {}) or {}
    for value in env.values():
        if isinstance(value, str):
            match = re.fullmatch(r"\s*\$\{\{\s*inputs\.([A-Za-z0-9_-]+)\s*\}\}\s*", value)
            if match:
                seen_env.add(match.group(1))
    body = step.get("run", "")
    if isinstance(body, str):
        for match in re.finditer(r"\$\{\{\s*inputs\.([A-Za-z0-9_-]+)\s*\}\}", body):
            bad.append(f"{step.get('id', step.get('name', '<unnamed>'))}:{match.group(1)}")
missing = sorted(inputs - seen_env)
if bad:
    print("direct input interpolation in run body: " + ", ".join(bad), file=sys.stderr)
if missing:
    print("inputs not supplied through step env: " + ", ".join(missing), file=sys.stderr)
sys.exit(bool(bad or missing))
PY
  then
    INPUT_SURFACE_OK=1
  fi
else
  echo "    NOTE: PyYAML unavailable; structural AU5.11 parser check skipped (fallback checks remain)."
  if grep -Eq '\$\{\{[[:space:]]*inputs\.[A-Za-z0-9_-]+[[:space:]]*\}\}' "$ACTION_FILE"; then
    # Input expressions are allowed only on env: assignment lines in the
    # fallback parser; no run-body line may contain one.
    if grep -E '^      run:|^        [^#].*\$\{\{[[:space:]]*inputs\.' "$ACTION_FILE" | grep -q '\$\{\{'; then
      INPUT_SURFACE_OK=1
    fi
  fi
fi
assert_true "all action inputs enter through step env and none interpolate in run bodies" "$INPUT_SURFACE_OK"

# Execute the actual locate/run/parse/propagate bodies with a stub corelink.
# The harness resolves each step's real env mappings from action.yml, routes
# prior-step outputs through those mappings, and runs every body inside an
# owned private fixture. Every case has unique RUNNER_TEMP/output paths; no
# fixed /tmp path is deleted. AU5.12 remains open: the production parse step
# still requires python3, and this test adds no runtime dependency to the action.
echo "NOTE: AU5.12 (python3 parser dependency) remains open; this validation covers AU5.11 only."
DYNAMIC_OK=0
if python3 -c "import yaml" 2>/dev/null; then
  DYNAMIC_TMP=$(mktemp -d)
  cleanup_dynamic() { rm -rf "$DYNAMIC_TMP"; }
  trap cleanup_dynamic EXIT
  for step_id in locate run parse propagate cleanup; do
    python3 - "$ACTION_FILE" "$step_id" "$DYNAMIC_TMP/$step_id" "$DYNAMIC_TMP/env-$step_id" <<'PY'
import re
import sys
import yaml

action, wanted, body_path, env_path = sys.argv[1:]
data = yaml.safe_load(open(action))
for step in data.get("runs", {}).get("steps", []):
    if step.get("id") != wanted:
        continue
    body = step.get("run")
    if not body:
        raise SystemExit(f"step {wanted} has no run body")
    open(body_path, "w").write(body)
    with open(env_path, "w") as out:
        for key, value in (step.get("env", {}) or {}).items():
            value = str(value)
            input_match = re.fullmatch(r"\$\{\{\s*inputs\.([A-Za-z0-9_-]+)\s*\}\}", value)
            output_match = re.fullmatch(r"\$\{\{\s*steps\.run\.outputs\.([A-Za-z0-9_-]+)\s*\}\}", value)
            if input_match:
                var = "INPUT_" + input_match.group(1).replace("-", "_").upper()
            elif output_match:
                var = "STEP_RUN_" + output_match.group(1).replace("-", "_").upper()
            else:
                raise SystemExit(f"unhandled env mapping {wanted}:{key}={value}")
            out.write(f'export {key}="${{{var}}}"\n')
    break
else:
    raise SystemExit(f"step {wanted} not found")
PY
  done
  mkdir -p "$DYNAMIC_TMP/bin"
  cat > "$DYNAMIC_TMP/bin/corelink" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ -n "${GITHUB_OUTPUT+x}" || -n "${GITHUB_PATH+x}" ]]; then
  printf 'control-file-env-present\n' > "$DYNAMIC_CAPTURE.control-env"
fi
printf 'stub-corelink-executed\n' > "$DYNAMIC_CAPTURE.marker"
printf '%s\0' "$@" > "$DYNAMIC_CAPTURE.argv"
{
  printf 'CORELINK_URL=%s\n' "$CORELINK_URL"
  printf 'CORELINK_PAT=%s\n' "$CORELINK_PAT"
  printf 'CORELINK_CHECK=%s\n' "$CORELINK_CHECK"
  printf 'CORELINK_CHECK_ID=%s\n' "$CORELINK_CHECK_ID"
  printf 'CORELINK_IMAGE=%s\n' "$CORELINK_IMAGE"
  printf 'CORELINK_VERIFY=%s\n' "$CORELINK_VERIFY"
  printf 'CORELINK_VERSION=%s\n' "${CORELINK_VERSION-}"
} > "$DYNAMIC_CAPTURE.env"
printf '{"lease_id":"lease-au5-11","exit":%s,"verified":%s}\n' "$DYNAMIC_RAW_EXIT" "$DYNAMIC_VERIFIED"
exit "$DYNAMIC_RAW_EXIT"
STUB
  chmod +x "$DYNAMIC_TMP/bin/corelink"
  cat > "$DYNAMIC_TMP/bin/id" <<'IDSTUB'
#!/usr/bin/env bash
printf 'id-command-executed\n' > "$DYNAMIC_CAPTURE.id"
printf 'uid=9999(stub)\n'
IDSTUB
  chmod +x "$DYNAMIC_TMP/bin/id"

  dynamic_fail() { echo "    FAIL: $*"; DYNAMIC_OK=1; }
  execute_case() {
    local label="$1" input_name="$2" payload="$3" raw="$4" verified="$5" verify_input="$6"
    local expected_run="$7" expected_parse="$8" expected_propagate="$9"
    local case_dir run_status parse_status propagate_status
    case_dir=$(mktemp -d "$DYNAMIC_TMP/case.XXXXXX")
    mkdir -p "$case_dir/work" "$case_dir/runner-temp"
    export PATH="$DYNAMIC_TMP/bin:$PATH" RUNNER_TEMP="$case_dir/runner-temp"
    export GITHUB_OUTPUT="$case_dir/gh-output" GITHUB_PATH="$case_dir/gh-path"
    export DYNAMIC_CAPTURE="$case_dir/capture" DYNAMIC_RAW_EXIT="$raw" DYNAMIC_VERIFIED="$verified"
    export INPUT_VERSION=0.1.0 INPUT_URL=https://safe.example INPUT_PAT=pat-safe \
      INPUT_CHECK='printf safe' INPUT_CHECK_ID=ci-safe INPUT_IMAGE='' INPUT_VERIFY=true
    case "$input_name" in
      url) INPUT_URL="$payload" ;; pat) INPUT_PAT="$payload" ;;
      check) INPUT_CHECK="$payload" ;; check-id) INPUT_CHECK_ID="$payload" ;;
      image) INPUT_IMAGE="$payload" ;; verify) INPUT_VERIFY="$payload" ;;
      version) INPUT_VERSION="$payload" ;;
    esac
    export INPUT_VERSION INPUT_URL INPUT_PAT INPUT_CHECK INPUT_CHECK_ID INPUT_IMAGE INPUT_VERIFY

    set +e
    (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-locate" && bash "$DYNAMIC_TMP/locate") >"$case_dir/locate.log" 2>&1
    local locate_status=$?
    (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-run" && bash "$DYNAMIC_TMP/run") >"$case_dir/run.log" 2>&1
    run_status=$?
    set -e
    [[ "$locate_status" -eq 0 ]] || dynamic_fail "$label locate status=$locate_status"
    [[ "$run_status" -eq "$expected_run" ]] || dynamic_fail "$label run status=$run_status expected=$expected_run"

    if [[ "$run_status" -eq 0 ]]; then
      export STEP_RUN_RAW_EXIT="$(sed -n 's/^raw_exit=//p' "$GITHUB_OUTPUT")"
      export STEP_RUN_OUTPUT_FILE="$(sed -n 's/^output_file=//p' "$GITHUB_OUTPUT")"
    else
      export STEP_RUN_RAW_EXIT="$(sed -n 's/^raw_exit=//p' "$GITHUB_OUTPUT")"
    fi
    export STEP_RUN_TEMP_DIR="$(sed -n 's/^temp_dir=//p' "$GITHUB_OUTPUT")"
    if [[ "$run_status" -eq 0 ]]; then
      set +e
      (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-parse" && python3 "$DYNAMIC_TMP/parse") >"$case_dir/parse.log" 2>&1
      parse_status=$?
      if [[ "$parse_status" -eq 0 ]]; then
        (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-propagate" && bash "$DYNAMIC_TMP/propagate") >"$case_dir/propagate.log" 2>&1
        propagate_status=$?
      else
        propagate_status=99
      fi
      set -e
      [[ "$parse_status" -eq "$expected_parse" ]] || dynamic_fail "$label parse status=$parse_status expected=$expected_parse"
      [[ "$propagate_status" -eq "$expected_propagate" ]] || dynamic_fail "$label propagate status=$propagate_status expected=$expected_propagate"
    else
      parse_status=99
      propagate_status=99
      if grep -q '^output_file=' "$GITHUB_OUTPUT" 2>/dev/null; then
        dynamic_fail "$label emitted output_file despite hard exit"
      fi
    fi

    set +e
    (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-cleanup" && bash "$DYNAMIC_TMP/cleanup") >"$case_dir/cleanup.log" 2>&1
    local cleanup_status=$?
    set -e
    [[ "$cleanup_status" -eq 0 ]] || dynamic_fail "$label cleanup status=$cleanup_status"
    if find "$RUNNER_TEMP" -mindepth 1 -print -quit | grep -q .; then
      dynamic_fail "$label RUNNER_TEMP was not emptied by the cleanup step"
    fi
    if [[ ! -f "$DYNAMIC_CAPTURE.marker" ]]; then
      dynamic_fail "$label stub did not execute"
    fi
    if [[ -e "$DYNAMIC_CAPTURE.id" ]]; then
      dynamic_fail "$label id command executed"
    fi
    if [[ -e "$DYNAMIC_CAPTURE.control-env" ]]; then
      dynamic_fail "$label corelink inherited GitHub control-file env"
    fi
    [[ -s "$GITHUB_OUTPUT" ]] || dynamic_fail "$label parent GITHUB_OUTPUT was lost"
    if find "$case_dir/work" -name pwned -print -quit | grep -q .; then
      dynamic_fail "$label payload executed shell code"
    fi

    # Negative cleanup matrix: all paths use the required basename but must be
    # rejected unless they are direct children of canonical RUNNER_TEMP.
    outside_parent="$DYNAMIC_TMP/outside-parent"
    sibling_parent="$case_dir/runner-sibling"
    link_target="$case_dir/link-target"
    link_path="$RUNNER_TEMP/corelink.symlink"
    mkdir -p "$outside_parent/corelink.outside" "$sibling_parent/corelink.sibling" "$link_target/corelink.target"
    printf '%s\n' sentinel > "$outside_parent/corelink.outside/sentinel"
    printf '%s\n' sentinel > "$sibling_parent/corelink.sibling/sentinel"
    printf '%s\n' sentinel > "$link_target/corelink.target/sentinel"
    ln -s "$link_target/corelink.target" "$link_path"
    for bad_path in "$outside_parent/corelink.outside" "$sibling_parent/corelink.sibling" "$link_path"; do
      export STEP_RUN_TEMP_DIR="$bad_path"
      set +e
      (cd "$case_dir/work" && source "$DYNAMIC_TMP/env-cleanup" && bash "$DYNAMIC_TMP/cleanup") >"$case_dir/negative-cleanup.log" 2>&1
      cleanup_status=$?
      set -e
      [[ "$cleanup_status" -ne 0 ]] || dynamic_fail "$label accepted invalid cleanup path $bad_path"
    done
    [[ -f "$outside_parent/corelink.outside/sentinel" ]] || dynamic_fail "$label touched outside sentinel"
    [[ -f "$sibling_parent/corelink.sibling/sentinel" ]] || dynamic_fail "$label touched sibling sentinel"
    [[ -L "$link_path" && -f "$link_target/corelink.target/sentinel" ]] || dynamic_fail "$label followed cleanup symlink"
    rm -f -- "$link_path"
    rm -rf -- "$outside_parent" "$sibling_parent" "$link_target"

    if [[ "$expected_run" -eq 0 ]]; then
      [[ -f "$DYNAMIC_CAPTURE.argv" && -f "$DYNAMIC_CAPTURE.env" ]] || dynamic_fail "$label capture missing"
      grep -Fqx "raw_exit=$raw" "$GITHUB_OUTPUT" || dynamic_fail "$label raw_exit output missing"
      grep -Fqx "output_file=$STEP_RUN_OUTPUT_FILE" "$GITHUB_OUTPUT" || dynamic_fail "$label output_file output missing"
      if [[ "$expected_parse" -eq 0 ]]; then
        grep -Fqx "exit=$raw" "$GITHUB_OUTPUT" || dynamic_fail "$label exit output missing"
        grep -Fqx "verified=$verified" "$GITHUB_OUTPUT" || dynamic_fail "$label verified output missing"
        grep -Fqx "lease_id=lease-au5-11" "$GITHUB_OUTPUT" || dynamic_fail "$label lease output missing"
      fi
      if grep -F -- "$INPUT_PAT" "$case_dir"/*.log >/dev/null 2>&1; then
        dynamic_fail "$label PAT leaked to logs"
      fi
      if [[ "$input_name" == version ]] && ! grep -F -- "$payload" "$case_dir/locate.log" >/dev/null 2>&1; then
        dynamic_fail "$label version was not preserved in locate env"
      fi
      python3 - "$input_name" "$payload" "$DYNAMIC_CAPTURE" "$verify_input" <<'PY' || dynamic_fail "$label argv/env mismatch"
import pathlib
import sys

name, payload, prefix, verify_input = sys.argv[1:]
argv = pathlib.Path(prefix + ".argv").read_bytes().split(b"\0")[:-1]
env = dict(line.split("=", 1) for line in pathlib.Path(prefix + ".env").read_text().splitlines())
values = {"url": "https://safe.example", "pat": "pat-safe", "check": "printf safe",
          "check-id": "ci-safe", "image": "", "verify": "true", "version": "0.1.0"}
values[name] = payload
expected_env = {"CORELINK_URL": values["url"], "CORELINK_PAT": values["pat"],
                "CORELINK_CHECK": values["check"], "CORELINK_CHECK_ID": values["check-id"],
                "CORELINK_IMAGE": values["image"], "CORELINK_VERIFY": values["verify"],
                # version belongs to locate's env, not the run step.
                "CORELINK_VERSION": ""}
for key, expected in expected_env.items():
    if env.get(key) != expected:
        raise SystemExit(f"{key} was not preserved in the actual step env")
expected_argv = [b"run", b"--url", values["url"].encode(), b"--check", values["check"].encode(),
                 b"--check-id", values["check-id"].encode(), b"--json"]
if values["image"]:
    expected_argv.extend([b"--image", values["image"].encode()])
if values["verify"] == "false":
    expected_argv.append(b"--no-verify")
if argv != expected_argv:
    raise SystemExit(f"argv mismatch: {argv!r} != {expected_argv!r}")
if verify_input == "false" and b"--no-verify" not in argv:
    raise SystemExit("verify=false did not set --no-verify")
PY
    fi
    rm -rf "$case_dir"
  }

  payloads=('$(id)' '"; touch pwned; #')
  for input_name in url pat check check-id image verify version; do
    for payload in "${payloads[@]}"; do
      execute_case "literal-$input_name" "$input_name" "$payload" 0 true true 0 0 0
    done
  done
  execute_case "exit-one" check 'printf safe' 1 true true 0 0 1
  execute_case "exit-two" check 'printf safe' 2 true true 2 99 99
  execute_case "verified-false" check 'printf safe' 0 false true 0 2 99
  execute_case "verify-false" verify false 0 false false 0 0 0
  trap - EXIT
  cleanup_dynamic
else
  echo "    NOTE: PyYAML unavailable; dynamic AU5.11 execution proof skipped."
  DYNAMIC_OK=1
fi
assert_true "dangerous literals remain literal through argv/env execution surface" "$DYNAMIC_OK"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "Results: $PASS passed, $FAIL failed"

if [[ "$FAIL" -gt 0 ]]; then
  echo -e "${RED}VALIDATION FAILED${NC}"
  exit 1
fi
echo -e "${GREEN}VALIDATION PASSED${NC}"
exit 0
