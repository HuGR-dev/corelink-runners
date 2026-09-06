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
DEVENV_DOCKERFILE = ROOT / "deploy/cloudflare/Dockerfile.runner-devenv"
WRANGLER = ROOT / "deploy/cloudflare/wrangler.jsonc"


class ContractError(RuntimeError):
    pass


def check(workflow: str, dockerfile: str, wrangler: str, devenv_dockerfile: str) -> None:
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
    if workflow.count("if: github.event_name != 'pull_request'") != 3:
        raise ContractError("each registry push must be gated away from pull_request")
    step_blocks = re.findall(r"(?ms)^      - name: ([^\n]+)\n(.*?)(?=^      - name: |\Z)", workflow)
    for name, body in step_blocks:
        if "secrets.CLOUDFLARE_API_TOKEN" in body and not name.startswith("Push "):
            raise ContractError("registry token must be injected only in push steps")
    if workflow.count("CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}") != 3:
        raise ContractError("registry token must be injected only in the three push steps")
    for image in ("RunnerContainer", "CheckHostContainer", "RunnerDevEnvDO"):
        if f"- name: Build {image} image" not in workflow:
            raise ContractError(f"{image} PR build step missing")
        if f"- name: Push {image} image" not in workflow:
            raise ContractError(f"{image} push step missing")
    if "bash scripts/ci/runner-image-build-preflight.sh" not in workflow:
        raise ContractError("disk/tool preflight is not wired before builds")
    if not (
        workflow.index("- name: Install deps (wrangler)")
        < workflow.index("- name: Guard — bounded disk budget before image materialization")
        < workflow.index("- name: Build RunnerContainer image")
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
    debian_pin = "FROM debian:12-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171"
    rust_pin = "FROM rust:1.82-alpine@sha256:2f42ce0d00c0b14f7fd84453cdc93ff5efec5da7ce03ead6e0b41adb1fbe834e"
    if devenv_dockerfile.count(debian_pin) != 3 or "FROM debian:12-slim AS" in devenv_dockerfile:
        raise ContractError("RunnerDevEnv Debian base must remain pinned to the recorded official digest")
    if devenv_dockerfile.count(rust_pin) != 1 or "FROM rust:1.82-alpine AS" in devenv_dockerfile:
        raise ContractError("RunnerDevEnv Rust builder base must remain pinned to the recorded official digest")
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
    Mutation("remove-push-gate", "if: github.event_name != 'pull_request'", "if: always()", "each registry push must be gated", "workflow"),
    Mutation("inject-secret-into-build", "- name: Build RunnerContainer image", "- name: Build RunnerContainer image\n        env:\n          CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}", "only in push steps", "workflow"),
    Mutation("remove-disk-bound", "bash scripts/ci/runner-image-build-preflight.sh", "bash scripts/ci/missing.sh", "disk/tool preflight", "workflow"),
    Mutation("remove-digest-resolution", "scripts/ci/resolve-pushed-ref.sh", "scripts/ci/missing-ref.sh", "immutable digest", "workflow"),
    Mutation("restore-oci-label", "org.opencontainers.image.vendor=\"HuGR / CoreLink\"", "org.opencontainers.image.vendor=\"HuGR / CoreLink\" corelink.node.version=\"unknown\"", "unconsumed corelink OCI tool labels", "dockerfile"),
    Mutation("restore-devenv-entry", "// ⛔ RunnerDevEnvDO's container entry is REMOVED until its image exists.", '"class_name": "RunnerDevEnvDO",\n      "image": "registry.cloudflare.com/corelink-runner-devenv:latest",', "DevEnv container entry", "wrangler"),
    Mutation("unpin-devenv-debian", "FROM debian:12-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS clw-download", "FROM debian:12-slim AS clw-download", "Debian base must remain pinned", "devenv"),
    Mutation("unpin-devenv-rust", "FROM rust:1.82-alpine@sha256:2f42ce0d00c0b14f7fd84453cdc93ff5efec5da7ce03ead6e0b41adb1fbe834e AS exec-builder", "FROM rust:1.82-alpine AS exec-builder", "Rust builder base must remain pinned", "devenv"),
)


def self_test() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    dockerfile = DOCKERFILE.read_text(encoding="utf-8")
    devenv_dockerfile = DEVENV_DOCKERFILE.read_text(encoding="utf-8")
    wrangler = WRANGLER.read_text(encoding="utf-8")
    for mutation in MUTATIONS:
        if mutation.target == "workflow":
            expected_count = 3 if mutation.name in {"remove-digest-resolution", "remove-push-gate"} else 1
            if workflow.count(mutation.old) != expected_count:
                raise ContractError(f"{mutation.name}: target count is not expected")
            mutated = workflow.replace(mutation.old, mutation.new, 1)
            values = (mutated, dockerfile, wrangler, devenv_dockerfile)
        elif mutation.target == "dockerfile":
            if dockerfile.count(mutation.old) != 1:
                raise ContractError(f"{mutation.name}: target count is not one")
            values = (workflow, dockerfile.replace(mutation.old, mutation.new, 1), wrangler, devenv_dockerfile)
        elif mutation.target == "devenv":
            if devenv_dockerfile.count(mutation.old) != 1:
                raise ContractError(f"{mutation.name}: target count is not one")
            values = (workflow, dockerfile, wrangler, devenv_dockerfile.replace(mutation.old, mutation.new, 1))
        else:
            if wrangler.count(mutation.old) != 1:
                raise ContractError(f"{mutation.name}: target count is not one")
            values = (workflow, dockerfile, wrangler.replace(mutation.old, mutation.new, 1), devenv_dockerfile)
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
            DEVENV_DOCKERFILE.read_text(encoding="utf-8"),
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
