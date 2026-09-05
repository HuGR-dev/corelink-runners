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
# Each dangerous value is supplied as an environment value, then captured from
# both argv and env by the stub. This proves shell never evaluates $(id) or the
# command-substitution payload, rather than merely grepping for safe-looking
# source text. The action's Python parser dependency remains an AU5.12 item;
# this test does not replace it or add a runtime dependency to the action.
echo "NOTE: AU5.12 (python3 parser dependency) remains open; this validation covers AU5.11 only."
DYNAMIC_OK=0
if python3 -c "import yaml" 2>/dev/null; then
  DYNAMIC_TMP=$(mktemp -d)
  cleanup_dynamic() { rm -rf "$DYNAMIC_TMP"; }
  trap cleanup_dynamic EXIT
  for step_id in locate run parse propagate; do
    python3 - "$ACTION_FILE" "$step_id" "$DYNAMIC_TMP/$step_id" <<'PY'
import sys
import yaml

data = yaml.safe_load(open(sys.argv[1]))
for step in data.get("runs", {}).get("steps", []):
    if step.get("id") == sys.argv[2]:
        body = step.get("run")
        if not body:
            raise SystemExit(f"step {sys.argv[2]} has no run body")
        open(sys.argv[3], "w").write(body)
        break
else:
    raise SystemExit(f"step {sys.argv[2]} not found")
PY
  done
  mkdir -p "$DYNAMIC_TMP/bin"
  cat > "$DYNAMIC_TMP/bin/corelink" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\0' "$@" > "$CORELINK_CAPTURE.argv"
{
  printf 'CORELINK_URL=%s\n' "$CORELINK_URL"
  printf 'CORELINK_PAT=%s\n' "$CORELINK_PAT"
  printf 'CORELINK_CHECK=%s\n' "$CORELINK_CHECK"
  printf 'CORELINK_CHECK_ID=%s\n' "$CORELINK_CHECK_ID"
  printf 'CORELINK_IMAGE=%s\n' "$CORELINK_IMAGE"
  printf 'CORELINK_VERIFY=%s\n' "$CORELINK_VERIFY"
  printf 'CORELINK_VERSION=%s\n' "$CORELINK_VERSION"
} > "$CORELINK_CAPTURE.env"
printf '%s\n' '{"lease_id":"lease-au5-11","exit":0,"verified":true}'
STUB
  chmod +x "$DYNAMIC_TMP/bin/corelink"

  DYNAMIC_OK=0
  export RAW_EXIT=0
  for input_name in url pat check check-id image verify version; do
    for payload in '$(id)' '"; touch pwned; #'; do
      rm -f "$DYNAMIC_TMP/capture.argv" "$DYNAMIC_TMP/capture.env" "$DYNAMIC_TMP/output" "$DYNAMIC_TMP/gh-output" \
        /tmp/corelink_output.json /tmp/corelink_stderr
      export PATH="$DYNAMIC_TMP/bin:$PATH"
      export CORELINK_VERSION=0.1.0 CORELINK_URL=https://safe.example CORELINK_PAT=pat-safe \
        CORELINK_CHECK='printf safe' CORELINK_CHECK_ID=ci-safe CORELINK_IMAGE='' CORELINK_VERIFY=true
      case "$input_name" in
        url) CORELINK_URL="$payload" ;;
        pat) CORELINK_PAT="$payload" ;;
        check) CORELINK_CHECK="$payload" ;;
        check-id) CORELINK_CHECK_ID="$payload" ;;
        image) CORELINK_IMAGE="$payload" ;;
        verify) CORELINK_VERIFY="$payload" ;;
        version) CORELINK_VERSION="$payload" ;;
      esac
      export CORELINK_CAPTURE="$DYNAMIC_TMP/capture"
      export GITHUB_OUTPUT="$DYNAMIC_TMP/gh-output"
      export GITHUB_PATH="$DYNAMIC_TMP/gh-path"
      if ! bash "$DYNAMIC_TMP/locate" >/dev/null 2>&1; then
        echo "    locate step failed for input $input_name payload $payload"
        DYNAMIC_OK=1
        continue
      fi
      if ! bash "$DYNAMIC_TMP/run" >/dev/null 2>&1; then
        echo "    run step failed for input $input_name payload $payload"
        DYNAMIC_OK=1
        continue
      fi
      if ! python3 "$DYNAMIC_TMP/parse" >/dev/null 2>&1; then
        echo "    parse step failed for input $input_name payload $payload"
        DYNAMIC_OK=1
        continue
      fi
      if ! bash "$DYNAMIC_TMP/propagate" >/dev/null 2>&1; then
        echo "    propagate step failed for input $input_name payload $payload"
        DYNAMIC_OK=1
        continue
      fi
      if [[ -e pwned ]]; then
        echo "    payload executed shell code for input $input_name"
        DYNAMIC_OK=1
      fi
      if ! python3 - "$input_name" "$payload" "$DYNAMIC_TMP/capture" <<'PY'
import pathlib
import sys

name, payload, prefix = sys.argv[1:]
argv = pathlib.Path(prefix + ".argv").read_bytes().split(b"\0")[:-1]
env = dict(line.split("=", 1) for line in pathlib.Path(prefix + ".env").read_text().splitlines())
expected_env = {"url": "CORELINK_URL", "pat": "CORELINK_PAT", "check": "CORELINK_CHECK",
                "check-id": "CORELINK_CHECK_ID", "image": "CORELINK_IMAGE",
                "verify": "CORELINK_VERIFY", "version": "CORELINK_VERSION"}[name]
if env[expected_env] != payload:
    raise SystemExit(f"{expected_env} was not preserved in env")
if name == "url" and argv[argv.index(b"--url") + 1].decode() != payload:
    raise SystemExit("url was not preserved in argv")
if name == "check" and argv[argv.index(b"--check") + 1].decode() != payload:
    raise SystemExit("check was not preserved in argv")
if name == "check-id" and argv[argv.index(b"--check-id") + 1].decode() != payload:
    raise SystemExit("check-id was not preserved in argv")
if name == "image" and argv[argv.index(b"--image") + 1].decode() != payload:
    raise SystemExit("image was not preserved in argv")
PY
      then
        DYNAMIC_OK=1
      fi
    done
  done
  rm -f pwned /tmp/corelink_output.json /tmp/corelink_stderr
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
