#!/usr/bin/env python3
"""Negative tests for the three planning gates.

Each mutation recreates a false PASS found by a cold review.  The self-test
passes only when the unmodified inputs pass and every corrupted copy blocks.

The fixtures intentionally exercise the physical Markdown surfaces as well as
the catalogues: a gate must not pass merely because a malformed row is opaque,
or because a summary/header/capability section was changed while the global id
set still looks correct.
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
DAG = PLAN_DIR / "2026-09-01-reconciled-dispatch-dag.md"


def execute(
    checker: str, target: Path, *, dag: Path | None = None
) -> subprocess.CompletedProcess[str]:
    command = [sys.executable, str(PLAN_DIR / checker), str(target)]
    if checker == "au-check.py":
        command.extend(["--plan", str(PLAN)])
        # The dispatch DAG is a Round-5 input.  Keep this compatible with the
        # pre-DAG snapshot while using it whenever the coordinated input is
        # present (the evolved AU gate requires the explicit path).
        supports_dag = False
        selected_dag = dag or DAG
        if selected_dag.exists():
            help_result = subprocess.run(
                [sys.executable, str(PLAN_DIR / checker), "--help"],
                cwd=REPO,
                check=False,
                capture_output=True,
                text=True,
            )
            supports_dag = "--dag" in help_result.stdout
        if supports_dag:
            command.extend(["--dag", str(selected_dag)])
    return subprocess.run(
        command,
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )


def require(
    label: str,
    checker: str,
    target: Path,
    should_pass: bool,
    *,
    dag: Path | None = None,
) -> None:
    result = execute(checker, target, dag=dag)
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


def swap_once(document: str, first: str, second: str, label: str) -> str:
    """Swap two adjacent physical rows, asserting the fixture is unambiguous."""

    pair = f"{first}\n{second}"
    count = document.count(pair)
    if count != 1:
        raise AssertionError(f"{label}: expected one adjacent row pair, found {count}")
    return document.replace(pair, f"{second}\n{first}", 1)


def table_cell(document: str, row_prefix: str, label: str) -> str:
    """Return the final cell of one declaration row for a scope mutation."""

    rows = [line for line in document.splitlines() if line.startswith(row_prefix)]
    if len(rows) != 1:
        raise AssertionError(
            f"{label}: expected one declaration row, found {len(rows)}"
        )
    cells = rows[0].strip().strip("|").split("|")
    if len(cells) < 2 or not cells[-1].strip():
        raise AssertionError(f"{label}: declaration has no scope cell")
    return cells[-1].strip()


def replace_in_row(
    document: str, row_prefix: str, old: str, new: str, label: str
) -> str:
    """Replace one token in one physical Markdown row."""

    rows = [line for line in document.splitlines() if line.startswith(row_prefix)]
    if len(rows) != 1:
        raise AssertionError(f"{label}: expected one row, found {len(rows)}")
    row = rows[0]
    if row.count(old) != 1:
        raise AssertionError(
            f"{label}: expected one token in row, found {row.count(old)}"
        )
    return document.replace(row, row.replace(old, new, 1), 1)


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

        kind_drift = work / "kind-drift.md"
        kind_drift.write_text(
            replace_once(
                plan,
                "| A1.1 | probe |",
                "| A1.1 | test |",
                "A kind drift",
            ),
            encoding="utf-8",
        )
        require("WP frozen A kind drift", "wp-check.py", kind_drift, False)

        opaque_extra_a = work / "opaque-extra-a-row.md"
        opaque_extra_a.write_text(
            replace_once(
                plan,
                "| A1.1 | probe | `GET /health` returns 200 |",
                "| A1.1 | probe | `GET /health` returns 200 |\n| A99.99 (opaque) | test | opaque extra acceptance row |",
                "opaque extra acceptance row",
            ),
            encoding="utf-8",
        )
        require("WP opaque extra acceptance row", "wp-check.py", opaque_extra_a, False)

        wave2_reorder = work / "wave2-reordered.md"
        wave2_row_1 = "| 1 | **T4-W1** | A4.1 | `index.ts` |"
        wave2_row_2 = "| 2 | **T4-W2** | A4.2 A4.3 A4.6 | `index.ts` + `lib.ts` |"
        wave2_reorder.write_text(
            swap_once(plan, wave2_row_1, wave2_row_2, "Wave-2 reorder"),
            encoding="utf-8",
        )
        require("WP Wave-2 serial reorder", "wp-check.py", wave2_reorder, False)

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

        summary_corruption = work / "summary-corruption.md"
        summary_corruption.write_text(
            replace_once(
                plan,
                "**94 rows —",
                "**95 rows —",
                "summary count",
            ),
            encoding="utf-8",
        )
        require(
            "WP acceptance summary corruption", "wp-check.py", summary_corruption, False
        )

        header_corruption = work / "header-corruption.md"
        header_corruption.write_text(
            replace_once(
                plan,
                "| # | WP | owns | scope |",
                "| # | WP | owns | wrong-header |",
                "Wave-2 header",
            ),
            encoding="utf-8",
        )
        require("WP Wave-2 header corruption", "wp-check.py", header_corruption, False)

        capability_corruption = work / "capability-corruption.md"
        capability_corruption.write_text(
            replace_once(
                plan,
                "### C1 — control plane",
                "### C1 — corrupted capability",
                "capability heading",
            ),
            encoding="utf-8",
        )
        require(
            "WP capability partition corruption",
            "wp-check.py",
            capability_corruption,
            False,
        )

        deleted_t4w3_dependency = work / "deleted-t4-w3-dependency.md"
        if DAG.exists():
            deleted_t4w3_dag = work / "deleted-t4-w3-dependency-dag.md"
            deleted_t4w3_dag.write_text(
                replace_once(
                    DAG.read_text(encoding="utf-8"),
                    "| T3-W10 | W1 serial | T3-W4, T4-W4 |",
                    "| T3-W10 | W1 serial | T3-W4, T4-W3 |",
                    "deleted T4-W3 dependency",
                ),
                encoding="utf-8",
            )
            deleted_t4w3_dependency = deleted_t4w3_dag
            deleted_t4w3_target = TRIAGE
        else:
            deleted_t4w3_dependency.write_text(
                replace_in_row(
                    triage,
                    "| union-10 |",
                    "T3-W4 → T4-W4",
                    "T3-W4 → T4-W3",
                    "deleted T4-W3 dependency",
                ),
                encoding="utf-8",
            )
            deleted_t4w3_target = deleted_t4w3_dependency
        require(
            "AU deleted T4-W3 dependency",
            "au-check.py",
            deleted_t4w3_target,
            False,
            dag=deleted_t4w3_dependency if DAG.exists() else None,
        )

        source_au_swap = work / "source-au-swap.md"
        source_au_swap.write_text(
            replace_once(
                triage,
                "| union-06 |",
                "| AU4.16a |",
                "source to AU swap",
            ),
            encoding="utf-8",
        )
        require("AU source→AU swap", "au-check.py", source_au_swap, False)

        au_invalid_kind = work / "au-invalid-kind.md"
        au_invalid_kind.write_text(
            replace_once(
                triage,
                "**AU3.19 — test:**",
                "**AU3.19 — invalid-kind:**",
                "AU invalid kind",
            ),
            encoding="utf-8",
        )
        require("AU invalid kind", "au-check.py", au_invalid_kind, False)

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
        if DAG.exists():
            missing_serial_dag = work / "missing-au-serial-edge-dag.md"
            missing_serial_dag.write_text(
                replace_once(
                    DAG.read_text(encoding="utf-8"),
                    "| T5-W3 | W1 serial | T5-W2 |",
                    "| T5-W3 | W1 serial | T7-W1 |",
                    "AU serial edge",
                ),
                encoding="utf-8",
            )
            missing_serial_target = TRIAGE
            missing_serial_dag_override = missing_serial_dag
        else:
            serial_edge = None
            for candidate in (
                "`T5-W2` → `T5-W3` → `T5-W6` is serial",
                "`T5-W2` → `T5-W3` is serial",
            ):
                if candidate in triage:
                    serial_edge = candidate
                    break
            if serial_edge is None:
                raise AssertionError(
                    "AU serial edge prose: no canonical serial declaration found"
                )
            without_edge = replace_once(
                triage,
                serial_edge,
                "`T5-W2` / `T5-W3` / `T5-W6` share scope",
                "AU serial edge prose",
            )
            without_edge = without_edge.replace(" (serial after T5-W2)", "", 1)
            missing_serial.write_text(without_edge, encoding="utf-8")
            missing_serial_target = missing_serial
            missing_serial_dag_override = None
        require(
            "AU missing shared-scope serial edge",
            "au-check.py",
            missing_serial_target,
            False,
            dag=missing_serial_dag_override,
        )

        scope_overlap = work / "au-scope-overlap.md"
        t8w4_scope = table_cell(triage, "| **T8-W4**", "AU parallel scope overlap")
        t7w4_scope = table_cell(triage, "| **T7-W4**", "AU parallel scope overlap")
        scope_overlap.write_text(
            replace_once(
                triage,
                t8w4_scope,
                t7w4_scope,
                "AU parallel scope overlap",
            ),
            encoding="utf-8",
        )
        require("AU parallel scope overlap", "au-check.py", scope_overlap, False)

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

    print("\nplan gate self-test: PASS — baselines accepted and 18 corruptions blocked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
