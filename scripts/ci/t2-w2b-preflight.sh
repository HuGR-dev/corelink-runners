#!/usr/bin/env bash
# T2-W2b local preflight. Read-only: never deploys, pushes, logs in, or invokes
# the live probe. Missing or ambiguous external gates are always RED.
set -Eeuo pipefail
IFS=$'\n\t'

usage() {
  cat <<'EOF'
usage: scripts/ci/t2-w2b-preflight.sh [--output PATH | --self-test]

Inspect local credentials, pins, prerequisite artifacts, and existing probe
wiring. PATH is written only when supplied; '-' (the default) writes the
sanitized evidence/v1 report to stdout.
EOF
}

root="$(cd -- "$(dirname -- "$0")/../.." && pwd)"
output=-
mode=run
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; output=$2; shift 2 ;;
    --self-test) mode=self-test; shift ;;
    --help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

python3 - "$root" "$output" "$mode" <<'PY'
import datetime as dt
import json
import os
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
output = sys.argv[2]
mode = sys.argv[3]

def parse_jsonc(text):
    """Parse the small JSONC subset used by Wrangler, ignoring comments only
    outside quoted strings and allowing trailing commas."""
    out, i, quote, escaped = [], 0, False, False
    while i < len(text):
        c = text[i]
        if quote:
            out.append(c)
            if escaped: escaped = False
            elif c == "\\": escaped = True
            elif c == '"': quote = False
            i += 1
            continue
        if c == '"': quote = True; out.append(c); i += 1; continue
        if text.startswith("//", i):
            i = text.find("\n", i)
            if i < 0: break
            continue
        if text.startswith("/*", i):
            end = text.find("*/", i + 2)
            if end < 0: raise ValueError("unterminated JSONC comment")
            i = end + 2
            continue
        out.append(c); i += 1
    return json.loads(re.sub(r",(\s*[}\]])", r"\1", "".join(out)))

def actual_images(path):
    try:
        value = parse_jsonc(path.read_text(encoding="utf-8"))
    except (OSError, ValueError, json.JSONDecodeError):
        return {}
    containers = value.get("containers", []) if isinstance(value, dict) else []
    return {str(item.get("class_name")): item.get("image")
            for item in containers if isinstance(item, dict) and item.get("class_name")}

def valid_runner_ref(value):
    return bool(re.fullmatch(r"registry\.cloudflare\.com/[^\s@\"]+@sha256:[0-9a-f]{64}", value or ""))

if mode == "self-test":
    import tempfile
    immutable = "registry.example/runner@sha256:" + "a" * 64
    def images(text):
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d) / "wrangler.jsonc"; p.write_text(text)
            return actual_images(p)
    cases = {
        "comment_stale": ('// "image": "registry.example/runner@sha256:' + "b" * 64 + '"\n{"containers":[]} ', {}),
        "devenv_absent": ('{"containers":[{"class_name":"RunnerContainer","image":"' + immutable + '"}]}', {"RunnerContainer": immutable}),
        "devenv_mutable": ('{"containers":[{"class_name":"RunnerDevEnvDO","image":"registry.example/devenv:latest"}]}', {"RunnerDevEnvDO":"registry.example/devenv:latest"}),
        "devenv_immutable": ('{"containers":[{"class_name":"RunnerDevEnvDO","image":"' + immutable + '"}]}', {"RunnerDevEnvDO":immutable}),
    }
    for name, (text, expected) in cases.items():
        got = images(text)
        if got != expected: raise SystemExit(f"{name}: parser mismatch: {got!r}")
    if valid_runner_ref("fake") or valid_runner_ref(immutable.replace("registry.example", "registry.cloudflare.com/acct")) is False:
        raise SystemExit("runner ref validator fixture failed")
    print("t2-w2b-preflight selftest: PASS (JSONC comment/absent/mutable/immutable fixtures)")
    raise SystemExit(0)

def git(*args):
    p = subprocess.run(["git", "-C", str(root), *args], text=True,
                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    return p.stdout.strip() if p.returncode == 0 else ""

commit = git("rev-parse", "HEAD")
if not re.fullmatch(r"[0-9a-f]{40}", commit):
    raise SystemExit("cannot determine source commit")

config_path = root / "deploy/cloudflare/wrangler.jsonc"
fabricd_path = root / "deploy/cloudflare-fabricd/wrangler.jsonc"
images = actual_images(config_path) if config_path.is_file() else {}
fabricd_images = actual_images(fabricd_path) if fabricd_path.is_file() else {}
runner_ref = images.get("RunnerContainer")
checkhost_ref = images.get("CheckHostContainer")
devenv_ref = images.get("RunnerDevEnvDO")
fabricd_ref = fabricd_images.get("FabricdContainer")
digest_ref = re.compile(r"^registry\.cloudflare\.com/[^\s@\"]+@sha256:([0-9a-f]{64})$")
runner = digest_ref.fullmatch(runner_ref or "")
checkhost = digest_ref.fullmatch(checkhost_ref or "")
fabricd_digest = digest_ref.fullmatch(fabricd_ref or "")

required = {
    "CLOUDFLARE_API_TOKEN": bool(os.getenv("CLOUDFLARE_API_TOKEN")),
    "CLOUDFLARE_ACCOUNT_ID": bool(os.getenv("CLOUDFLARE_ACCOUNT_ID")),
    "FABRIC_INTROSPECT_AUTH_KEY": bool(os.getenv("FABRIC_INTROSPECT_AUTH_KEY")),
    "FLEET_BUSY_READ_KEY": bool(os.getenv("FLEET_BUSY_READ_KEY")),
    "CONTAINER_APP_ID": bool(os.getenv("CONTAINER_APP_ID")),
    "EXPECTED_RUNNER_IMAGE": bool(os.getenv("EXPECTED_RUNNER_IMAGE")),
}
expected = os.getenv("EXPECTED_RUNNER_IMAGE", "")
if not re.fullmatch(r"registry\.cloudflare\.com/[^\s@\"]+@sha256:[0-9a-f]{64}", expected):
    blockers_expected = "EXPECTED_RUNNER_IMAGE is absent or malformed"
elif expected != runner_ref:
    blockers_expected = "EXPECTED_RUNNER_IMAGE does not exactly match configured RunnerContainer image"
else:
    blockers_expected = ""
files = {
    "t8_w4b_probe": root / "scripts/ops/t8-w4b-version-bound-probe.sh",
    "ref_resolver": root / "scripts/ci/resolve-pushed-ref.sh",
    "image_workflow": root / ".github/workflows/build-cf-container-images.yml",
    "freshness_schema": root / "docs/plan/evidence/schema-v1.json",
    "canonical_deploy_artifact": root / "docs/plan/evidence/T2-W2b-deploy.json",
}
checks = {
    "T2-W1a": files["image_workflow"].is_file(),
    "T3-W18": (root / "docs/plan/evidence/T3-W18-containment-live.json").is_file(),
    "T8-W4a": (root / "deploy/runner/entrypoint.sh").is_file(),
    "T8-W4b": (root / "deploy/cloudflare/entrypoint.sh").is_file(),
    "T7-W4b": files["freshness_schema"].is_file(),
    "probe_wiring": all(p.is_file() for k, p in files.items() if k != "canonical_deploy_artifact"),
}
blockers = [f"missing local credential/input: {n}" for n, present in required.items() if not present]
if blockers_expected:
    blockers.append(blockers_expected)
blockers.append("provider proof unavailable: local configuration/credentials cannot establish live deployment acceptance")
if not runner or not checkhost or not fabricd_digest:
    blockers.append("one or more immutable image digests are absent from Wrangler config")
if not devenv_ref:
    blockers.append("O-DEVENV-PIN unresolved: RunnerDevEnvDO image is absent")
elif not digest_ref.fullmatch(devenv_ref):
    blockers.append("O-DEVENV-PIN unresolved: RunnerDevEnvDO uses a mutable image reference")
if not checks["T3-W18"]:
    blockers.append("T3-W18 evidence artifact is absent")
if not checks["probe_wiring"]:
    blockers.append("required T2-W2b probe/workflow/schema wiring is incomplete")
if files["canonical_deploy_artifact"].exists():
    blockers.append("canonical T2-W2b deploy artifact already exists; inspect it separately")

report = {
    "schema_version": "evidence/v1",
    "artifact_id": "t2-w2b-local-preflight",
    "kind": "test+probe",
    "status": "PASS" if not blockers else "RED",
    "observed_at": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "source": {"repository": "corelink-runners", "commit_sha": commit,
               "path": "docs/plan/evidence/T2-W2b-deploy.json"},
    "claims": ["T2-W2b"],
    "version": {"id": commit, **({"digest": "sha256:" + runner.group(1)} if runner else {})},
    "evidence": {
        "mode": "local-read-only-preflight",
        "credentials_present": required,
        "prerequisite_files": {k: v.is_file() for k, v in files.items()},
        "prerequisite_source_paths": checks,
        "config": {
            "account_id_present": bool(os.getenv("CLOUDFLARE_ACCOUNT_ID")),
            "runner_image_ref": runner_ref,
            "checkhost_image_ref": checkhost_ref,
            "fabricd_image_ref": fabricd_ref,
            "devenv_image_ref": devenv_ref,
            "expected_runner_image_present": bool(expected),
            "expected_runner_image_matches_config": bool(expected and expected == runner_ref),
            "provider_proof": False,
        },
        "blockers": blockers,
        "live_probe_invoked": False,
        "deploy_invoked": False,
    },
    "notes": "A local preflight cannot grant live deployment acceptance; missing or mutable external prerequisites are RED.",
}
payload = json.dumps(report, indent=2) + "\n"
if output == "-":
    print(payload, end="")
else:
    destination = pathlib.Path(output)
    if not destination.is_absolute():
        destination = root / destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(payload, encoding="utf-8")
    print(f"wrote {destination}")
sys.exit(0 if not blockers else 1)
PY
