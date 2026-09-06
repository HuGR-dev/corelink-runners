#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
python3 - <<'PY'
import json, pathlib, re
root = pathlib.Path('.')
files = ['monitor-foundation.yaml','monitor-runtime.yaml','sensitivity-foundation.yaml','sensitivity-runtime.yaml','verifier-foundation.yaml']
for name in files:
    text = (root/name).read_text()
    if any(x in text for x in ('TODO','PLACEHOLDER','example.com','DeletionPolicy: Delete','UpdateReplacePolicy: Delete')):
        raise SystemExit(f'{name}: forbidden placeholder/destructive marker')
    if 'Subscription:' in text or 'Email' in text or 'email' in text:
        raise SystemExit(f'{name}: subscription/email is forbidden')
m = json.loads((root/'manifest.json').read_text())
assert m['secrets'] is False and m['resource_mutation_performed'] is False
for account in m['accounts'].values(): assert re.fullmatch(r'\d{12}', account)
f = (root/'monitor-foundation.yaml').read_text(); r = (root/'monitor-runtime.yaml').read_text()
assert 'ObjectLockEnabled: Enabled' in f and 'COMPLIANCE' in f and 'DeletionProtectionEnabled: true' in f
assert 'PAY_PER_REQUEST' in f and 'ImageUri' in r and 'AWS::Lambda::Version' in r
assert 'Mode: OFF' in r and 'FunctionVersion' in r
print('infra structural assertions: PASS')
PY
