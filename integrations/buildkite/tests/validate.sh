#!/usr/bin/env bash
# validate.sh — lightweight self-check for integrations/buildkite/
#
# Asserts:
#   1. plugin.yml parses as valid YAML with required top-level sections.
#   2. The command hook exists and is executable.
#   3. The command hook begins with #!/usr/bin/env bash and set -euo pipefail.
#   4. Every configuration option declared in plugin.yml is referenced in
#      the command hook (via BUILDKITE_PLUGIN_CORELINK_* env var names).
#   5. CORELINK_PAT is never echoed in the hook (secret safety).
#   6. The --json flag is present in the hook (required for output parsing).
#   7. exit-2 from corelink causes a hard failure in the hook.
#   8. verified=false with verify=true causes a hard failure in the hook.
#
# Dependency-light: python3 stdlib only. pyyaml is tried first; if absent
# the script falls back to structural grep assertions.
#
# Exit 0 = all assertions passed.
# Exit 1 = one or more assertions failed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_FILE="$SCRIPT_DIR/../plugin.yml"
HOOK_FILE="$SCRIPT_DIR/../hooks/command"

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
# 1. plugin.yml parses as valid YAML with required sections
# ---------------------------------------------------------------------------
YAML_PARSE_OK=0
if python3 -c "import yaml" 2>/dev/null; then
  python3 -c "
import yaml, sys
try:
    data = yaml.safe_load(open('$PLUGIN_FILE'))
    if not isinstance(data, dict):
        print('ERROR: parsed value is not a dict', file=sys.stderr)
        sys.exit(1)
    assert 'name'          in data, 'missing top-level name'
    assert 'description'   in data, 'missing description'
    assert 'author'        in data, 'missing author'
    assert 'requirements'  in data, 'missing requirements'
    assert 'configuration' in data, 'missing configuration'
    cfg = data['configuration']
    assert 'properties'    in cfg,  'missing configuration.properties'
    assert 'required'      in cfg,  'missing configuration.required'
    assert 'check' in cfg['required'], \"'check' must be in required\"
    print('plugin.yml structure OK')
except Exception as e:
    print(f'ERROR: {e}', file=sys.stderr)
    sys.exit(1)
" 2>&1 || YAML_PARSE_OK=1
else
  # Fallback: structural grep
  grep -q "^name:"          "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "^description:"   "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "^author:"        "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "^requirements:"  "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "^configuration:" "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "  required:"     "$PLUGIN_FILE" || YAML_PARSE_OK=1
  grep -q "  - check"       "$PLUGIN_FILE" || YAML_PARSE_OK=1
fi
assert_true "plugin.yml parses as valid YAML with required sections" "$YAML_PARSE_OK"

# ---------------------------------------------------------------------------
# 2. command hook exists and is executable
# ---------------------------------------------------------------------------
HOOK_EXISTS_OK=0
if [[ ! -f "$HOOK_FILE" ]]; then
  echo "    hooks/command file not found at: $HOOK_FILE"
  HOOK_EXISTS_OK=1
elif [[ ! -x "$HOOK_FILE" ]]; then
  echo "    hooks/command exists but is not executable (chmod +x required)"
  HOOK_EXISTS_OK=1
fi
assert_true "hooks/command exists and is executable" "$HOOK_EXISTS_OK"

# ---------------------------------------------------------------------------
# 3. command hook has #!/usr/bin/env bash shebang and set -euo pipefail
# ---------------------------------------------------------------------------
SAFETY_OK=0
if ! head -1 "$HOOK_FILE" | grep -q '#!/usr/bin/env bash\|#!/bin/bash'; then
  echo "    hooks/command does not start with a bash shebang"
  SAFETY_OK=1
fi
if ! grep -q 'set -euo pipefail' "$HOOK_FILE"; then
  echo "    hooks/command missing 'set -euo pipefail'"
  SAFETY_OK=1
fi
assert_true "hooks/command has bash shebang and set -euo pipefail" "$SAFETY_OK"

# ---------------------------------------------------------------------------
# 4. Every configuration option in plugin.yml is referenced in the hook
# ---------------------------------------------------------------------------
# Extract option names from plugin.yml properties block.
OPTIONS_OK=0

# Get property names (url, check, check-id, image, verify)
if python3 -c "import yaml" 2>/dev/null; then
  OPTION_NAMES=$(python3 -c "
import yaml
data = yaml.safe_load(open('$PLUGIN_FILE'))
for k in data['configuration']['properties'].keys():
    print(k)
" 2>/dev/null)
else
  # Fallback: extract lines indented under properties:
  OPTION_NAMES=$(grep -A 50 "^  properties:" "$PLUGIN_FILE" \
    | grep -E "^    [a-z]" | sed 's/://g' | awk '{print $1}' || true)
fi

for opt in $OPTION_NAMES; do
  # Buildkite env var convention: hyphens → underscores, uppercase
  env_suffix=$(echo "$opt" | tr '[:lower:]-' '[:upper:]_')
  env_var="BUILDKITE_PLUGIN_CORELINK_${env_suffix}"
  if ! grep -q "$env_var" "$HOOK_FILE"; then
    echo "    option '$opt' → '$env_var' not referenced in hooks/command"
    OPTIONS_OK=1
  fi
done
assert_true "Every plugin.yml configuration option is referenced in hooks/command" "$OPTIONS_OK"

# ---------------------------------------------------------------------------
# 5. CORELINK_PAT is never echoed in the hook (value, not the name as text)
# ---------------------------------------------------------------------------
# We guard against:
#   echo "$CORELINK_PAT"   echo "${CORELINK_PAT}"   echo $CORELINK_PAT
# We explicitly allow lines that merely mention the variable name in a string
# literal (e.g. error messages like echo "CORELINK_PAT is not set").
# The distinction: a dollar-sign before CORELINK_PAT indicates expansion
# (potential secret leak); bare name is just documentation.
PAT_ECHO_OK=0
if grep -E 'echo[[:space:]].*\$\{?CORELINK_PAT' "$HOOK_FILE" | grep -v '#'; then
  PAT_ECHO_OK=1
fi
assert_true "CORELINK_PAT value is never echoed in hooks/command" "$PAT_ECHO_OK"

# ---------------------------------------------------------------------------
# 6. --json flag is present (required for output parsing)
# ---------------------------------------------------------------------------
JSON_FLAG_OK=0
grep -q -- "--json" "$HOOK_FILE" || JSON_FLAG_OK=1
assert_true "--json flag present in corelink invocation" "$JSON_FLAG_OK"

# ---------------------------------------------------------------------------
# 7. exit-2 from corelink causes hard failure in the hook
# ---------------------------------------------------------------------------
EXIT2_OK=0
grep -q 'RAW_EXIT.*-eq 2\|eq 2.*RAW_EXIT' "$HOOK_FILE" || EXIT2_OK=1
assert_true "exit-2 from corelink (attestation/wire error) causes hard failure" "$EXIT2_OK"

# ---------------------------------------------------------------------------
# 8. verified=false with verify=true causes hard failure
# ---------------------------------------------------------------------------
VERIFY_FAIL_OK=0
grep -q 'verified.*!=.*true\|verified != .true' "$HOOK_FILE" || VERIFY_FAIL_OK=1
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
