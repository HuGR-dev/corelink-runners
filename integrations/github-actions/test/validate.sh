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
