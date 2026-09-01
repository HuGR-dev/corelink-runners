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
from dataclasses import dataclass
from pathlib import Path


AU_RE = re.compile(r"\bAU\d+\.\d+(?:[a-z])?\b")
A_RE = re.compile(r"\bA\d+\.\d+\b")
WP_RE = re.compile(r"\bT\d+-W\d+[A-Za-z]*\b")
INV_RE = re.compile(r"\bINV-\d+\b")
KNOWN_INVARIANTS = {f"INV-{i}" for i in range(1, 9)}

LEGACY_PROPOSAL_WPS = {"T3-W8", "T8-W3", "T2-W5"}
EXPECTED_PROPOSAL_WAVES = {
    "T3-W14": 2,
    "T3-W9": 2,
    "T8-W5": 2,
    "T8-W6": 3,
    "T3-W10": 1,
    "T8-W4": 1,
    "T5-W3": 1,
    "T7-W4": 1,
    "T7-W5": 3,
    "T2-W6": 3,
}
EXPECTED_EXTENSION_WAVES = {
    "T4-W1": 2,
    "T8-W1": 2,
    "T6-W6": 3,
    "T3-W5": 4,
    "T5-W1": 1,
}
ALLOWED_BUCKETS = {
    "W0-unblock",
    "W1-parallel",
    "W2-serial-worker",
    "W3-live-proof",
    "W4-post-decision",
    "DECISION",
    "ARMING",
    "RELAY",
    "DOCS-sweep",
    "CLEAN-no-action",
    "DEFER-needs-waiver",
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


@dataclass(frozen=True)
class Placement:
    item: str
    owner: str
    invariants: frozenset[str]
    source: str
    bucket: str
    wave: int


def read_wp_catalog(
    path: Path,
) -> tuple[
    dict[str, list[str]], dict[str, list[str]], dict[str, str], dict[str, float]
]:
    """Read the literal WP dictionary from wp-check.py without executing it."""

    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    wp_value: dict[str, object] | None = None
    documented_scopes: dict[str, str] = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "WP"
            for target in node.targets
        ):
            wp_value = ast.literal_eval(node.value)
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "DOC_SCOPE"
            for target in node.targets
        ):
            documented_scopes = ast.literal_eval(node.value)
    if wp_value is not None:
        items = {name: list(spec[0]) for name, spec in wp_value.items()}
        invariants = {name: list(spec[1]) for name, spec in wp_value.items()}
        scopes = {
            name: documented_scopes.get(name, str(spec[2]))
            for name, spec in wp_value.items()
        }
        waves = {name: float(spec[3]) for name, spec in wp_value.items()}
        return items, invariants, scopes, waves
    raise ValueError(f"could not find literal WP catalog in {path}")


def section_between(document: str, start: str, end: str) -> str:
    """Return a section delimited by two unique, exact Markdown headings."""

    lines = document.splitlines(keepends=True)
    starts = [index for index, line in enumerate(lines) if line.rstrip("\r\n") == start]
    ends = [index for index, line in enumerate(lines) if line.rstrip("\r\n") == end]
    if len(starts) != 1 or len(ends) != 1:
        raise ValueError(
            f"section headings must occur exactly once: {start!r}={len(starts)}, {end!r}={len(ends)}"
        )
    if starts[0] >= ends[0]:
        raise ValueError(
            f"section heading order is invalid: {start!r} must precede {end!r}"
        )
    return "".join(lines[starts[0] : ends[0]])


def parse_rows(document: str) -> tuple[list[Placement], list[str], list[str], set[str]]:
    """Parse source rows and their AU acceptance ids.

    A source finding normally has one acceptance id.  M3 is intentionally the
    exception: its one source row owns AU3.23a (test) and AU3.23b (probe), and
    the two WP names in the wave/WP cell align with those ids.
    """

    section = section_between(
        document, "## 2. Per-finding placement", "## 3. Structural consequences"
    )
    rows: list[Placement] = []
    errors: list[str] = []
    sources: list[str] = []
    mentioned_wps: set[str] = set()
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
        principal_ids = A_RE.findall(columns[8])
        if principal_ids:
            errors.append(
                f"placement row {line_no} uses principal A ids in the AU item cell: {principal_ids}"
            )
        # id | origin | severity | bucket | wave/WP | invariants | ...
        source_match = re.search(r"\b(?:union-\d+|RH[59]|M(?:3|5|19))\b", columns[1])
        if not source_match:
            errors.append(f"placement row {line_no} has no source finding id")
            continue
        source = source_match.group()
        sources.append(source)
        # Only bold names are owners.  Dependencies in this cell can mention
        # predecessor WPs (for example T4-W4) and are not AU ownership.
        owner_matches = list(re.finditer(r"\*\*(T\d+-W\d+[A-Za-z]*)\*\*", columns[5]))
        owners = [match.group(1) for match in owner_matches]
        mentioned_wps.update(WP_RE.findall("|".join(columns[5:8])))
        legacy = sorted(set(owners) & LEGACY_PROPOSAL_WPS)
        if legacy:
            errors.append(
                f"placement row {line_no} reuses legacy proposal WP ids: {legacy}"
            )
        if len(owners) != len(ids):
            errors.append(
                f"placement row {line_no} has {len(ids)} AU ids but {len(owners)} WPs: {ids} / {owners}"
            )
            continue
        bucket_tokens = set(
            re.findall(
                r"\b(?:W[0-4]-(?:unblock|parallel|serial-worker|live-proof|post-decision)|DECISION|ARMING|RELAY|DOCS-sweep|CLEAN-no-action|DEFER-needs-waiver)\b",
                columns[4],
            )
        )
        if not bucket_tokens or not bucket_tokens <= ALLOWED_BUCKETS:
            errors.append(
                f"placement row {line_no} has no valid bucket: {columns[4].strip()!r}"
            )
        invariants = frozenset(INV_RE.findall(columns[6]))
        for item, owner, owner_match in zip(ids, owners, owner_matches):
            preceding = columns[5][: owner_match.start()]
            waves = re.findall(r"\bW([0-4])\b", preceding)
            if not waves:
                errors.append(
                    f"placement row {line_no} has no wave before owner {owner}"
                )
                continue
            wave = int(waves[-1])
            required_bucket = {
                1: "W1-parallel",
                2: "W2-serial-worker",
                3: "W3-live-proof",
                4: "W4-post-decision",
            }.get(wave)
            if (
                required_bucket
                and required_bucket not in bucket_tokens
                and not (wave == 1 and bucket_tokens & {"DOCS-sweep", "RELAY"})
            ):
                errors.append(
                    f"placement row {line_no} owner {owner} is W{wave} but bucket is {sorted(bucket_tokens)}"
                )
            rows.append(
                Placement(
                    item,
                    owner,
                    invariants,
                    source,
                    "+".join(sorted(bucket_tokens)),
                    wave,
                )
            )
    return rows, errors, sources, mentioned_wps


def parse_wp_declarations(
    document: str,
) -> tuple[
    dict[str, list[str]],
    dict[str, list[str]],
    dict[str, int],
    dict[str, str],
    list[str],
]:
    """Parse new-WP rows and the five existing-WP AU extensions."""

    section = section_between(
        document,
        "## 3. Structural consequences",
        "## 4. Findings",
    )
    declarations: dict[str, list[str]] = {}
    errors: list[str] = []

    table_marker = (
        "**New WPs and their item counts** (all ≤ the `wp-check.py` 4-item ceiling):"
    )
    extension_marker = "**Extensions to existing WPs:**"
    table_markers = [m.start() for m in re.finditer(re.escape(table_marker), section)]
    extension_markers = [
        m.start() for m in re.finditer(re.escape(extension_marker), section)
    ]
    if len(table_markers) != 1 or len(extension_markers) != 1:
        return (
            declarations,
            {},
            {},
            {},
            [
                "new-WP and extension markers must each occur exactly once "
                f"(new={len(table_markers)}, extensions={len(extension_markers)})"
            ],
        )
    table_start = table_markers[0]
    extensions_start = extension_markers[0]
    if extensions_start <= table_start:
        return declarations, {}, {}, {}, ["new-WP table must precede extensions"]

    table = section[table_start:extensions_start]
    declaration_waves: dict[str, int] = {}
    declaration_scopes: dict[str, str] = {}
    for line_no, line in enumerate(table.splitlines(), 1):
        if not line.startswith("|") or "*(new)*" not in line:
            continue
        columns = line.split("|")
        if len(columns) < 6:
            errors.append(f"new-WP row {line_no} has too few columns")
            continue
        wp_match = WP_RE.search(columns[1])
        ids = AU_RE.findall(columns[3])
        if not wp_match:
            errors.append(f"new-WP row {line_no} has no WP name")
            continue
        name = wp_match.group()
        if name in LEGACY_PROPOSAL_WPS:
            errors.append(f"new-WP table reuses legacy proposal id {name}")
        if name in declarations:
            errors.append(f"new-WP {name} is declared more than once")
        declarations[name] = ids
        wave_match = re.search(r"\bW([0-4])\b", columns[2])
        if not wave_match:
            errors.append(f"new-WP {name} has no declared wave")
        else:
            declaration_waves[name] = int(wave_match.group(1))
        scope = columns[4].strip()
        if not scope or scope == "—":
            errors.append(f"new-WP {name} has no exclusive scope")
        declaration_scopes[name] = scope
        duplicate_ids = sorted(
            item for item, count in Counter(ids).items() if count > 1
        )
        if duplicate_ids:
            errors.append(f"new-WP {name} physically repeats AU ids: {duplicate_ids}")

    extension_section = section[extensions_start:]
    paragraph_end = extension_section.find("\n\n")
    if paragraph_end < 0:
        errors.append("extension declaration must be one terminated Markdown paragraph")
        extension_section = ""
    else:
        extension_section = extension_section[:paragraph_end]
    extensions: dict[str, list[str]] = defaultdict(list)
    # The extension sentence can wrap across lines, and one WP may have more
    # than one ``+AU`` token.  Associate tokens with the WP that precedes them
    # until the next WP name rather than assuming one token per WP.
    wp_matches = list(WP_RE.finditer(extension_section))
    for index, match in enumerate(wp_matches):
        end = (
            wp_matches[index + 1].start()
            if index + 1 < len(wp_matches)
            else len(extension_section)
        )
        ids = AU_RE.findall(extension_section[match.end() : end])
        name = match.group()
        if name in LEGACY_PROPOSAL_WPS:
            errors.append(f"extension list reuses legacy proposal id {name}")
        extensions[name].extend(ids)
    for name, ids in extensions.items():
        duplicate_ids = sorted(
            item for item, count in Counter(ids).items() if count > 1
        )
        if duplicate_ids:
            errors.append(
                f"extension {name} physically repeats AU ids: {duplicate_ids}"
            )
    return declarations, dict(extensions), declaration_waves, declaration_scopes, errors


def acceptance_ids(plan_path: Path) -> tuple[set[str], list[str], list[str]]:
    document = plan_path.read_text(encoding="utf-8")
    suite = section_between(
        document,
        "## 3. The acceptance suite (the completeness anchor)",
        "## 4. Owner decisions",
    )
    ids = re.findall(
        r"^\|\s*(?:\*\*)?(?:★)?(?:~~)?(A\d+\.\d+)(?:~~)?(?:\*\*)?\s*\|",
        suite,
        re.MULTILINE,
    )
    duplicates = sorted(item for item, count in Counter(ids).items() if count > 1)
    leaked_au_rows = re.findall(
        r"^\|\s*(?:\*\*)?(AU\d+\.\d+(?:[a-z])?)(?:\*\*)?\s*\|",
        suite,
        re.MULTILINE,
    )
    return set(ids), duplicates, leaked_au_rows


def check(
    triage_path: Path, plan_path: Path, wp_path: Path
) -> tuple[bool, list[str], dict[str, object]]:
    failures: list[str] = []
    document = triage_path.read_text(encoding="utf-8")
    rows, parse_errors, sources, mentioned_wps = parse_rows(document)
    failures.extend(parse_errors)
    row_counts = Counter(row.item for row in rows)
    row_owners = {row.item: row.owner for row in rows}
    source_counts = Counter(sources)
    missing_sources = sorted(EXPECTED_SOURCE_FINDINGS - set(source_counts))
    unknown_sources = sorted(set(source_counts) - EXPECTED_SOURCE_FINDINGS)
    duplicate_sources = sorted(
        source for source, count in source_counts.items() if count != 1
    )
    if len(sources) != len(EXPECTED_SOURCE_FINDINGS) or len(source_counts) != len(
        EXPECTED_SOURCE_FINDINGS
    ):
        failures.append(
            f"source findings must contain exactly {len(EXPECTED_SOURCE_FINDINGS)} unique ids "
            f"(rows={len(sources)}, unique={len(source_counts)})"
        )
    if missing_sources:
        failures.append(
            f"source findings missing from placement rows: {missing_sources}"
        )
    if unknown_sources:
        failures.append(f"unknown source findings in placement rows: {unknown_sources}")
    if duplicate_sources:
        failures.append(
            f"source findings repeated in placement rows: {duplicate_sources}"
        )

    (
        declarations,
        extensions,
        declaration_waves,
        declaration_scopes,
        declaration_errors,
    ) = parse_wp_declarations(document)
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
    duplicate_ownership = sorted(
        item for item, count in owned_counts.items() if count != 1
    )
    if missing_ownership:
        failures.append(f"AU ids without a WP declaration: {missing_ownership}")
    if unknown_ownership:
        failures.append(f"unknown AU ids in WP declarations: {unknown_ownership}")
    if duplicate_ownership:
        failures.append(f"AU ids not owned exactly once: {duplicate_ownership}")

    row_declaration_mismatch = sorted(
        item
        for item in EXPECTED_AU
        if item in row_owners and owned_by.get(item) != [row_owners[item]]
    )
    if row_declaration_mismatch:
        failures.append(f"placement/WP ownership mismatch: {row_declaration_mismatch}")
    if row_owners.get("AU7.10") != "T5-W1":
        failures.append(
            "AU7.10 must extend T5-W1; T7-W4 cannot own a file outside its exclusive scope"
        )

    try:
        main_items, main_invariants, main_scopes, main_waves = read_wp_catalog(wp_path)
    except (OSError, SyntaxError, ValueError) as exc:
        failures.append(f"cannot read principal WP catalog: {exc}")
        main_items, main_invariants, main_scopes, main_waves = {}, {}, {}, {}

    main_names = set(main_items)
    proposal_names = set(declarations)
    # After applying the three coordinated renames, proposal names must be
    # disjoint from principal names.  Do not merge proposal items into the
    # principal catalog: they are not suite items until a later delta lands.
    collisions = sorted(proposal_names & main_names)
    if collisions:
        failures.append(f"proposal/principal WP name collisions: {collisions}")

    expected_proposals = {
        "T3-W14",
        "T3-W9",
        "T8-W5",
        "T8-W6",
        "T3-W10",
        "T8-W4",
        "T5-W3",
        "T7-W4",
        "T7-W5",
        "T2-W6",
    }
    if proposal_names != expected_proposals:
        failures.append(
            f"proposal WPs differ from the triage contract: expected "
            f"{sorted(expected_proposals)}, got {sorted(proposal_names)}"
        )
    expected_extensions = {"T4-W1", "T8-W1", "T6-W6", "T3-W5", "T5-W1"}
    if set(extensions) != expected_extensions:
        failures.append(
            f"AU extensions differ from the triage contract: expected "
            f"{sorted(expected_extensions)}, got {sorted(extensions)}"
        )

    legacy_owners = sorted(
        (proposal_names | set(extensions) | set(row_owners.values()))
        & LEGACY_PROPOSAL_WPS
    )
    if legacy_owners:
        failures.append(
            f"legacy proposal WP ids are forbidden as owners: {legacy_owners}"
        )

    if declaration_waves != EXPECTED_PROPOSAL_WAVES:
        failures.append(
            "proposal WP waves differ from the triage contract: "
            f"expected {EXPECTED_PROPOSAL_WAVES}, got {declaration_waves}"
        )
    row_wave_mismatch = sorted(
        row.item
        for row in rows
        if declaration_waves.get(
            row.owner,
            EXPECTED_EXTENSION_WAVES.get(row.owner, main_waves.get(row.owner)),
        )
        != row.wave
    )
    if row_wave_mismatch:
        failures.append(f"placement/declaration wave mismatch: {row_wave_mismatch}")

    plan_document = plan_path.read_text(encoding="utf-8")
    known_plan_wps = set(WP_RE.findall(plan_document))
    known_wps = main_names | known_plan_wps | proposal_names
    unknown_wps = sorted(mentioned_wps - known_wps)
    if unknown_wps:
        failures.append(f"placement rows mention unknown WP ids: {unknown_wps}")
    unknown_extensions = sorted(set(extensions) - (main_names | known_plan_wps))
    if unknown_extensions:
        failures.append(
            f"AU extensions target unknown principal WPs: {unknown_extensions}"
        )

    # Proposal scope collisions can be checked exactly.  The two intentional
    # serial exceptions must say so in the declaration rather than relying on
    # reviewer memory.
    parallel_scopes: dict[tuple[int, str], list[str]] = defaultdict(list)
    for wp, scope in declaration_scopes.items():
        wave = declaration_waves.get(wp)
        declaration_line = next(
            (line for line in document.splitlines() if line.startswith(f"| **{wp}**")),
            "",
        )
        is_serial_exception = "serial" in declaration_line.lower()
        if wave is not None and wave != 2 and not is_serial_exception:
            normalized_scope = (
                re.sub(r"\s+", " ", re.sub(r"[`*]", "", scope)).strip().lower()
            )
            parallel_scopes[(wave, normalized_scope)].append(wp)
    scope_collisions = {
        key: owners for key, owners in parallel_scopes.items() if len(owners) > 1
    }
    if scope_collisions:
        failures.append(f"parallel proposal WP scope collisions: {scope_collisions}")
    t3w10_line = next(
        (line for line in document.splitlines() if line.startswith("| **T3-W10**")), ""
    )
    if "serial after T4-W4" not in t3w10_line:
        failures.append("T3-W10 must explicitly remain serial after T4-W4")

    # T5-W3 owns the GitHub Action file inside T5-W2's broad integrations
    # scope.  This proposal deliberately requires an explicit serial edge;
    # an implicit or not-yet-decided carve-out is not staging evidence.
    t5w2_scope = main_scopes.get("T5-W2", "").lower()
    t5w3_scope = declaration_scopes.get("T5-W3", "").lower()
    t5w3_line = next(
        (line for line in document.splitlines() if line.startswith("| **T5-W3**")),
        "",
    )
    t5_serial_edge = "serial after T5-W2" in t5w3_line or any(
        edge in document for edge in ("T5-W2 -> T5-W3", "T5-W2 → T5-W3")
    )
    if (
        "T5-W3" in proposal_names
        and "integrations" in t5w2_scope
        and "integrations/github-actions/action.yml" in t5w3_scope
        and not t5_serial_edge
    ):
        failures.append(
            "parallel scope collision: T5-W2/T5-W3 requires an explicit T5-W2 -> T5-W3 serial edge"
        )
    t5w1_scope = main_scopes.get("T5-W1", "").lower()
    t7w4_scope = declaration_scopes.get("T7-W4", "").lower()
    memoize_readme = "actions/corelink-memoize/readme.md"
    t5w1_owns_memoize = memoize_readme in t5w1_scope or "actions-readme" in t5w1_scope
    if t5w1_owns_memoize and memoize_readme in t7w4_scope:
        failures.append(
            "parallel scope collision: T5-W1 owns the memoize actions README while T7-W4 also declares it"
        )

    bad_invariants: dict[str, list[str]] = {}
    for row in rows:
        unknown = sorted(row.invariants - KNOWN_INVARIANTS)
        if unknown:
            bad_invariants[row.item] = unknown
    if bad_invariants:
        failures.append(f"AU rows declare unknown invariants: {bad_invariants}")
    for wp, ids in declarations.items():
        if not set().union(
            *(
                set(next((row.invariants for row in rows if row.item == i), set()))
                for i in ids
            )
        ):
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
        a_ids, duplicate_a_ids, leaked_au_rows = acceptance_ids(plan_path)
    except (OSError, ValueError) as exc:
        failures.append(f"cannot read principal acceptance suite: {exc}")
        a_ids = set()
        duplicate_a_ids = []
        leaked_au_rows = []
    if duplicate_a_ids:
        failures.append(
            f"principal acceptance suite physically repeats A ids: {duplicate_a_ids}"
        )
    if leaked_au_rows:
        failures.append(
            f"AU ids leaked into the principal acceptance suite: {leaked_au_rows}"
        )
    # Compare canonical numeric portions as well as literal strings.  An AU
    # id may intentionally shadow an A id numerically while staged, but that
    # fact must remain visible rather than being reported as zero collisions.
    au_numeric = {item.removeprefix("AU"): item for item in EXPECTED_AU}
    numeric_collisions = sorted(
        item for item in a_ids if item.removeprefix("A") in au_numeric
    )
    a_collisions = numeric_collisions

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
    except (OSError, ValueError) as exc:
        print(f"au-check: BLOCKED — {exc}")
        return 1

    print(f"triage {args.triage}")
    print(
        f"source findings {context['source_rows']} · unique {context['source_unique']} · expected {len(EXPECTED_SOURCE_FINDINGS)}"
    )
    print(
        f"AU acceptance ids {context['rows']} · unique {context['unique_rows']} · expected {len(EXPECTED_AU)}"
    )
    print(
        f"AU ownership declarations {context['owned']} · unique {context['unique_owned']} · expected {len(EXPECTED_AU)}"
    )
    print(
        f"proposal WPs {context['proposal_wps']} · existing-WP extensions {context['extensions']}"
    )
    print(
        f"max WP items after AU extensions {context['max_wp_items']} · numeric A/AU shadows {len(context['a_collisions'])}"
    )
    for failure in failures:
        print(f"  BLOCK: {failure}")
    if ok:
        print(
            "\nau-check: AU STAGING PASS — 30 source findings / 31 proposed AU acceptance ids structurally owned exactly once; not freeze evidence"
        )
        return 0
    print("\nau-check: BLOCKED")
    return 1


if __name__ == "__main__":
    sys.exit(main())
