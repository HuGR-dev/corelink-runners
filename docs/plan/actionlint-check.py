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

# Expressions in ``runs-on`` can resolve to labels that actionlint cannot
# include in its runner-label diagnostics.  Permit only the deliberately small
# subset whose expression is a single string literal resolving to a label that
# is already approved by this repository.  All other expression/dynamic forms
# fail closed before the diagnostic multiset is compared.
APPROVED_RUNNER_LABELS = {
    "corelink",
    "corelink-builder",
    "corelink-dogfood",
    "ubuntu-latest",
}

RUNS_ON_KEY_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?:runs-on|'runs-on'|\"runs-on\")\s*:\s*(?P<value>.*)$"
)
STATIC_RUNNER_LABEL_RE = re.compile(r"[A-Za-z0-9._-]+")
STATIC_LITERAL_EXPRESSIONS = (
    re.compile(r"^\$\{\{\s*(['\"])(?P<label>[A-Za-z0-9._-]+)\1\s*\}\}$"),
    re.compile(r'^"\$\{\{\s*\'(?P<label>[A-Za-z0-9._-]+)\'\s*\}\}"$'),
    re.compile(r"^'\$\{\{\s*\"(?P<label>[A-Za-z0-9._-]+)\"\s*\}\}'$"),
)

PLAN_INTEGRITY_SHA_BINDING_REQUIREMENTS = {
    "trigger SHA environment": ("          EXPECTED_SHA: ${{ github.sha }}", 2),
    "checked-out SHA capture": (
        '          ACTUAL_SHA="$(git rev-parse --verify HEAD)"',
        2,
    ),
    "checked-out SHA equality": (
        '          if [[ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]]; then',
        2,
    ),
    "PR base/head parent binding": (
        '              if [[ -n "$EXTRA_PARENT" || "$FIRST_PARENT" != '
        '"$PR_BASE_SHA" || "$SECOND_PARENT" != "$PR_HEAD_SHA" ]]; then',
        1,
    ),
    "push after binding": (
        '              if [[ "$ACTUAL_SHA" != "$PUSH_AFTER_SHA" ]]; then',
        1,
    ),
    "success-only completion": ("        if: ${{ success() }}", 1),
    "completion record": (
        '          RECORD="plan-integrity-completion/v1 event=$EVENT_NAME '
        'ref=$REF_NAME sha=$ACTUAL_SHA"',
        1,
    ),
    "completion summary": (
        "          printf '### Plan integrity completion\\n\\n%s\\n' "
        '"$RECORD" >> "$GITHUB_STEP_SUMMARY"',
        1,
    ),
}
PLAN_INTEGRITY_STEP_ORDER = (
    "      - name: Bind checkout to triggering SHA",
    "      - name: Lint workflow syntax",
    "      - name: Exercise structural planning gates",
    "      - name: Emit SHA-bound completion record",
)

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
        (".github/workflows/pg-suite.yml", "corelink"): 1,
        (".github/workflows/prove-baked-buildkit.yml", "corelink"): 1,
        (".github/workflows/release.yml", "corelink"): 3,
        (".github/workflows/spawn-worker-ci.yml", "corelink"): 1,
    }
)

DIAGNOSTIC_RE = re.compile(
    r'^(?P<path>[^:]+):\d+:\d+: label "(?P<label>[^"]+)" is unknown\..*\[runner-label\]$'
)


def _strip_yaml_comment(value: str) -> str:
    """Remove a plain YAML comment without treating quoted ``#`` as one."""

    quote: str | None = None
    escaped = False
    for index, character in enumerate(value):
        if escaped:
            escaped = False
            continue
        if character == "\\" and quote == '"':
            escaped = True
            continue
        if character in {"'", '"'}:
            if quote is None:
                quote = character
            elif quote == character:
                quote = None
            continue
        if (
            character == "#"
            and quote is None
            and (index == 0 or value[index - 1].isspace())
        ):
            return value[:index].rstrip()
    return value.rstrip()


def _static_runner_label(value: str) -> str | None:
    value = _strip_yaml_comment(value.strip())
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        value = value[1:-1]
    return value if STATIC_RUNNER_LABEL_RE.fullmatch(value) else None


def _static_literal_expression_label(value: str) -> str | None:
    value = _strip_yaml_comment(value.strip())
    for pattern in STATIC_LITERAL_EXPRESSIONS:
        match = pattern.fullmatch(value)
        if match is not None:
            return match.group("label")
    return None


def validate_runs_on_bindings(workflow_path: Path) -> list[str]:
    """Reject runner expressions/aliases that cannot be proven repository-approved."""

    text = workflow_path.read_text(encoding="utf-8")
    lines = text.splitlines()
    errors: list[str] = []
    for index, line in enumerate(lines):
        match = RUNS_ON_KEY_RE.match(line)
        if match is None:
            continue

        line_number = index + 1
        indent = len(match.group("indent").expandtabs(8))
        value = match.group("value").strip()
        block_lines: list[str] = []
        if not value:
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor]
                if not candidate.strip():
                    cursor += 1
                    continue
                candidate_indent = len(candidate) - len(candidate.lstrip(" \t"))
                if candidate_indent <= indent:
                    break
                block_lines.append(candidate.strip())
                cursor += 1

        if value:
            expression_label = _static_literal_expression_label(value)
            if "${{" in value:
                if expression_label not in APPROVED_RUNNER_LABELS:
                    errors.append(
                        f"{workflow_path}: line {line_number}: runs-on expression is "
                        "not a statically approved literal label"
                    )
                continue
            if _static_runner_label(value) is not None:
                continue
            if value.startswith("[") and value.endswith("]"):
                entries = [entry.strip() for entry in value[1:-1].split(",")]
                if entries and all(_static_runner_label(entry) for entry in entries):
                    continue
            errors.append(
                f"{workflow_path}: line {line_number}: runs-on value cannot be "
                "statically proven"
            )
            continue

        if block_lines and all(
            entry.startswith("-") and _static_runner_label(entry[1:].strip())
            for entry in block_lines
        ):
            continue
        errors.append(
            f"{workflow_path}: line {line_number}: block runs-on value cannot be "
            "statically proven"
        )
    return errors


def validate_plan_integrity_sha_binding(root: Path) -> list[str]:
    """Require the CI checkout and completion record to bind to the event SHA."""

    workflow_path = root / ".github" / "workflows" / "plan-integrity.yml"
    workflow = workflow_path.read_text(encoding="utf-8")
    errors: list[str] = []
    for label, (
        required_text,
        expected_count,
    ) in PLAN_INTEGRITY_SHA_BINDING_REQUIREMENTS.items():
        count = workflow.count(required_text)
        if count != expected_count:
            errors.append(
                f"{workflow_path}: SHA binding {label!r} count mismatch: "
                f"expected {expected_count}, got {count}"
            )
    positions = [workflow.find(step) for step in PLAN_INTEGRITY_STEP_ORDER]
    if any(position < 0 for position in positions) or positions != sorted(positions):
        errors.append(
            f"{workflow_path}: SHA binding/check/completion steps are missing or reordered"
        )
    return errors


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
    runner_binding_errors = [
        error
        for path in workflow_files
        for error in validate_runs_on_bindings(root / path)
    ]
    if runner_binding_errors:
        raise RuntimeError(
            "actionlint: unapproved runs-on binding:\n"
            + "\n".join(runner_binding_errors)
        )
    sha_binding_errors = validate_plan_integrity_sha_binding(root)
    if sha_binding_errors:
        raise RuntimeError(
            "actionlint: plan-integrity SHA binding mismatch:\n"
            + "\n".join(sha_binding_errors)
        )
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
