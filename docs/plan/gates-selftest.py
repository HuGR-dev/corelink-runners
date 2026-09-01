#!/usr/bin/env python3
"""Negative tests for the three planning gates.

Each mutation recreates a false PASS found by a cold review.  The self-test
passes only when the unmodified inputs pass and every corrupted copy blocks.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


PLAN_DIR = Path(__file__).resolve().parent
REPO = PLAN_DIR.parent.parent
SOURCE = PLAN_DIR / "audit-2026-08-30-finding-ids.txt"
PLAN = PLAN_DIR / "2026-08-30-golive-remediation-plan.md"
TRIAGE = PLAN_DIR / "union-triage-remaining.md"


def execute(checker: str, target: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(PLAN_DIR / checker), str(target)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )


def require(label: str, checker: str, target: Path, should_pass: bool) -> None:
    result = execute(checker, target)
    passed = result.returncode == 0
    if passed != should_pass:
        stream = result.stdout + result.stderr
        expectation = "PASS" if should_pass else "BLOCK"
        raise AssertionError(
            f"{label}: expected {expectation}, rc={result.returncode}\n{stream}"
        )
    print(f"PASS {label}: {'accepted baseline' if should_pass else 'blocked mutation'}")


def replace_once(document: str, old: str, new: str, label: str) -> str:
    count = document.count(old)
    if count != 1:
        raise AssertionError(f"{label}: expected one mutation target, found {count}")
    return document.replace(old, new, 1)


def main() -> int:
    require("plan baseline", "plan-check.py", SOURCE, True)
    require("WP baseline", "wp-check.py", PLAN, True)
    require("AU staging baseline", "au-check.py", TRIAGE, True)

    source = SOURCE.read_text(encoding="utf-8")
    plan = PLAN.read_text(encoding="utf-8")
    triage = TRIAGE.read_text(encoding="utf-8")

    with tempfile.TemporaryDirectory(prefix="corelink-plan-gates-") as temp:
        work = Path(temp)

        duplicate_source = work / "duplicate-source.txt"
        first_id = source.splitlines()[0]
        duplicate_source.write_text(source + first_id + "\n", encoding="utf-8")
        require(
            "plan duplicate physical source row",
            "plan-check.py",
            duplicate_source,
            False,
        )

        duplicate_a = work / "duplicate-a-row.md"
        a1_line = next(
            line for line in plan.splitlines() if line.startswith("| A1.1 |")
        )
        duplicate_a.write_text(
            plan.replace(a1_line, f"{a1_line}\n{a1_line}", 1), encoding="utf-8"
        )
        require("WP duplicate physical A row", "wp-check.py", duplicate_a, False)

        ownership_drift = work / "ownership-drift.md"
        ownership_drift.write_text(
            replace_once(
                plan,
                "| **T1-W2** | A1.1 A1.2 A1.3 A1.5 |",
                "| **T1-W2** | A1.2 A1.3 A1.5 |",
                "WP ownership drift",
            ),
            encoding="utf-8",
        )
        require(
            "WP Markdown/catalog ownership drift", "wp-check.py", ownership_drift, False
        )

        renamed_heading = work / "renamed-heading.md"
        renamed_heading.write_text(
            replace_once(
                plan,
                "## 3. The acceptance suite (the completeness anchor)",
                "## 3. The acceptance suite (the completeness anchor) RENAMED",
                "WP exact heading",
            ),
            encoding="utf-8",
        )
        require("WP renamed canonical heading", "wp-check.py", renamed_heading, False)

        legacy_alias = work / "legacy-au-alias.md"
        legacy_alias.write_text(triage.replace("T3-W14", "T3-W8"), encoding="utf-8")
        require("AU legacy WP alias collision", "au-check.py", legacy_alias, False)

        renamed_au_heading = work / "renamed-au-heading.md"
        renamed_au_heading.write_text(
            replace_once(
                triage,
                "## 2. Per-finding placement",
                "## 2. Per-finding placement RENAMED",
                "AU exact heading",
            ),
            encoding="utf-8",
        )
        require(
            "AU renamed canonical heading", "au-check.py", renamed_au_heading, False
        )

        missing_serial = work / "missing-au-serial-edge.md"
        without_edge = replace_once(
            triage,
            "`T5-W2` → `T5-W3` is serial",
            "`T5-W2` / `T5-W3` share scope",
            "AU serial edge prose",
        )
        without_edge = replace_once(
            without_edge,
            "| **T5-W3** *(new)* — GitHub Action shell safety | W1 (serial after T5-W2) |",
            "| **T5-W3** *(new)* — GitHub Action shell safety | W1 |",
            "AU serial edge declaration",
        )
        missing_serial.write_text(without_edge, encoding="utf-8")
        require(
            "AU missing shared-scope serial edge", "au-check.py", missing_serial, False
        )

        wrong_au_owner = work / "wrong-au-owner.md"
        union23 = next(
            line for line in triage.splitlines() if line.startswith("| union-23 |")
        )
        wrong_union23 = union23.replace("**T5-W1**", "**T7-W4**", 1)
        wrong_au_owner.write_text(
            triage.replace(union23, wrong_union23, 1), encoding="utf-8"
        )
        require(
            "AU7.10 outside canonical file owner", "au-check.py", wrong_au_owner, False
        )

    print("\nplan gate self-test: PASS — baselines accepted and 8 corruptions blocked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
