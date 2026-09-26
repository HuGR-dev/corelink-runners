#!/usr/bin/env python3
"""Fail closed unless RunnerContainer export/import is cache-separated and budgeted."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def fail(message: str) -> int:
    print(f"verify_runner_image_disk_contract: {message}", file=sys.stderr)
    return 1


def verify(workflow_path: Path, builder_path: Path) -> int:
    if not workflow_path.is_file() or not builder_path.is_file():
        return fail("workflow or bounded builder helper is missing")
    workflow = workflow_path.read_text(encoding="utf-8")
    builder = builder_path.read_text(encoding="utf-8")

    if not re.search(r"(?m)^  workflow_dispatch:[ \t]*(?:\{\})?[ \t]*(?:#.*)?$", workflow):
        return fail("production image workflow must remain manual-only")
    if re.search(r"(?m)^  pull_request:[^\n]*$", workflow):
        return fail("production image workflow must not become a pull_request self-hosted lane")

    runner_start = workflow.find("- name: Build + push RunnerContainer image")
    runner_end = workflow.find("- name: Build corelink-check-exec-server", runner_start)
    if runner_start < 0 or runner_end < 0:
        return fail("RunnerContainer build step boundaries are missing")
    runner_step = workflow[runner_start:runner_end]
    if "container-build-export-load.sh" not in runner_step:
        return fail("RunnerContainer does not use the bounded export/import helper")
    if re.search(r"(?m)^\s+docker build(?:\s|$)", runner_step):
        return fail("RunnerContainer still imports directly from docker build")
    if "wrangler containers push" not in runner_step:
        return fail("existing digest-resolving publication step was not preserved")
    if "trap cleanup EXIT" not in runner_step or "--image \"${IMAGE}:${TAG}\" --prune-only" not in runner_step:
        return fail("calling workflow must retain exact-image cleanup after push")

    required = (
        (r"buildctl .* build \\", "BuildKit export command"),
        (r"--output \"type=oci,name=\$image_ref,dest=\$archive,compression=gzip\"", "named OCI archive export"),
        (r"--metadata-file \"\$metadata\"", "BuildKit digest metadata"),
        (r"buildctl .* prune --all --force", "BuildKit cache eviction"),
        (r"verify_runner_image_oci_archive\.py", "OCI blob/digest verifier"),
        (r"uncompressed_layer_bytes", "measured uncompressed layer size"),
        (r"buildkit_bytes <= 268435456", "post-prune cache ceiling"),
        (r"filesystem_bytes >= 17179869184 && filesystem_bytes <= 21474836480", "18 GB box identity boundary"),
        (r"layers_bytes \+ buildkit_bytes \+ reserve_bytes", "pre-import disk budget"),
        (r"free_bytes < required_import_bytes", "fail-closed import budget guard"),
        (r"docker load --input \"\$archive\"", "containerd import after budget guard"),
        (r"loaded_id.*config_digest.*manifest_digest", "import digest preservation check"),
        (r"CORELINK_NIGHTLY=nightly-\$\{nightly_date\}", "nightly runtime contract check"),
        (r"trap cleanup EXIT", "archive/image cleanup trap"),
        (r"rm -rf -- \"\$temp_dir\"", "exact temporary export cleanup"),
        (r"GITHUB_STEP_SUMMARY", "hosted build receipt output"),
        (r"GITHUB_SHA", "source SHA binding"),
    )
    for pattern, description in required:
        if not re.search(pattern, builder, re.MULTILINE):
            return fail(f"missing {description}")

    export = builder.find("--output \"type=oci")
    prune = builder.find("prune --all --force", export)
    verify_archive = builder.find('python3 "$archive_verifier"', prune)
    load = builder.find("docker load --input", verify_archive)
    if min(export, prune, verify_archive, load) < 0 or not (export < prune < verify_archive < load):
        return fail("required order is OCI export → BuildKit prune → digest/budget checks → import")
    budget = builder.find("free_bytes < required_import_bytes", verify_archive)
    if budget < 0 or budget > load:
        return fail("disk budget must be evaluated after archive measurement and before import")
    if re.search(r"(?i)\b(?:buildctl|nerdctl)\s+.*\b(?:push|login)\b", builder):
        return fail("bounded helper must not publish or authenticate to a registry")

    print("verify_runner_image_disk_contract: PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--workflow", required=True, type=Path)
    parser.add_argument("--builder", required=True, type=Path)
    args = parser.parse_args()
    return verify(args.workflow, args.builder)


if __name__ == "__main__":
    raise SystemExit(main())
