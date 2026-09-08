#!/usr/bin/env python3
"""Verify that the PR runner-image lane is structurally build-only.

This intentionally reads only the workflow's ``run: |`` command blocks. A
publisher-looking word in a prose comment is not a command, while a command
split across shell continuation lines is normalized before policy matching.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


DENIED_COMMANDS = (
    ("docker login", re.compile(r"\bdocker\s+login\b", re.I)),
    ("docker push", re.compile(r"\bdocker\s+push\b", re.I)),
    ("docker image/manifest push", re.compile(r"\bdocker\s+(?:image|manifest)\s+push\b", re.I)),
    (
        "docker buildx imagetools publisher",
        re.compile(r"\bdocker\s+buildx\s+imagetools\s+create\b", re.I),
    ),
    (
        "docker buildx/bake --push",
        re.compile(r"\bdocker\s+buildx\s+(?:build|bake)\b[^\n;]*--push\b", re.I),
    ),
    ("generic build --push", re.compile(r"\b--push(?:=true)?\b|\bpush\s*=\s*true\b", re.I)),
    ("registry output", re.compile(r"\btype\s*=\s*registry\b|--output(?:=|\s+)registry\b", re.I)),
    ("nerdctl/buildctl", re.compile(r"\b(?:nerdctl|buildctl)\b", re.I)),
    (
        "registry copy/publish tools",
        re.compile(r"\b(?:crane|skopeo|oras|podman|buildah|regctl)\b", re.I),
    ),
    ("wrangler publication", re.compile(r"\bwrangler\b.*\b(?:push|deploy)\b", re.I)),
    ("package publication", re.compile(r"\b(?:npm|pnpm|yarn)\s+(?:publish|npm\s+publish)\b", re.I)),
    ("HTTP registry publication", re.compile(r"\b(?:curl|wget)\b[^\n;]*(?:registry|/v2/)\b", re.I)),
    ("workflow dispatch", re.compile(r"\bgh\s+workflow\s+run\b", re.I)),
)

TOKEN_REFERENCES = (
    ("github.token interpolation", re.compile(r"\bgithub\.token\b", re.I)),
    (
        "token interpolation",
        re.compile(
            r"\$\{\{[^}\n]*(?:GITHUB_TOKEN|ACTIONS_RUNTIME_TOKEN|GH_TOKEN|[A-Z_]*TOKEN)[^}\n]*\}\}",
            re.I,
        ),
    ),
    (
        "runtime token variable",
        re.compile(r"\$\{?(?:ACTIONS_RUNTIME_TOKEN|GITHUB_TOKEN|GH_TOKEN)\}?\b", re.I),
    ),
)


def action_blocks(text: str) -> list[tuple[str, str]]:
    """Return (uses reference, complete step text) for every action step.

    The repository intentionally avoids a third-party YAML dependency in these
    offline guards. GitHub action steps have a stable two-space nesting shape;
    this parser finds both ``- uses:`` and ``- name: ...`` followed by
    ``uses:`` and retains the step's ``with:`` body for policy checks.
    """

    lines = text.splitlines()
    result: list[tuple[str, str]] = []
    for index, line in enumerate(lines):
        direct = re.match(r"^(?P<indent>\s*)-\s+uses:\s*(?P<ref>[^\s#]+)", line)
        nested = re.match(r"^(?P<indent>\s*)uses:\s*(?P<ref>[^\s#]+)", line)
        if direct:
            step_indent = len(direct.group("indent"))
            reference = direct.group("ref")
            start = index
        elif nested:
            key_indent = len(nested.group("indent"))
            step_indent = key_indent - 2
            reference = nested.group("ref")
            start = index
            while start > 0:
                candidate = lines[start - 1]
                if candidate.strip() and len(candidate) - len(candidate.lstrip()) <= step_indent:
                    break
                start -= 1
        else:
            continue

        end = index + 1
        while end < len(lines):
            candidate = lines[end]
            if candidate.strip() and len(candidate) - len(candidate.lstrip()) <= step_indent:
                break
            end += 1
        result.append((reference, "\n".join(lines[start:end])))
    return result


def action_push_value(block: str) -> str | None:
    match = re.search(r"(?m)^\s+push\s*:\s*(.*?)\s*(?:#.*)?$", block)
    return match.group(1).strip().strip("'\"").lower() if match else None


def run_blocks(text: str) -> list[str]:
    """Extract YAML literal ``run`` blocks without needing PyYAML."""

    lines = text.splitlines()
    blocks: list[str] = []
    index = 0
    while index < len(lines):
        match = re.match(r"^(\s*)run:\s*\|\s*$", lines[index])
        if not match:
            index += 1
            continue
        parent_indent = len(match.group(1))
        command_lines: list[str] = []
        index += 1
        while index < len(lines):
            line = lines[index]
            if line.strip() and len(line) - len(line.lstrip()) <= parent_indent:
                break
            command_lines.append(line[parent_indent + 2 :] if line else "")
            index += 1
        blocks.append("\n".join(command_lines))
    return blocks


def normalized_commands(block: str) -> str:
    lines = [line for line in block.splitlines() if not line.lstrip().startswith("#")]
    return " ".join(line.strip() for line in lines)


def fail(message: str) -> int:
    print(f"verify_build_only_workflow: {message}", file=sys.stderr)
    return 1


def verify(workflow_path: Path, validator_path: Path) -> int:
    if not workflow_path.is_file() or not validator_path.is_file():
        return fail("workflow or validator is missing")
    workflow = workflow_path.read_text(encoding="utf-8")
    validator = validator_path.read_text(encoding="utf-8")

    required = (
        (r"^  pull_request:\s*$", "pull_request trigger"),
        (r"^    runs-on:\s+ubuntu-latest\s*$", "hosted runner"),
        (r"^  contents:\s+read\s*$", "read-only contents permission"),
        (r"persist-credentials:\s+false", "non-persistent checkout credentials"),
        (r"uses:\s+actions/checkout@[0-9a-f]{40}", "pinned checkout action"),
        (r"github\.event\.pull_request\.number", "PR-scoped concurrency"),
        (r"^  cancel-in-progress:\s+true\s*$", "bounded concurrency cancellation"),
        (r"^      - 'deploy/runner/\*\*'\s*$", "runner context path trigger"),
        (
            r"^      - '\.github/workflows/build-cf-container-images\.yml'\s*$",
            "production image workflow trigger",
        ),
        (r"^      - '\.github/workflows/image-build-impact\.yml'\s*$", "workflow trigger"),
        (r"^      - 'scripts/ci/image-build-impact\.sh'\s*$", "impact detector trigger"),
        (
            r"^      - 'scripts/ci/image-build-impact\.selftest\.sh'\s*$",
            "impact detector selftest trigger",
        ),
        (
            r"^      - 'scripts/ci/runner-image-build-validation\.sh'\s*$",
            "build validator trigger",
        ),
        (
            r"^      - 'scripts/ci/runner-image-build-validation\.selftest\.sh'\s*$",
            "build validator selftest trigger",
        ),
        (r"^      - 'scripts/ci/runner-image-static-check\.sh'\s*$", "static-check path trigger"),
        (
            r"^      - 'scripts/ci/runner-image-static-check\.selftest\.sh'\s*$",
            "static selftest path trigger",
        ),
        (r"^      - 'scripts/ci/verify_build_only_workflow\.py'\s*$", "build-only checker trigger"),
        (r"runner-image-build-validation\.sh", "build validator invocation"),
    )
    for pattern, description in required:
        if not re.search(pattern, workflow, re.MULTILINE):
            return fail(f"missing {description}")
    if re.search(r"pull_request_target|secrets\.", workflow):
        return fail("trusted trigger or secret reference found")

    for description, pattern in TOKEN_REFERENCES:
        match = pattern.search(workflow)
        if match:
            return fail(f"{description} found: {match.group(0)}")

    blocks = run_blocks(workflow)
    if not blocks:
        return fail("workflow has no executable run block")
    command_text = "\n".join(normalized_commands(block) for block in blocks)
    for description, pattern in DENIED_COMMANDS:
        match = pattern.search(command_text)
        if match:
            return fail(f"{description} found in workflow command: {match.group(0)}")

    for reference, block in action_blocks(workflow):
        reference_lower = reference.lower()
        if re.search(r"(?:^|/)(?:docker|azure|redhat-actions)/.*(?:build-push|login)", reference_lower):
            return fail(f"publication-capable action found: {reference}")
        push_value = action_push_value(block)
        if push_value is not None and push_value not in {"false", "0", "no", "off"}:
            return fail(f"action publication input found in {reference}: push: {push_value}")

    # The validator itself is shell, not YAML. Keep the same denylist over its
    # executable source as a second fence against a future publication escape.
    validator_lines = [line for line in validator.splitlines() if not line.lstrip().startswith("#")]
    validator_text = "\n".join(validator_lines)
    for description, pattern in DENIED_COMMANDS:
        match = pattern.search(validator_text)
        if match:
            return fail(f"{description} found in validator: {match.group(0)}")
    for description, pattern in TOKEN_REFERENCES:
        match = pattern.search(validator_text)
        if match:
            return fail(f"{description} found in validator: {match.group(0)}")

    print("verify_build_only_workflow: PASS")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: verify_build_only_workflow.py WORKFLOW VALIDATOR")
    raise SystemExit(verify(Path(sys.argv[1]), Path(sys.argv[2])))
