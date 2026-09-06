#!/usr/bin/env python3
"""Fail-closed static contract for the runner image build lane.

This is deliberately cheap: it does not contact Cloudflare or build a
container. The workflow performs the real build/push, while this verifier keeps
its PR trigger, digest handoff, disk bounds, and label semantics from drifting.
``--self-test`` applies bounded in-memory mutations and requires each one to be
rejected for the expected reason.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/build-cf-container-images.yml"
DOCKERFILE = ROOT / "deploy/runner/Dockerfile"
WRANGLER = ROOT / "deploy/cloudflare/wrangler.jsonc"


class ContractError(RuntimeError):
    pass


def check(workflow: str, dockerfile: str, wrangler: str) -> None:
    for path in (
        '"deploy/runner/**"',
        '"deploy/check-host/**"',
        '"deploy/cloudflare/Dockerfile.runner-devenv"',
        '"scripts/ci/runner-image-build-preflight.sh"',
        '"scripts/ci/runner-image-build-cleanup.sh"',
        '"scripts/verify_image_build_contract.py"',
        '"tests/test_runner_image_build_preflight.sh"',
        '".github/actionlint.yaml"',
    ):
        if path not in workflow:
            raise ContractError(f"PR trigger path missing: {path}")
    if "  pull_request:\n    paths:" not in workflow or "  workflow_dispatch: {}" not in workflow:
        raise ContractError("image lane must have bounded PR and explicit dispatch triggers")
    if "if: github.event_name != 'pull_request'" not in workflow:
        raise ContractError("registry secret must not be required for PR build-only validation")
    if "PR validation: RunnerContainer built locally; push is dispatch-only" not in workflow:
        raise ContractError("RunnerContainer PR build-only branch missing")
    if "PR validation: CheckHostContainer built locally; push is dispatch-only" not in workflow:
        raise ContractError("CheckHostContainer PR build-only branch missing")
    if "PR validation: RunnerDevEnvDO built locally; push is dispatch-only" not in workflow:
        raise ContractError("RunnerDevEnvDO PR build-only branch missing")
    if "bash scripts/ci/runner-image-build-preflight.sh" not in workflow:
        raise ContractError("disk/tool preflight is not wired before builds")
    if not (
        workflow.index("- name: Install deps (wrangler)")
        < workflow.index("- name: Guard — bounded disk budget before image materialization")
        < workflow.index("- name: Build + push RunnerContainer image")
    ):
        raise ContractError("disk/tool preflight must run after setup and before image builds")
    if workflow.count("bash scripts/ci/runner-image-build-cleanup.sh") != 3:
        raise ContractError("each of the three pushed images must release local build state")
    if workflow.count("scripts/ci/resolve-pushed-ref.sh") != 3:
        raise ContractError("each pushed image must resolve an immutable digest")
    if not re.search(r"(?m)^    runs-on: corelink$", workflow):
        raise ContractError("image lane lost the real corelink runner label")
    if re.search(r"corelink\.(rust|python|gh|clw|sccache|node|pnpm)\.", dockerfile):
        raise ContractError("unconsumed corelink OCI tool labels remain")
    containers_start = wrangler.find('"containers": [')
    if containers_start < 0:
        raise ContractError("Wrangler container list is missing")
    container_block = wrangler[containers_start:]
    if '"class_name": "RunnerDevEnvDO"' in container_block:
        raise ContractError("DevEnv container entry must remain absent until a digest is proven")
    if '"image": "' not in wrangler or not re.search(r'"image": "[^"\n]+@sha256:[0-9a-f]{64}"', wrangler):
        raise ContractError("remaining container image pins must be immutable digests")


@dataclass(frozen=True)
class Mutation:
    name: str
    old: str
    new: str
    expected: str
    target: str


MUTATIONS = (
    Mutation("remove-pr-trigger", "  pull_request:\n    paths:", "  # pull_request removed", "bounded PR and explicit dispatch", "workflow"),
    Mutation("require-pr-secret", "if: github.event_name != 'pull_request'", "if: true", "registry secret must not", "workflow"),
    Mutation("remove-disk-bound", "bash scripts/ci/runner-image-build-preflight.sh", "bash scripts/ci/missing.sh", "disk/tool preflight", "workflow"),
    Mutation("remove-digest-resolution", "scripts/ci/resolve-pushed-ref.sh", "scripts/ci/missing-ref.sh", "immutable digest", "workflow"),
    Mutation("restore-oci-label", "org.opencontainers.image.vendor=\"HuGR / CoreLink\"", "org.opencontainers.image.vendor=\"HuGR / CoreLink\" corelink.node.version=\"unknown\"", "unconsumed corelink OCI tool labels", "dockerfile"),
    Mutation("restore-devenv-entry", "// ⛔ RunnerDevEnvDO's container entry is REMOVED until its image exists.", '"class_name": "RunnerDevEnvDO",\n      "image": "registry.cloudflare.com/corelink-runner-devenv:latest",', "DevEnv container entry", "wrangler"),
)


def self_test() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    dockerfile = DOCKERFILE.read_text(encoding="utf-8")
    wrangler = WRANGLER.read_text(encoding="utf-8")
    for mutation in MUTATIONS:
        if mutation.target == "workflow":
            if workflow.count(mutation.old) != (3 if mutation.name == "remove-digest-resolution" else 1):
                raise ContractError(f"{mutation.name}: target count is not expected")
            mutated = workflow.replace(mutation.old, mutation.new, 1)
            values = (mutated, dockerfile, wrangler)
        elif mutation.target == "dockerfile":
            if dockerfile.count(mutation.old) != 1:
                raise ContractError(f"{mutation.name}: target count is not one")
            values = (workflow, dockerfile.replace(mutation.old, mutation.new, 1), wrangler)
        else:
            if wrangler.count(mutation.old) != 1:
                raise ContractError(f"{mutation.name}: target count is not one")
            values = (workflow, dockerfile, wrangler.replace(mutation.old, mutation.new, 1))
        try:
            check(*values)
        except ContractError as error:
            if mutation.expected not in str(error):
                raise ContractError(f"{mutation.name}: wrong rejection reason: {error}") from error
            print(f"IMAGE CONTRACT MUTATION PASS: {mutation.name} -> {error}")
        else:
            raise ContractError(f"{mutation.name}: weakened contract was accepted")


def main() -> int:
    try:
        values = (
            WORKFLOW.read_text(encoding="utf-8"),
            DOCKERFILE.read_text(encoding="utf-8"),
            WRANGLER.read_text(encoding="utf-8"),
        )
        check(*values)
        if "--self-test" in sys.argv:
            self_test()
    except (OSError, ContractError) as error:
        print(f"IMAGE CONTRACT FAIL: {error}", file=sys.stderr)
        return 1
    print("IMAGE CONTRACT PASS: bounded PR build, immutable dispatch push, labels, and disk preflight verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
