#!/usr/bin/env bash
# Focused static contract checks for the hosted, build-only RunnerDevEnv route.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
workflow="${repo}/.github/workflows/build-cf-container-images.yml"

python3 - "${workflow}" <<'PY'
import re
import sys
from pathlib import Path


def reject(message):
    raise ValueError(message)


def job_block(workflow, name):
    matches = list(re.finditer(r"(?m)^  ([a-z0-9-]+):[ \t]*$", workflow))
    start = next((match for match in matches if match.group(1) == name), None)
    if start is None:
        reject(f"missing job: {name}")
    end = next((match.start() for match in matches if match.start() > start.start()), len(workflow))
    return workflow[start.start():end]


def validate(workflow):
    trigger_start = workflow.find("on:\n")
    permissions_start = workflow.find("\npermissions:", trigger_start)
    if trigger_start < 0 or permissions_start < 0:
        reject("workflow trigger boundary is missing")
    trigger = workflow[trigger_start:permissions_start]
    if "workflow_dispatch:" not in trigger or "pull_request:" in trigger or "push:" in trigger:
        reject("image workflow must remain manual-only")
    if "- production-publish" not in trigger or "- devenv-build-only" not in trigger:
        reject("workflow_dispatch must expose both explicit operation modes")
    if "expected_source_sha:" not in trigger:
        reject("manual DevEnv build mode must accept an expected full source SHA")

    guard = job_block(workflow, "validate-dispatch")
    if "runs-on: ubuntu-latest" not in guard:
        reject("dispatch validation must use GitHub-hosted Linux")
    if '"refs/heads/main"' not in guard or "EXPECTED_SOURCE_SHA" not in guard or "GITHUB_SHA" not in guard:
        reject("dispatch guard must bind production to main and DevEnv proof to exact SHA")

    publish = job_block(workflow, "build-and-push")
    if "needs: validate-dispatch" not in publish:
        reject("production publisher must wait for dispatch validation")
    if "inputs.operation == 'production-publish'" not in publish or "github.ref == 'refs/heads/main'" not in publish:
        reject("self-hosted publisher must require explicit production intent on main")
    if "runs-on: corelink" not in publish or "container-build-export-load.sh" not in publish:
        reject("existing production runner build and #575 bounded path must remain intact")

    hosted = job_block(workflow, "devenv-build-only")
    if "needs: validate-dispatch" not in hosted or "inputs.operation == 'devenv-build-only'" not in hosted:
        reject("hosted build must be reachable only after validated build-only dispatch")
    if "runs-on: ubuntu-latest" not in hosted:
        reject("DevEnv build proof must use GitHub-hosted Linux")
    if "ref: ${{ github.sha }}" not in hosted or "persist-credentials: false" not in hosted:
        reject("hosted job must checkout the exact dispatch SHA without persisted credentials")
    if "runner-devenv-build-contract.selftest.sh" not in hosted:
        reject("hosted build must run the focused contract test")
    for required in (
        "EXPECTED_SOURCE_SHA",
        'test "${EXPECTED_SOURCE_SHA}" = "${GITHUB_SHA}"',
        'test "$(git rev-parse HEAD)" = "${GITHUB_SHA}"',
        "deploy/cloudflare/Dockerfile.runner-devenv",
        "cp Cargo.toml Cargo.lock",
        "cp -R crates",
        "deploy/cloudflare/entrypoint.sh deploy/cloudflare/supervisord.conf",
        "docker buildx build",
        "--provenance=mode=max",
        'type=oci,name=${IMAGE_REF},dest=${ARCHIVE}',
        "--metadata-file",
        "def verify_blob(archive, digest)",
        "containerimage.digest",
        "vnd.docker.reference.type",
        "slsa.dev/provenance/",
        "org.opencontainers.image.revision",
        "GITHUB_STEP_SUMMARY",
    ):
        if required not in hosted:
            reject(f"hosted DevEnv digest/provenance proof is missing: {required}")
    forbidden = (
        "secrets.",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "wrangler containers push",
        "wrangler deploy",
        "docker push",
        "runs-on: corelink",
    )
    for value in forbidden:
        if value in hosted:
            reject(f"hosted DevEnv proof contains a forbidden publish/provider path: {value}")


source = Path(sys.argv[1]).read_text(encoding="utf-8")
validate(source)

# Negative cases ensure the assertions fail closed on the routing and evidence
# regressions that would otherwise make this proof unsafe or non-reproducible.
mutations = (
    ("missing main publication boundary", "build-and-push", "github.ref == 'refs/heads/main'", "github.ref == 'refs/heads/other'"),
    ("self-hosted DevEnv build", "devenv-build-only", "runs-on: ubuntu-latest", "runs-on: corelink"),
    ("missing provenance", "devenv-build-only", "--provenance=mode=max", "--provenance=disabled"),
    ("missing exact-SHA bind", "devenv-build-only", 'test \"${EXPECTED_SOURCE_SHA}\" = \"${GITHUB_SHA}\"', "true"),
    ("hosted registry publication", "devenv-build-only", 'type=oci,name=${IMAGE_REF},dest=${ARCHIVE}', "docker push"),
    ("missing workflow selftest", "devenv-build-only", "runner-devenv-build-contract.selftest.sh", "runner-devenv-contract-missing.sh"),
)
for label, job, before, after in mutations:
    block = job_block(source, job)
    if before not in block:
        reject(f"selftest mutation anchor missing: {label}")
    mutated_block = block.replace(before, after, 1)
    mutated = source.replace(block, mutated_block, 1)
    try:
        validate(mutated)
    except ValueError:
        continue
    reject(f"negative contract case passed unexpectedly: {label}")

print(f"runner-devenv-build-contract.selftest: PASS ({len(mutations) + 1} bounded contract cases)")
PY
