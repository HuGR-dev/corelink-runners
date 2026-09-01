#!/usr/bin/env python3
"""Check the proposed AU layer without changing the frozen acceptance suite.

The 247-finding ``plan-check.py`` gate and the acceptance/WP ``wp-check.py``
gate intentionally remain independent of this proposal.  This checker is the
mechanical seam between ``union-triage-remaining.md`` and a future suite delta:
it checks that the triage has exactly 30 source findings and 31 AU acceptance
ids (M3 is deliberately split into ``AU3.23a`` and ``AU3.23b``), that each
acceptance id is structurally owned once, and that proposed WPs do not collide
with the principal WPs.

Usage (the positional argument is optional)::

    python3 docs/plan/au-check.py
    python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md

The default path is discovered relative to this script, so the command is
safe to run from the repository root or from another working directory.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path


AU_RE = re.compile(r"\bAU\d+\.\d+(?:[a-z])?\b")
A_RE = re.compile(r"\bA\d+\.\d+\b")
WP_RE = re.compile(r"\bT\d+-W\d+[A-Za-z]*\b")
INV_RE = re.compile(r"\bINV-\d+\b")
KNOWN_INVARIANTS = {f"INV-{i}" for i in range(1, 9)}

# The triage was authored before the lead's coordinated rename.  These are
# aliases only for the proposal: the old names remain principal WPs in the
# frozen suite and must not be reused by a future AU delta.
WP_RENAMES = {
    "T3-W8": "T3-W14",
    "T8-W3": "T8-W5",
    "T2-W5": "T2-W6",
}

EXPECTED_AU = {
    "AU1.8",
    "AU1.9",
    *{f"AU3.{i}" for i in range(19, 23)},
    "AU3.23a",
    "AU3.23b",
    *{f"AU3.{i}" for i in range(24, 29)},
    *{f"AU4.{i}" for i in range(14, 20)},
    *{f"AU5.{i}" for i in range(10, 13)},
    *{f"AU6.{i}" for i in range(16, 18)},
    *{f"AU7.{i}" for i in range(6, 13)},
}
EXPECTED_SOURCE_FINDINGS = {
    *{f"union-{i:02d}" for i in range(6, 31)},
    "RH5",
    "RH9",
    "M3",
    "M5",
    "M19",
}


def normalize_wp(name: str) -> str:
    return WP_RENAMES.get(name, name)


def read_wp_catalog(path: Path) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Read the literal WP dictionary from wp-check.py without executing it."""

    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "WP" for target in node.targets
        ):
            value = ast.literal_eval(node.value)
            items = {name: list(spec[0]) for name, spec in value.items()}
            invariants = {name: list(spec[1]) for name, spec in value.items()}
            return items, invariants
    raise ValueError(f"could not find literal WP catalog in {path}")


def section_between(document: str, start: str, end: str) -> str:
    try:
        body = document[document.index(start) :]
        return body[: body.index(end)]
    except ValueError as exc:
        raise ValueError(f"missing section marker {start!r} or {end!r}") from exc


def parse_rows(document: str) -> tuple[list[tuple[str, str, set[str], str]], list[str], list[str]]:
    """Parse source rows and their AU acceptance ids.

    A source finding normally has one acceptance id.  M3 is intentionally the
    exception: its one source row owns AU3.23a (test) and AU3.23b (probe), and
    the two WP names in the wave/WP cell align with those ids.
    """

    section = section_between(document, "## 2. Per-finding placement", "## 3. Structural consequences")
    rows: list[tuple[str, str, set[str], str]] = []
    errors: list[str] = []
    sources: list[str] = []
    for line_no, line in enumerate(section.splitlines(), 1):
        if not line.startswith("|"):
            continue
        columns = line.split("|")
        # The dependency/evidence cells may cite an AU id too.  The proposed
        # acceptance-item cell is the authoritative per-finding occurrence.
        if len(columns) < 9:
            continue
        ids = AU_RE.findall(columns[8])
        if not ids:
            continue
        # id | origin | severity | bucket | wave/WP | invariants | ...
        source_match = re.search(r"\b(?:union-\d+|RH[59]|M(?:3|5|19))\b", columns[1])
        if not source_match:
            errors.append(f"placement row {line_no} has no source finding id")
            continue
        source = source_match.group()
        sources.append(source)
        # Only bold names are owners.  Dependencies in this cell can mention
        # predecessor WPs (for example T4-W4) and are not AU ownership.
        owners = [normalize_wp(match.group(1)) for match in re.finditer(r"\*\*(T\d+-W\d+[A-Za-z]*)\*\*", columns[5])]
        if len(owners) != len(ids):
            errors.append(
                f"placement row {line_no} has {len(ids)} AU ids but {len(owners)} WPs: {ids} / {owners}"
            )
            continue
        invariants = set(INV_RE.findall(columns[6]))
        rows.extend((item, owner, invariants, source) for item, owner in zip(ids, owners))
    return rows, errors, sources


def parse_wp_declarations(document: str) -> tuple[dict[str, list[str]], dict[str, list[str]], list[str]]:
    """Parse new-WP rows and the four existing-WP AU extensions."""

    section = section_between(document, "## 3. Structural consequences", "## 4. Findings")
    declarations: dict[str, list[str]] = {}
    errors: list[str] = []

    table_start = section.find("**New WPs and their item counts**")
    extensions_start = section.find("**Extensions to existing WPs:**")
    if table_start < 0 or extensions_start < 0 or extensions_start <= table_start:
        return declarations, {}, ["missing new-WP or extension declaration"]

    table = section[table_start:extensions_start]
    for line_no, line in enumerate(table.splitlines(), 1):
        if not line.startswith("|") or "*(new)*" not in line:
            continue
        columns = line.split("|")
        if len(columns) < 4:
            errors.append(f"new-WP row {line_no} has too few columns")
            continue
        wp_match = WP_RE.search(columns[1])
        ids = AU_RE.findall(columns[3])
        if not wp_match:
            errors.append(f"new-WP row {line_no} has no WP name")
            continue
        name = normalize_wp(wp_match.group())
        if name in declarations:
            errors.append(f"new-WP {name} is declared more than once")
        declarations[name] = ids

    extension_section = section[extensions_start:]
    extension_end = extension_section.find("**Four things the lead must decide")
    if extension_end >= 0:
        extension_section = extension_section[:extension_end]
    extensions: dict[str, list[str]] = defaultdict(list)
    # The extension sentence can wrap across lines, and one WP may have more
    # than one ``+AU`` token.  Associate tokens with the WP that precedes them
    # until the next WP name rather than assuming one token per WP.
    wp_matches = list(WP_RE.finditer(extension_section))
    for index, match in enumerate(wp_matches):
        end = wp_matches[index + 1].start() if index + 1 < len(wp_matches) else len(extension_section)
        ids = AU_RE.findall(extension_section[match.end() : end])
        extensions[normalize_wp(match.group())].extend(ids)
    return declarations, dict(extensions), errors


def acceptance_ids(plan_path: Path) -> set[str]:
    document = plan_path.read_text(encoding="utf-8")
    suite = section_between(document, "## 3. The acceptance suite", "## 4. Owner decisions")
    return set(A_RE.findall(suite))


def check(triage_path: Path, plan_path: Path, wp_path: Path) -> tuple[bool, list[str], dict[str, object]]:
    failures: list[str] = []
    document = triage_path.read_text(encoding="utf-8")
    rows, parse_errors, sources = parse_rows(document)
    failures.extend(parse_errors)
    row_counts = Counter(item for item, _, _, _ in rows)
    row_owners = {item: owner for item, owner, _, _ in rows}
    source_counts = Counter(sources)
    missing_sources = sorted(EXPECTED_SOURCE_FINDINGS - set(source_counts))
    unknown_sources = sorted(set(source_counts) - EXPECTED_SOURCE_FINDINGS)
    duplicate_sources = sorted(source for source, count in source_counts.items() if count != 1)
    if len(sources) != len(EXPECTED_SOURCE_FINDINGS) or len(source_counts) != len(EXPECTED_SOURCE_FINDINGS):
        failures.append(
            f"source findings must contain exactly {len(EXPECTED_SOURCE_FINDINGS)} unique ids "
            f"(rows={len(sources)}, unique={len(source_counts)})"
        )
    if missing_sources:
        failures.append(f"source findings missing from placement rows: {missing_sources}")
    if unknown_sources:
        failures.append(f"unknown source findings in placement rows: {unknown_sources}")
    if duplicate_sources:
        failures.append(f"source findings repeated in placement rows: {duplicate_sources}")

    declarations, extensions, declaration_errors = parse_wp_declarations(document)
    failures.extend(declaration_errors)
    owned_by = defaultdict(list)
    for wp, ids in declarations.items():
        for item in ids:
            owned_by[item].append(wp)
    for wp, ids in extensions.items():
        for item in ids:
            owned_by[item].append(wp)
    owned_counts = Counter({item: len(owners) for item, owners in owned_by.items()})

    missing_rows = sorted(EXPECTED_AU - set(row_counts))
    unknown_rows = sorted(set(row_counts) - EXPECTED_AU)
    duplicate_rows = sorted(item for item, count in row_counts.items() if count != 1)
    if len(rows) != len(EXPECTED_AU) or len(row_counts) != len(EXPECTED_AU):
        failures.append(
            f"placement rows must contain exactly {len(EXPECTED_AU)} unique AU ids "
            f"(rows={len(rows)}, unique={len(row_counts)})"
        )
    if missing_rows:
        failures.append(f"AU ids missing from placement rows: {missing_rows}")
    if unknown_rows:
        failures.append(f"unknown AU ids in placement rows: {unknown_rows}")
    if duplicate_rows:
        failures.append(f"AU ids repeated in placement rows: {duplicate_rows}")

    missing_ownership = sorted(EXPECTED_AU - set(owned_counts))
    unknown_ownership = sorted(set(owned_counts) - EXPECTED_AU)
    duplicate_ownership = sorted(item for item, count in owned_counts.items() if count != 1)
    if missing_ownership:
        failures.append(f"AU ids without a WP declaration: {missing_ownership}")
    if unknown_ownership:
        failures.append(f"unknown AU ids in WP declarations: {unknown_ownership}")
    if duplicate_ownership:
        failures.append(f"AU ids not owned exactly once: {duplicate_ownership}")

    row_declaration_mismatch = sorted(
        item for item in EXPECTED_AU if item in row_owners and owned_by.get(item) != [row_owners[item]]
    )
    if row_declaration_mismatch:
        failures.append(f"placement/WP ownership mismatch: {row_declaration_mismatch}")

    try:
        main_items, main_invariants = read_wp_catalog(wp_path)
    except (OSError, SyntaxError, ValueError) as exc:
        failures.append(f"cannot read principal WP catalog: {exc}")
        main_items, main_invariants = {}, {}

    main_names = set(main_items)
    proposal_names = set(declarations)
    # After applying the three coordinated renames, proposal names must be
    # disjoint from principal names.  Do not merge proposal items into the
    # principal catalog: they are not suite items until a later delta lands.
    collisions = sorted(proposal_names & main_names)
    if collisions:
        failures.append(f"proposal/principal WP name collisions: {collisions}")

    expected_proposals = {
        "T3-W14", "T3-W9", "T8-W5", "T8-W6", "T3-W10", "T8-W4",
        "T5-W3", "T7-W4", "T7-W5", "T2-W6",
    }
    if proposal_names != expected_proposals:
        failures.append(
            f"proposal WPs differ from the triage contract: expected "
            f"{sorted(expected_proposals)}, got {sorted(proposal_names)}"
        )
    expected_extensions = {"T4-W1", "T8-W1", "T6-W6", "T3-W5"}
    if set(extensions) != expected_extensions:
        failures.append(
            f"AU extensions differ from the triage contract: expected "
            f"{sorted(expected_extensions)}, got {sorted(extensions)}"
        )

    bad_invariants: dict[str, list[str]] = {}
    for item, _, invariants, _ in rows:
        unknown = sorted(invariants - KNOWN_INVARIANTS)
        if unknown:
            bad_invariants[item] = unknown
    if bad_invariants:
        failures.append(f"AU rows declare unknown invariants: {bad_invariants}")
    for wp, ids in declarations.items():
        if not set().union(*(set(next((inv for item, _, inv, _ in rows if item == i), set())) for i in ids)):
            failures.append(f"proposal WP {wp} has no known invariant declaration")
    for wp, invariants in main_invariants.items():
        unknown = sorted(set(invariants) - KNOWN_INVARIANTS)
        if unknown:
            failures.append(f"principal WP {wp} declares unknown invariants: {unknown}")

    total_items = Counter({wp: len(items) for wp, items in main_items.items()})
    for wp, ids in declarations.items():
        total_items[wp] += len(ids)
    for wp, ids in extensions.items():
        total_items[wp] += len(ids)
    oversized = sorted(wp for wp, count in total_items.items() if count > 4)
    if oversized:
        failures.append(
            "WPs exceed the 4-item ceiling after AU extensions: "
            + str({wp: total_items[wp] for wp in oversized})
        )

    try:
        a_ids = acceptance_ids(plan_path)
    except (OSError, ValueError) as exc:
        failures.append(f"cannot read principal acceptance suite: {exc}")
        a_ids = set()
    a_collisions = sorted(EXPECTED_AU & a_ids)
    if a_collisions:
        failures.append(f"AU/A acceptance-id collisions: {a_collisions}")

    context = {
        "source_rows": len(sources),
        "source_unique": len(source_counts),
        "rows": len(rows),
        "unique_rows": len(row_counts),
        "owned": sum(owned_counts.values()),
        "unique_owned": len(owned_counts),
        "proposal_wps": len(proposal_names),
        "extensions": len(extensions),
        "max_wp_items": max(total_items.values(), default=0),
        "a_collisions": a_collisions,
    }
    return not failures, failures, context


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "triage",
        nargs="?",
        type=Path,
        default=Path(__file__).with_name("union-triage-remaining.md"),
        help="union triage Markdown (default: adjacent union-triage-remaining.md)",
    )
    parser.add_argument(
        "--plan",
        type=Path,
        default=Path(__file__).with_name("2026-08-30-golive-remediation-plan.md"),
        help="frozen plan containing the A acceptance suite",
    )
    parser.add_argument(
        "--wp-check",
        type=Path,
        default=Path(__file__).with_name("wp-check.py"),
        help="principal WP gate (used as a literal catalog)",
    )
    args = parser.parse_args(argv)

    try:
        ok, failures, context = check(args.triage, args.plan, args.wp_check)
    except OSError as exc:
        print(f"au-check: BLOCKED — {exc}")
        return 1

    print(f"triage {args.triage}")
    print(f"source findings {context['source_rows']} · unique {context['source_unique']} · expected {len(EXPECTED_SOURCE_FINDINGS)}")
    print(f"AU acceptance ids {context['rows']} · unique {context['unique_rows']} · expected {len(EXPECTED_AU)}")
    print(f"AU ownership declarations {context['owned']} · unique {context['unique_owned']} · expected {len(EXPECTED_AU)}")
    print(f"proposal WPs {context['proposal_wps']} · existing-WP extensions {context['extensions']}")
    print(f"max WP items after AU extensions {context['max_wp_items']} · A/AU collisions {len(context['a_collisions'])}")
    for rename_from, rename_to in WP_RENAMES.items():
        print(f"  coordinated WP rename: {rename_from} (AU) -> {rename_to}")
    for failure in failures:
        print(f"  BLOCK: {failure}")
    if ok:
        print("\nau-check: PASS — 30 source findings / 31 proposed AU acceptance ids structurally owned exactly once")
        return 0
    print("\nau-check: BLOCKED")
    return 1


if __name__ == "__main__":
    sys.exit(main())
