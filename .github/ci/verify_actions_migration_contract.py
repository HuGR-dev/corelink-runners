#!/usr/bin/env python3
"""Fail closed on hosted CI routing and provider-workflow trigger boundaries."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HOSTED = (
    "ci.yml",
    "conformance.yml",
    "dco.yml",
    "pg-suite.yml",
    "plan-integrity.yml",
    "secret-scan.yml",
    "spawn-worker-ci.yml",
)
MANUAL_PROVIDER = (
    "build-cf-container-images.yml",
    "build-fabricd-image.yml",
    "deploy-spawn-worker.yml",
)


def workflow_path(name: str) -> Path:
    return ROOT / ".github" / "workflows" / name


def top_level_events(text: str) -> tuple[set[str], dict[str, str]]:
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if re.fullmatch(r'(?:"on"|on):', line))
    except StopIteration:
        return set(), {}
    event_lines: dict[str, list[str]] = {}
    current: str | None = None
    for line in lines[start + 1:]:
        if line and not line[0].isspace() and not line.startswith("#"):
            break
        match = re.match(r"^  ([A-Za-z_][A-Za-z0-9_-]*):(?:\s*(.*))?$", line)
        if match:
            current = match.group(1)
            event_lines[current] = [match.group(2) or ""]
        elif current is not None:
            event_lines[current].append(line)
    blocks = {key: "\n".join(value) for key, value in event_lines.items()}
    return set(blocks), blocks


def job_blocks(text: str) -> dict[str, str]:
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line == "jobs:")
    except StopIteration:
        return {}
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i] and not lines[i][0].isspace()),
        len(lines),
    )
    starts = [
        (i, match.group(1))
        for i in range(start + 1, end)
        if (match := re.match(r"^  ([A-Za-z0-9_-]+):\s*$", lines[i]))
    ]
    blocks: dict[str, str] = {}
    for index, (line_no, name) in enumerate(starts):
        stop = starts[index + 1][0] if index + 1 < len(starts) else end
        blocks[name] = "\n".join(lines[line_no:stop])
    return blocks


def runner_labels(text: str) -> list[str]:
    return re.findall(r"(?m)^    runs-on:\s*(.*?)\s*(?:#.*)?$", text)


def has_exact_read_only_permissions(text: str) -> bool:
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line == "permissions:")
    except StopIteration:
        return False
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i] and not lines[i][0].isspace()),
        len(lines),
    )
    grants = [
        (match.group(1), match.group(2))
        for line in lines[start + 1:end]
        if (match := re.match(r"^  ([a-z_]+):\s*([a-z]+)\s*$", line))
    ]
    has_job_override = bool(re.search(r"(?m)^    permissions:", "\n".join(lines)))
    return grants == [("contents", "read")] and not has_job_override


def has_no_secret_path(text: str) -> bool:
    non_comment_lines = "\n".join(
        line for line in text.splitlines() if not line.lstrip().startswith("#")
    )
    forbidden = (
        r"\$\{\{\s*secrets\.",
        r"(?mi)^\s*secrets:\s*inherit\s*$",
        r"(?m)^ {4,6}environment:\s*[^#\s]*",
        r"(?m)^    uses:.*\.github/workflows/[^\s@]+@",
    )
    return not any(re.search(pattern, non_comment_lines) for pattern in forbidden)


def validate(documents: dict[str, str]) -> list[str]:
    errors: list[str] = []
    for name in HOSTED:
        jobs = job_blocks(documents[name])
        if not jobs or any(
            [value.strip("'\"") for value in runner_labels(block)] != ["ubuntu-latest"]
            for block in jobs.values()
        ):
            errors.append(f"{name}: every job must use ubuntu-latest")
        if not has_exact_read_only_permissions(documents[name]):
            errors.append(f"{name}: permissions must be only workflow-level contents: read")
        if not has_no_secret_path(documents[name]):
            errors.append(f"{name}: hosted validation must not read repository/environment secrets")
        for block in jobs.values():
            if not re.search(r"(?m)^    timeout-minutes:\s*[1-9][0-9]*\s*$", block):
                errors.append(f"{name}: each job must declare a finite timeout")

    for name in MANUAL_PROVIDER:
        events, _ = top_level_events(documents[name])
        if events != {"workflow_dispatch"}:
            errors.append(f"{name}: only workflow_dispatch is allowed (found {sorted(events)})")
        jobs = job_blocks(documents[name])
        if not jobs or any(
            [value.strip("'\"") for value in runner_labels(block)] != ["corelink"]
            for block in jobs.values()
        ):
            errors.append(f"{name}: every job must retain the corelink runner boundary")

    release_events, release_blocks = top_level_events(documents["release.yml"])
    if release_events != {"push"}:
        errors.append(f"release.yml: only tag push is allowed (found {sorted(release_events)})")
    push_block = release_blocks.get("push", "")
    if not re.search(r"(?m)^\s+tags:\s*\n\s+-\s*['\"]?v\*['\"]?\s*$", push_block):
        errors.append("release.yml: push must remain limited to v* tags")
    if re.search(r"(?m)^\s+branches:", push_block):
        errors.append("release.yml: branch pushes are not allowed")
    return errors


def negative_controls(documents: dict[str, str]) -> None:
    mutations = (
        ("provider pull_request", "deploy-spawn-worker.yml",
         lambda s: s.replace("\non:\n", "\non:\n  pull_request:\n", 1)),
        ("provider branch push", "build-cf-container-images.yml",
         lambda s: s.replace("\non:\n", "\non:\n  push:\n    branches: [main]\n", 1)),
        ("provider schedule", "deploy-spawn-worker.yml",
         lambda s: s.replace("\non:\n", "\non:\n  schedule:\n    - cron: '0 0 * * *'\n", 1)),
        ("release schedule", "release.yml",
         lambda s: s.replace("\non:\n", "\non:\n  schedule:\n    - cron: '0 0 * * *'\n", 1)),
        ("release branch push", "release.yml",
         lambda s: s.replace("\n  push:\n", "\n  push:\n    branches: [main]\n", 1)),
        ("hosted runner removed", "ci.yml",
         lambda s: s.replace("runs-on: ubuntu-latest", "runs-on: corelink", 1)),
        ("extra permission", "ci.yml",
         lambda s: s.replace("  contents: read\n", "  contents: read\n  packages: write\n", 1)),
        ("job permission override", "ci.yml",
         lambda s: s.replace("  gates:\n", "  gates:\n    permissions:\n      id-token: write\n", 1)),
        ("inherited secrets", "ci.yml",
         lambda s: s.replace("  gates:\n", "  gates:\n    secrets: inherit\n", 1)),
        ("environment secrets", "ci.yml",
         lambda s: s.replace("  gates:\n", "  gates:\n    environment: production\n", 1)),
        ("partial provider runner migration", "deploy-spawn-worker.yml",
         lambda s: re.sub(r"(?m)^    runs-on: corelink$", "    runs-on: ubuntu-latest", s, count=1)),
    )
    for label, path, mutate in mutations:
        case = dict(documents)
        before = case[path]
        case[path] = mutate(before)
        if case[path] == before or not validate(case):
            raise SystemExit(f"negative control failed to reject {label}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    names = (*HOSTED, *MANUAL_PROVIDER, "release.yml")
    documents = {name: workflow_path(name).read_text() for name in names}
    errors = validate(documents)
    if errors:
        print("\n".join(f"ERROR: {error}" for error in errors), file=sys.stderr)
        return 1
    if args.self_test:
        negative_controls(documents)
        print("workflow migration contract and 11 negative controls passed")
    else:
        print("workflow migration contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
