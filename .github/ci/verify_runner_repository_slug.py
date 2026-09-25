#!/usr/bin/env python3
"""Verify the active runner-repository slug and preserve only classified history/fixtures."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path


CANONICAL = "HuGR-dev/corelink-runners"
LEGACY = "HuGR-Labs/corelink-runners"

ACTIVE_REFERENCE_FILES = {
    ".github/ci/verify_runner_repository_slug.py",
    "actions/corelink-memoize/README.md",
    "deploy/RUNBOOK.md",
    "deploy/autoscaler-stage-b.md",
    "deploy/cloudflare-fabricd/wrangler.jsonc",
    "deploy/cloudflare/src/index.ts",
    "deploy/cloudflare/wrangler.jsonc",
    "deploy/northflank-service.json",
    "docs/deploy/fabric-server.md",
    "docs/runbook/cloudflare-go-live.md",
    "docs/runbook/dogfood-go-live.md",
    "docs/runbook/incident-playbook.md",
    "docs/runbook/runner-image-rollout.md",
    "docs/runbook/secret-inventory.md",
    "integrations/buildkite/README.md",
    "integrations/buildkite/plugin.yml",
    "integrations/github-actions/README.md",
    "integrations/github-actions/action.yml",
    "integrations/github-actions/examples/ci.yml",
    "sdk/python/pyproject.toml",
    "sdk/typescript/package.json",
}

# These old-slug fixtures model request payloads and fail-closed legacy cases.
# Keep them stable; this issue migrates live configuration and operating docs.
FIXTURE_FILES = {
    "crates/corelink-fabric-server/src/admission.rs",
    "crates/corelink-fabric-server/src/handlers/test_mint.rs",
    "crates/corelink-fabric-server/src/handlers/webhook.rs",
    "crates/corelink-fabric-server/src/runner_broker.rs",
    "crates/corelink-fabric-server/tests/acceptance_moat.rs",
    "crates/corelink-fabric-server/tests/acceptance_runner_lease.rs",
    "crates/corelink-fabric-server/tests/cloudflare_flip_e2e.rs",
    "crates/corelink-fabric-server/tests/hybrid_flip_e2e.rs",
    "deploy/cloudflare/test/check-host.test.ts",
    "deploy/cloudflare/test/fleet-busy-read.test.ts",
    "deploy/cloudflare/test/index.test.ts",
    "deploy/cloudflare/test/option-c-pat-dispatch.test.ts",
    "deploy/cloudflare/test/reconciler-allowlist.test.ts",
    "deploy/cloudflare/test/webhook-installation-allowlist.test.ts",
    "scripts/e2e/journey/door-a-spawn.sh",
}

# Dated records remain an immutable history of the pre-transfer state.
HISTORY_FILES = {
    "CHANGELOG.md",
    "CLAUDE.md",
    "docs/handoff/2026-09-05-session-state.md",
    "docs/plan/evidence/T3-W18-containment-live.json",
    "docs/plan/execution/2026-09-06-git-closeout-inventory.json",
}


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def require_reconciler_repos(value: str) -> None:
    repos = value.split()
    if CANONICAL not in repos:
        raise ValueError(f"{CANONICAL} is not an exact RECONCILER_REPOS entry")
    if LEGACY in repos:
        raise ValueError(f"legacy runner slug remains in RECONCILER_REPOS: {LEGACY}")
    if "HuGR-Labs/corelink-server" not in repos:
        raise ValueError("the unrelated corelink-server reconciler entry was dropped")


def lookup_installation(mapping: dict[str, object], repository: str) -> str:
    value = mapping.get(repository, "")
    if isinstance(value, str):
        return value
    # Match the deployed JS lookup: numeric IDs are stringified, while objects,
    # booleans, null, and other values resolve to an empty ID.
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return str(value)
    return ""


def tracked_paths(root: Path, own_path: str) -> set[str]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "-z"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
    )
    paths = {item.decode("utf-8") for item in result.stdout.split(b"\0") if item}
    paths.add(own_path)
    return paths


def verify(root: Path) -> None:
    own_path = Path(__file__).resolve().relative_to(root).as_posix()
    paths = tracked_paths(root, own_path)

    missing_active = sorted(ACTIVE_REFERENCE_FILES - paths)
    if missing_active:
        fail(f"active allowlist paths are missing: {missing_active}")

    for relative in sorted(ACTIVE_REFERENCE_FILES):
        content = (root / relative).read_bytes()
        if CANONICAL.encode() not in content:
            fail(f"canonical slug is absent from active path {relative}")
        if relative != own_path and LEGACY.encode() in content:
            fail(f"legacy slug remains in active path {relative}")

    config = (root / "deploy/cloudflare/wrangler.jsonc").read_text(encoding="utf-8")
    reconciler_match = re.search(
        r'^\s*"RECONCILER_REPOS"\s*:\s*"([^"]*)"\s*,?$',
        config,
        re.MULTILINE,
    )
    if not reconciler_match:
        fail("RECONCILER_REPOS was not found in the shipped Wrangler config")
    try:
        require_reconciler_repos(reconciler_match.group(1))
    except ValueError as error:
        fail(str(error))

    map_match = re.search(
        r'^\s*"REPO_INSTALLATION_MAP"\s*:\s*("(?:\\.|[^"\\])*")\s*,?$',
        config,
        re.MULTILINE,
    )
    if not map_match:
        fail("REPO_INSTALLATION_MAP was not found in the shipped Wrangler config")
    try:
        mapping = json.loads(json.loads(map_match.group(1)))
    except (json.JSONDecodeError, TypeError) as error:
        fail(f"REPO_INSTALLATION_MAP is not valid encoded JSON: {error}")
    if not isinstance(mapping, dict):
        fail("REPO_INSTALLATION_MAP must decode to an object")
    canonical_installation = lookup_installation(mapping, CANONICAL)
    if canonical_installation != "150584374":
        fail("canonical runner slug does not resolve to the recorded pre-transfer installation ID")
    if lookup_installation(mapping, LEGACY):
        fail("legacy runner slug still resolves through REPO_INSTALLATION_MAP")

    # Mutation controls prove that the same assertions reject the retired key.
    try:
        require_reconciler_repos(f"{CANONICAL} HuGR-Labs/corelink-server {LEGACY}")
    except ValueError:
        pass
    else:
        fail("RECONCILER_REPOS negative control accepted the legacy runner slug")
    if lookup_installation({LEGACY: "old-installation"}, CANONICAL):
        fail("installation-map negative control accepted a legacy-only key")
    if lookup_installation({CANONICAL: {"unexpected": 1}}, CANONICAL):
        fail("installation-map negative control accepted an object value")
    if lookup_installation({CANONICAL: 150584374}, CANONICAL) != "150584374":
        fail("installation-map positive control rejected the runtime-supported numeric ID")

    allowed_legacy_paths = FIXTURE_FILES | HISTORY_FILES | {own_path}
    actual_legacy_paths = {
        relative
        for relative in paths
        if (root / relative).is_file()
        and LEGACY.encode() in (root / relative).read_bytes()
    }
    unexpected = sorted(actual_legacy_paths - allowed_legacy_paths)
    missing_retained = sorted((FIXTURE_FILES | HISTORY_FILES) - actual_legacy_paths)
    if unexpected:
        fail(f"legacy slug escaped the fixture/history allowlist: {unexpected}")
    if missing_retained:
        fail(f"classified fixture/history paths changed unexpectedly: {missing_retained}")

    print(
        "PASS: canonical slug is active; legacy slug is rejected and occurs only "
        f"in {len(FIXTURE_FILES)} fixture paths, {len(HISTORY_FILES)} immutable "
        "history paths, and this verifier's negative control."
    )


if __name__ == "__main__":
    repository = Path(
        subprocess.check_output(
            ["git", "rev-parse", "--show-toplevel"], text=True
        ).strip()
    )
    verify(repository)
