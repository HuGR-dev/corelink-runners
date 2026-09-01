#!/usr/bin/env python3
"""Run the authoritative actionlint check and verify its exact known baseline.

The repository intentionally uses private self-hosted runner labels.  They
produce actionlint ``[runner-label]`` diagnostics until the runner fleet is
available to the lint environment.  Those diagnostics are an explicit,
versioned multiset: a new workflow, a new label, a changed count, or any other
actionlint diagnostic is a failure.

This checker deliberately ignores repository actionlint configuration for the
authoritative run.  The optional configuration is validated separately by the
workflow, but it cannot suppress syntax or runner-label diagnostics here.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path


EXPECTED_ACTIONLINT_VERSION = "1.7.12"

# Keep this as an exact occurrence multiset, rather than a path/label regex.
# The key is (workflow path relative to the repository root, runner label).
EXPECTED_RUNNER_LABEL_DIAGNOSTICS: Counter[tuple[str, str]] = Counter(
    {
        (".github/workflows/build-cf-container-images.yml", "corelink"): 1,
        (".github/workflows/build-fabricd-image.yml", "corelink"): 1,
        (".github/workflows/canary-smoke.yml", "corelink-builder"): 1,
        (".github/workflows/ci.yml", "corelink"): 1,
        (".github/workflows/clw-ticket-propagation-check.yml", "corelink-dogfood"): 1,
        (".github/workflows/corelink-smoke.yml", "corelink"): 1,
        (".github/workflows/corelink-stress.yml", "corelink"): 1,
        (".github/workflows/dco.yml", "corelink"): 1,
        (".github/workflows/deploy-spawn-worker.yml", "corelink"): 2,
        (".github/workflows/dogfood-smoke.yml", "corelink-dogfood"): 1,
        (".github/workflows/latency-probe.yml", "corelink"): 1,
        (".github/workflows/moat-action-test.yml", "corelink-dogfood"): 1,
        (".github/workflows/moat-benchmark.yml", "corelink-dogfood"): 1,
        (".github/workflows/moat-correctness.yml", "corelink-dogfood"): 1,
        (".github/workflows/o7-metadata-probe.yml", "corelink-dogfood"): 1,
        (".github/workflows/orphan-box-detect.yml", "corelink"): 1,
        (".github/workflows/plan-integrity.yml", "corelink"): 1,
        (".github/workflows/prove-baked-buildkit.yml", "corelink"): 1,
        (".github/workflows/release.yml", "corelink"): 3,
        (".github/workflows/spawn-worker-ci.yml", "corelink"): 1,
    }
)

DIAGNOSTIC_RE = re.compile(
    r'^(?P<path>[^:]+):\d+:\d+: label "(?P<label>[^"]+)" is unknown\..*\[runner-label\]$'
)


def actionlint_binary() -> str:
    binary = shutil.which("actionlint")
    if binary is None:
        raise RuntimeError("actionlint is required but was not found on PATH")
    return binary


def check_version(binary: str) -> None:
    result = subprocess.run(
        [binary, "-version"], capture_output=True, text=True, check=False
    )
    first_line = (result.stdout + result.stderr).splitlines()
    actual = first_line[0].strip() if first_line else ""
    if result.returncode != 0 or actual != EXPECTED_ACTIONLINT_VERSION:
        raise RuntimeError(
            "actionlint: expected pinned version "
            f"{EXPECTED_ACTIONLINT_VERSION}, got {actual or '<no version>'}"
        )


def classify_diagnostics(output: str) -> Counter[tuple[str, str]]:
    observed: Counter[tuple[str, str]] = Counter()
    unexpected: list[str] = []
    for line in output.splitlines():
        if not line.strip():
            continue
        match = DIAGNOSTIC_RE.fullmatch(line)
        if match is None:
            unexpected.append(line)
            continue
        observed[(match.group("path"), match.group("label"))] += 1

    if unexpected:
        details = "\n".join(unexpected)
        raise RuntimeError("actionlint: unexpected diagnostics:\n" + details)
    return observed


def run(root: Path) -> int:
    binary = actionlint_binary()
    check_version(binary)
    workflow_dir = root / ".github" / "workflows"
    workflow_files = sorted(
        path.relative_to(root)
        for path in workflow_dir.rglob("*")
        if path.is_file() and path.suffix in {".yml", ".yaml"}
    )
    if not workflow_files:
        raise RuntimeError(f"actionlint: no workflow files found below {workflow_dir}")
    result = subprocess.run(
        [
            binary,
            "-config-file",
            "/dev/null",
            "-oneline",
            *(str(path) for path in workflow_files),
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    output = result.stdout + result.stderr
    observed = classify_diagnostics(output)

    if observed != EXPECTED_RUNNER_LABEL_DIAGNOSTICS:
        missing = EXPECTED_RUNNER_LABEL_DIAGNOSTICS - observed
        extra = observed - EXPECTED_RUNNER_LABEL_DIAGNOSTICS
        details: list[str] = []
        if missing:
            details.append(
                f"missing known occurrences: {dict(sorted(missing.items()))}"
            )
        if extra:
            details.append(
                f"extra/unapproved occurrences: {dict(sorted(extra.items()))}"
            )
        raise RuntimeError(
            "actionlint: runner-label baseline mismatch; " + "; ".join(details)
        )

    if result.returncode == 0:
        raise RuntimeError(
            "actionlint: expected known runner-label diagnostics but command passed"
        )

    print(
        "actionlint: PASS — version "
        f"{EXPECTED_ACTIONLINT_VERSION}; exact runner-label baseline "
        f"({sum(observed.values())} diagnostics)"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root to lint (default: repository containing this script)",
    )
    args = parser.parse_args(argv)
    try:
        return run(args.root.resolve())
    except (OSError, RuntimeError, subprocess.SubprocessError) as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
