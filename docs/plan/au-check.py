#!/usr/bin/env python3
"""Check the proposed AU layer without changing the frozen acceptance suite.

The 247-finding ``plan-check.py`` gate and the acceptance/WP ``wp-check.py``
gate intentionally remain independent of this proposal.  This checker is the
mechanical seam between ``union-triage-remaining.md`` and a future suite delta:
it checks that the triage has exactly 30 source findings and 33 AU acceptance
ids (union-06, union-09, and M3 each split repo tests from live probes), that each
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
import fnmatch
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
    "T8-W7": 3,
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
    "T6-W10": 3,
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

EXPECTED_SOURCE_TO_AU = {
    "union-06": ("AU4.16a", "AU4.16b"),
    "union-07": ("AU3.19",),
    "union-08": ("AU4.14",),
    "union-09": ("AU3.26a", "AU3.26b"),
    "union-10": ("AU3.25",),
    "union-11": ("AU4.15",),
    "union-12": ("AU5.10",),
    "union-13": ("AU4.18",),
    "union-14": ("AU3.20",),
    "union-15": ("AU6.17",),
    "union-16": ("AU7.7",),
    "union-17": ("AU5.11",),
    "union-18": ("AU7.8",),
    "union-19": ("AU1.8",),
    "union-20": ("AU7.9",),
    "union-21": ("AU3.22",),
    "union-22": ("AU1.9",),
    "union-23": ("AU7.10",),
    "union-24": ("AU5.12",),
    "union-25": ("AU4.17",),
    "union-26": ("AU3.21",),
    "union-27": ("AU7.11",),
    "union-28": ("AU4.19",),
    "union-29": ("AU7.12",),
    "union-30": ("AU3.27",),
    "RH5": ("AU7.6",),
    "RH9": ("AU6.16",),
    "M3": ("AU3.23a", "AU3.23b"),
    "M5": ("AU3.28",),
    "M19": ("AU3.24",),
}
EXPECTED_AU_KINDS = {
    item: kind
    for kind, items in {
        "probe": {
            "AU1.8",
            "AU3.23b",
            "AU3.26b",
            "AU4.16b",
            "AU4.19",
            "AU6.17",
            "AU7.11",
            "AU7.12",
        },
        "test": {
            "AU1.9",
            "AU3.19",
            "AU3.20",
            "AU3.21",
            "AU3.22",
            "AU3.23a",
            "AU3.24",
            "AU3.25",
            "AU3.26a",
            "AU3.27",
            "AU3.28",
            "AU4.14",
            "AU4.15",
            "AU4.16a",
            "AU4.17",
            "AU4.18",
            "AU5.10",
            "AU5.11",
            "AU5.12",
            "AU6.16",
            "AU7.6",
            "AU7.7",
            "AU7.8",
            "AU7.9",
            "AU7.10",
        },
    }.items()
    for item in items
}
EXPECTED_AU = set(EXPECTED_AU_KINDS)
EXPECTED_SOURCE_FINDINGS = set(EXPECTED_SOURCE_TO_AU)
EXPECTED_BUCKET_FINDINGS = {
    "W0-unblock": (),
    "W1-parallel": (
        "union-09",
        "union-10",
        "union-16",
        "union-17",
        "union-18",
        "union-22",
        "union-24",
    ),
    "W2-serial-worker": (
        "union-06",
        "union-07",
        "union-08",
        "union-11",
        "union-12",
        "union-13",
        "union-14",
        "union-21",
        "union-25",
        "union-26",
        "RH5p",
        "RH9p",
        "M3p",
        "M19p",
    ),
    "W3-live-proof": (
        "union-15",
        "union-19",
        "union-27",
        "union-28",
        "union-29",
    ),
    "W4-post-decision": ("union-30", "M5p"),
    "DECISION": (),
    "ARMING": (),
    "RELAY": ("union-23",),
    "DOCS-sweep": ("union-20",),
    "CLEAN-no-action": (),
    "DEFER-needs-waiver": (),
}
EXPECTED_PROPOSAL_WPS = set(EXPECTED_PROPOSAL_WAVES)
EXPECTED_EXTENSION_WPS = set(EXPECTED_EXTENSION_WAVES)
STAGED_DECISION_IDS = {"D11", "D12", "D13"}
STAGED_OWNER_IDS = {"O-CFINVENTORY", "O-CFRATE"}
STAGED_RELAY_IDS = {"R6"}
TOMBSTONED_WPS = {"T4-W3"}

PLACEMENT_HEADER = [
    "id",
    "origin",
    "sev",
    "bucket",
    "phase / wp",
    "inv",
    "dependency",
    "proposed acceptance item",
    "source evidence revalidated at b70deae / 3fe8d06",
]
SUMMARY_HEADER = ["bucket", "n", "findings"]
DECLARATION_HEADER = ["wp", "owns", "exact exclusive write scope (the x)"]
DAG_HEADER = [
    "node",
    "phase / wave",
    "exact hard predecessors",
    "exclusive path atoms; artifact filename",
    "lane",
]
AU_DECL_RE = re.compile(r"\*\*(AU\d+\.\d+(?:[a-z])?)\s+—\s+([a-z+]+):\*\*")
DEPENDENCY_TARGET_RE = re.compile(
    r"\b(?:T\d+-W\d+[A-Za-z]*|AU\d+\.\d+(?:[a-z])?|A\d+\.\d+|"
    r"O(?:1|-[A-Z][A-Z0-9-]*)|R\d+|D\d+|union-\d{2}|"
    r"(?:adopt|billing-money-path|ci-cd|conf|deploy|docs-truth|e2e|fabric-core|"
    r"fabricd|fabricd-deploy|gap|hist|live-probe|runner-core|sc|sec|spawn-cf)-\d{2})\b"
)


@dataclass(frozen=True)
class Placement:
    item: str
    kind: str
    owner: str
    invariants: frozenset[str]
    source: str
    bucket: str
    dependency: str


def markdown_cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def plain_markdown(value: str) -> str:
    value = value.replace("*", "").replace("`", "")
    return re.sub(r"\s+", " ", value).strip()


def exact_markdown_table(
    section: str,
    expected_header: list[str],
    label: str,
    *,
    start_after: str | None = None,
) -> tuple[list[tuple[int, list[str]]], list[str]]:
    """Parse one complete table; never skip malformed or opaque body rows."""

    errors: list[str] = []
    lines = section.splitlines()
    offset = 0
    if start_after is not None:
        markers = [index for index, line in enumerate(lines) if line == start_after]
        if len(markers) != 1:
            return [], [
                f"{label} marker must occur exactly once: {start_after!r}={len(markers)}"
            ]
        offset = markers[0] + 1

    candidates = [
        index
        for index, line in enumerate(lines[offset:], offset)
        if line.startswith("|")
    ]
    if not candidates:
        return [], [f"{label} Markdown table is missing"]
    start = candidates[0]
    table: list[tuple[int, str]] = []
    for index in range(start, len(lines)):
        line = lines[index]
        if not line.startswith("|"):
            break
        table.append((index + 1, line))
    extra_pipe_lines = [
        index + 1
        for index, line in enumerate(lines[table[-1][0] :], table[-1][0])
        if line.startswith("|")
    ]
    if extra_pipe_lines:
        errors.append(
            f"{label} contains additional pipe rows outside its one exhaustive table: {extra_pipe_lines}"
        )
    if len(table) < 3:
        return [], errors + [f"{label} Markdown table has no complete body"]

    header = [plain_markdown(cell).lower() for cell in markdown_cells(table[0][1])]
    if header != expected_header:
        errors.append(
            f"{label} table header mismatch: expected {expected_header}, got {header}"
        )
    separator = markdown_cells(table[1][1])
    if len(separator) != len(expected_header) or any(
        not re.fullmatch(r":?-{3,}:?", cell) for cell in separator
    ):
        errors.append(f"{label} has a malformed Markdown table separator")

    body: list[tuple[int, list[str]]] = []
    for line_no, line in table[2:]:
        cells = markdown_cells(line)
        if len(cells) != len(expected_header):
            errors.append(
                f"{label} row {line_no} has {len(cells)} cells; expected {len(expected_header)}"
            )
        else:
            body.append((line_no, cells))
    return body, errors


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


def read_finding_catalog(path: Path) -> set[str]:
    """Read finding ids from literal ``b(bucket, ids)`` calls without execution."""

    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    findings: set[str] = set()
    for node in tree.body:
        if not isinstance(node, ast.Expr) or not isinstance(node.value, ast.Call):
            continue
        call = node.value
        if not isinstance(call.func, ast.Name) or call.func.id != "b":
            continue
        if len(call.args) != 2:
            raise ValueError(f"malformed b(...) registry call in {path}")
        ids = ast.literal_eval(call.args[1])
        if not isinstance(ids, str):
            raise ValueError(f"non-literal finding registry in {path}")
        for item in ids.split():
            if item in findings:
                raise ValueError(f"duplicate principal finding id {item} in {path}")
            findings.add(item)
    if not findings:
        raise ValueError(f"could not find principal finding registry in {path}")
    return findings


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
        document,
        "## 2. Per-finding placement",
        "## 3. Staged ownership consequences",
    )
    rows: list[Placement] = []
    table_rows, errors = exact_markdown_table(
        section, PLACEMENT_HEADER, "per-finding placement"
    )
    sources: list[str] = []
    mentioned_wps: set[str] = set()
    for line_no, columns in table_rows:
        # The dependency/evidence cells may cite an AU id too.  The proposed
        # acceptance-item cell is the authoritative per-finding occurrence.
        acceptance_cell = columns[7]
        declarations = AU_DECL_RE.findall(acceptance_cell)
        ids = [item for item, _ in declarations]
        kinds = [kind for _, kind in declarations]
        physical_ids = AU_RE.findall(acceptance_cell)
        if physical_ids != ids:
            errors.append(
                f"placement row {line_no} has malformed AU declaration(s): "
                f"physical={physical_ids}, parsed={ids}"
            )
        principal_ids = A_RE.findall(acceptance_cell)
        if principal_ids:
            errors.append(
                f"placement row {line_no} uses principal A ids in the AU item cell: {principal_ids}"
            )
        source_text = plain_markdown(columns[0])
        source_match = re.fullmatch(
            r"(union-\d{2}|RH5|RH9|M3|M5|M19)(?: \(partial\))?", source_text
        )
        if not source_match:
            errors.append(
                f"placement row {line_no} has an opaque source finding cell: {columns[0]!r}"
            )
            continue
        source = source_match.group(1)
        sources.append(source)
        # Only bold names are owners.  Dependencies in this cell can mention
        # predecessor WPs (for example T4-W4) and are not AU ownership.
        owner_matches = list(re.finditer(r"\*\*(T\d+-W\d+[A-Za-z]*)\*\*", columns[4]))
        owners = [match.group(1) for match in owner_matches]
        mentioned_wps.update(WP_RE.findall("|".join(columns[4:7])))
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
                columns[3],
            )
        )
        if not bucket_tokens or not bucket_tokens <= ALLOWED_BUCKETS:
            errors.append(
                f"placement row {line_no} has no valid bucket: {columns[3].strip()!r}"
            )
        invariants = frozenset(INV_RE.findall(columns[5]))
        for item, kind, owner in zip(ids, kinds, owners):
            rows.append(
                Placement(
                    item,
                    kind,
                    owner,
                    invariants,
                    source,
                    "+".join(sorted(bucket_tokens)),
                    columns[6],
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
        "## 3. Staged ownership consequences",
        "## 4. Findings",
    )
    declarations: dict[str, list[str]] = {}
    errors: list[str] = []

    table_marker = "**New WPs and their item counts** (all ≤ the four-item ceiling):"
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
    declaration_rows, table_errors = exact_markdown_table(
        table,
        DECLARATION_HEADER,
        "new-WP declaration",
        start_after=table_marker,
    )
    errors.extend(table_errors)
    for line_no, columns in declaration_rows:
        wp_matches = WP_RE.findall(columns[0])
        ids = AU_RE.findall(columns[1])
        if len(wp_matches) != 1 or "*(new)*" not in columns[0]:
            errors.append(
                f"new-WP row {line_no} has an opaque WP label: {columns[0]!r}"
            )
            continue
        name = wp_matches[0]
        if name in LEGACY_PROPOSAL_WPS:
            errors.append(f"new-WP table reuses legacy proposal id {name}")
        if name in declarations:
            errors.append(f"new-WP {name} is declared more than once")
        declarations[name] = ids
        scope = columns[2].strip()
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


def validate_summary(document: str) -> list[str]:
    """Check the rendered summary table and prose totals against the contract."""

    errors: list[str] = []
    section = section_between(
        document, "## 1. Summary counts", "## 2. Per-finding placement"
    )
    rows, table_errors = exact_markdown_table(section, SUMMARY_HEADER, "summary counts")
    errors.extend(table_errors)
    rendered: dict[str, tuple[int, tuple[str, ...]]] = {}
    rendered_total: int | None = None
    for line_no, columns in rows:
        bucket = plain_markdown(columns[0])
        count_text = plain_markdown(columns[1])
        if not count_text.isdigit():
            errors.append(
                f"summary row {line_no} has a non-integer count: {columns[1]!r}"
            )
            continue
        count = int(count_text)
        if bucket == "total":
            if rendered_total is not None:
                errors.append("summary table physically repeats the total row")
            rendered_total = count
            if plain_markdown(columns[2]) not in {"", "—"}:
                errors.append("summary total findings cell must be an em dash")
            continue
        findings = (
            tuple(
                re.findall(
                    r"union-\d{2}|RH[59]p|M(?:3|5|19)p",
                    plain_markdown(columns[2]),
                )
            )
            if count
            else ()
        )
        if bucket in rendered:
            errors.append(f"summary table physically repeats bucket {bucket}")
        rendered[bucket] = (count, findings)

    if set(rendered) != set(EXPECTED_BUCKET_FINDINGS):
        errors.append(
            "summary buckets differ from the triage contract: "
            f"missing={sorted(set(EXPECTED_BUCKET_FINDINGS) - set(rendered))}, "
            f"unexpected={sorted(set(rendered) - set(EXPECTED_BUCKET_FINDINGS))}"
        )
    for bucket, expected_findings in EXPECTED_BUCKET_FINDINGS.items():
        if bucket not in rendered:
            continue
        count, findings = rendered[bucket]
        if count != len(expected_findings) or findings != expected_findings:
            errors.append(
                f"summary bucket {bucket} mismatch: expected "
                f"({len(expected_findings)}, {expected_findings}), got ({count}, {findings})"
            )
    if rendered_total != len(EXPECTED_SOURCE_FINDINGS):
        errors.append(
            f"summary rendered total must be {len(EXPECTED_SOURCE_FINDINGS)}, got {rendered_total}"
        )

    prose_totals = {
        "acceptance": (
            re.findall(
                r"\*\*Acceptance items proposed: (\d+) for (\d+) source findings\*\*",
                section,
            ),
            (len(EXPECTED_AU), len(EXPECTED_SOURCE_FINDINGS)),
        ),
        "new WPs": (
            re.findall(r"\*\*New WPs named: (\d+)\*\*", section),
            (len(EXPECTED_PROPOSAL_WPS),),
        ),
        "existing WPs": (
            re.findall(r"\*\*Existing WPs extended: (\d+)\*\*", section),
            (len(EXPECTED_EXTENSION_WPS),),
        ),
    }
    for label, (matches, expected) in prose_totals.items():
        parsed = [
            tuple(
                int(value)
                for value in (match if isinstance(match, tuple) else (match,))
            )
            for match in matches
        ]
        if parsed != [expected]:
            errors.append(
                f"summary rendered {label} total mismatch: expected {[expected]}, got {parsed}"
            )
    return errors


def principal_dependency_registries(
    plan_document: str,
) -> tuple[set[str], set[str], set[str], list[str]]:
    """Read D/O/R ids only from their authoritative canonical sections."""

    errors: list[str] = []
    decisions_section = section_between(
        plan_document, "## 4. Owner decisions", "## 5. Waves"
    )
    decision_rows, table_errors = exact_markdown_table(
        decisions_section,
        ["id", "decision", "blocks"],
        "principal owner decisions",
    )
    errors.extend(table_errors)
    decisions: set[str] = set()
    for line_no, columns in decision_rows:
        matches = re.findall(r"\bD\d+\b", columns[0])
        if len(matches) != 1:
            errors.append(
                f"principal owner-decision row {line_no} has an opaque id: {columns[0]!r}"
            )
        elif matches[0] in decisions:
            errors.append(f"principal owner decision repeats {matches[0]}")
        else:
            decisions.add(matches[0])

    arming_section = section_between(
        plan_document,
        "## 6. Owner arming (config only)",
        "## 7. Cross-repo relays (8 findings)",
    )
    arming_rows, table_errors = exact_markdown_table(
        arming_section,
        ["id", "action", "dep"],
        "principal owner armings",
    )
    errors.extend(table_errors)
    armings: set[str] = set()
    for line_no, columns in arming_rows:
        matches = re.findall(r"\bO(?:1|-[A-Z][A-Z0-9-]*)\b", columns[0])
        if not matches:
            errors.append(
                f"principal owner-arming row {line_no} has no registered id: {columns[0]!r}"
            )
        repeated = sorted(item for item, count in Counter(matches).items() if count > 1)
        if repeated or armings & set(matches):
            errors.append(
                f"principal owner arming repeats ids: {repeated or sorted(armings & set(matches))}"
            )
        armings.update(matches)

    relay_section = section_between(
        plan_document,
        "## 7. Cross-repo relays (8 findings)",
        "## 8. Gates and the done-gate",
    )
    relay_ids = re.findall(r"\*\*(R\d+)\*\*", relay_section)
    relays = set(relay_ids)
    if len(relay_ids) != len(relays) or not relays:
        errors.append(
            f"principal relay registry must contain unique bold ids, got {relay_ids}"
        )
    return decisions, armings, relays, errors


def validate_dependency_targets(
    dependencies: list[tuple[str, str]],
    *,
    wps: set[str],
    acceptance: set[str],
    findings: set[str],
    decisions: set[str],
    armings: set[str],
    relays: set[str],
) -> list[str]:
    """Reject dependency-like ids that are absent from an authoritative registry."""

    errors: list[str] = []
    authorized = (
        wps
        | acceptance
        | EXPECTED_AU
        | decisions
        | STAGED_DECISION_IDS
        | armings
        | STAGED_OWNER_IDS
        | relays
        | STAGED_RELAY_IDS
        | findings
        | EXPECTED_SOURCE_FINDINGS
        | {f"union-{i:02d}" for i in range(1, 6)}
    )
    for owner, dependency in dependencies:
        targets = DEPENDENCY_TARGET_RE.findall(dependency)
        tombstones = sorted(set(targets) & TOMBSTONED_WPS)
        if tombstones:
            errors.append(
                f"{owner} dependency targets deleted/tombstoned WP ids: {tombstones}"
            )
        unknown = sorted(set(targets) - authorized - TOMBSTONED_WPS)
        if unknown:
            errors.append(
                f"{owner} dependency targets ids outside authoritative registries: {unknown}"
            )
    return errors


def scope_atoms(scope: str) -> tuple[str, ...]:
    """Extract normalized path/glob atoms from a prose scope declaration."""

    fragments: list[str] = []
    for segment in scope.split(";"):
        segment_fragments = re.findall(r"`([^`]+)`", segment)
        if "excluding" in segment.lower() and segment_fragments:
            segment_fragments = segment_fragments[:1]
        fragments.extend(segment_fragments)
    if not fragments:
        fragments = [scope]
    atoms: list[str] = []
    for fragment in fragments:
        for raw in re.split(r"\s*(?:,|\+)\s*", fragment):
            atom = plain_markdown(raw).lower().strip(" .;:")
            atom = re.sub(r"^(?:new|every)\s+", "", atom)
            atom = re.sub(r"\s+\([^)]*\)$", "", atom)
            if not atom or atom.startswith("probe:"):
                continue
            if "/" not in atom and not re.search(r"[.*][a-z0-9{}*,?-]*$", atom):
                continue
            if atom.endswith("/"):
                atom += "**"
            atoms.append(atom)
    return tuple(dict.fromkeys(atoms))


def scope_exclusions(scope: str) -> tuple[str, ...]:
    """Resolve backticked exclusions relative to their preceding broad atom."""

    exclusions: list[str] = []
    for segment in scope.split(";"):
        if "excluding" not in segment.lower():
            continue
        fragments = re.findall(r"`([^`]+)`", segment)
        if len(fragments) < 2:
            continue
        base = re.split(r"[*?[{]", fragments[0].lower(), maxsplit=1)[0]
        for raw in fragments[1:]:
            atom = plain_markdown(raw).lower().strip(" .;:")
            if not atom:
                continue
            if not atom.startswith(base):
                atom = base + atom.lstrip("/")
            if atom.endswith("/"):
                atom += "**"
            exclusions.append(atom)
    return tuple(dict.fromkeys(exclusions))


def path_atoms_overlap(left: str, right: str) -> bool:
    """Conservatively detect exact/glob path intersections."""

    left_glob = any(char in left for char in "*?[")
    right_glob = any(char in right for char in "*?[")
    if not left_glob and not right_glob:
        return left == right
    if left_glob and not right_glob:
        return fnmatch.fnmatchcase(right, left)
    if right_glob and not left_glob:
        return fnmatch.fnmatchcase(left, right)
    if left == right:
        return True
    left_prefix = re.split(r"[*?[{]", left, maxsplit=1)[0]
    right_prefix = re.split(r"[*?[{]", right, maxsplit=1)[0]
    return left_prefix.startswith(right_prefix) or right_prefix.startswith(left_prefix)


def parse_dispatch_dag(
    dag_path: Path,
) -> tuple[
    dict[str, int],
    dict[str, str],
    dict[str, tuple[str, ...]],
    dict[str, tuple[str, ...]],
    dict[str, tuple[str, ...]],
    list[str],
]:
    """Parse the canonical DAG's complete node table without learning aliases from prose."""

    document = dag_path.read_text(encoding="utf-8")
    section = section_between(
        document, "## Canonical node table", "## Deterministic ready sets and proof"
    )
    rows, errors = exact_markdown_table(section, DAG_HEADER, "canonical dispatch DAG")
    waves: dict[str, int] = {}
    phases: dict[str, str] = {}
    predecessors: dict[str, tuple[str, ...]] = {}
    scopes: dict[str, tuple[str, ...]] = {}
    exclusions: dict[str, tuple[str, ...]] = {}
    target_pattern = re.compile(
        r"(?:T\d+-W\d+[A-Za-z]*|D\d+|O(?:1|-[A-Z][A-Z0-9-]*)|R\d+)"
    )
    for line_no, columns in rows:
        node = plain_markdown(columns[0])
        if not re.fullmatch(r"T\d+-W\d+[A-Za-z]*", node):
            errors.append(f"DAG row {line_no} has an opaque node id: {columns[0]!r}")
            continue
        if node in waves:
            errors.append(f"DAG physically repeats node {node}")
            continue
        if node in TOMBSTONED_WPS:
            errors.append(f"DAG registers deleted/tombstoned node {node}")
        phase = plain_markdown(columns[1]).lower()
        wave_match = re.fullmatch(r"w([0-4])\b.+", phase)
        if not wave_match:
            errors.append(f"DAG node {node} has an invalid phase: {columns[1]!r}")
            continue
        waves[node] = int(wave_match.group(1))
        phases[node] = phase

        predecessor_text = plain_markdown(columns[2])
        if predecessor_text == "—":
            deps: tuple[str, ...] = ()
        else:
            dep_parts = tuple(
                part.strip() for part in predecessor_text.split(",") if part.strip()
            )
            invalid = [part for part in dep_parts if not target_pattern.fullmatch(part)]
            if invalid:
                errors.append(
                    f"DAG node {node} has opaque predecessor token(s): {invalid}"
                )
            deps = tuple(part for part in dep_parts if target_pattern.fullmatch(part))
            repeated = sorted(
                item for item, count in Counter(deps).items() if count > 1
            )
            if repeated:
                errors.append(f"DAG node {node} repeats predecessors: {repeated}")
        predecessors[node] = deps

        atoms = scope_atoms(columns[3])
        if not atoms:
            errors.append(f"DAG node {node} declares no exclusive path atom")
        scopes[node] = atoms
        exclusions[node] = scope_exclusions(columns[3])
        if not plain_markdown(columns[4]):
            errors.append(f"DAG node {node} has no lane")

    rendered_batches: list[tuple[str, ...]] = []
    batch_rows = re.findall(r"^B(\d{2}):\s*(.*?)\s*$", document, re.MULTILINE)
    if [number for number, _ in batch_rows] != [
        f"{index:02d}" for index in range(len(batch_rows))
    ]:
        errors.append(
            f"DAG ready-set batch labels must be contiguous from B00, got {[n for n, _ in batch_rows]}"
        )
    for number, body in batch_rows:
        nodes = tuple(body.split())
        if len(nodes) > 8:
            errors.append(f"DAG ready set B{number} exceeds cap 8: {len(nodes)}")
        opaque = [node for node in nodes if not WP_RE.fullmatch(node)]
        if opaque:
            errors.append(f"DAG ready set B{number} has opaque nodes: {opaque}")
        rendered_batches.append(nodes)
    rendered_nodes = [node for batch in rendered_batches for node in batch]
    repeated = sorted(
        node for node, count in Counter(rendered_nodes).items() if count > 1
    )
    if repeated:
        errors.append(f"DAG ready sets repeat nodes: {repeated}")
    if set(rendered_nodes) != set(waves):
        errors.append(
            "DAG ready-set vertex mismatch: "
            f"missing={sorted(set(waves) - set(rendered_nodes))}, "
            f"unexpected={sorted(set(rendered_nodes) - set(waves))}"
        )

    remaining = set(waves)
    computed_batches: list[tuple[str, ...]] = []
    while remaining:
        ready = sorted(
            node
            for node in remaining
            if not (set(predecessors.get(node, ())) & remaining)
        )
        if not ready:
            errors.append(f"DAG contains a cycle among nodes: {sorted(remaining)}")
            break
        batch = tuple(ready[:8])
        computed_batches.append(batch)
        remaining.difference_update(batch)
    if rendered_batches != computed_batches:
        errors.append(
            f"DAG rendered ready sets disagree with deterministic Kahn batches: "
            f"rendered={rendered_batches}, computed={computed_batches}"
        )
    return waves, phases, predecessors, scopes, exclusions, errors


def dag_scope_collisions(
    phases: dict[str, str],
    predecessors: dict[str, tuple[str, ...]],
    scopes: dict[str, tuple[str, ...]],
    exclusions: dict[str, tuple[str, ...]],
) -> list[tuple[str, str, str, str]]:
    """Find path/glob overlaps among nodes explicitly declared in one parallel phase."""

    collisions: list[tuple[str, str, str, str]] = []

    def depends_on(node: str, target: str) -> bool:
        pending = list(predecessors.get(node, ()))
        seen: set[str] = set()
        while pending:
            current = pending.pop()
            if current == target:
                return True
            if current in seen:
                continue
            seen.add(current)
            pending.extend(predecessors.get(current, ()))
        return False

    def excluded(owner: str, atom: str) -> bool:
        return any(path_atoms_overlap(atom, item) for item in exclusions.get(owner, ()))

    nodes = sorted(scopes)
    for index, left_wp in enumerate(nodes):
        left_phase = phases.get(left_wp, "")
        if "serial" in left_phase or "worker" in left_phase or "separate" in left_phase:
            continue
        for right_wp in nodes[index + 1 :]:
            right_phase = phases.get(right_wp, "")
            if left_phase != right_phase:
                continue
            if depends_on(left_wp, right_wp) or depends_on(right_wp, left_wp):
                continue
            if (
                "serial" in right_phase
                or "worker" in right_phase
                or "separate" in right_phase
            ):
                continue
            for left_atom in scopes[left_wp]:
                for right_atom in scopes[right_wp]:
                    if path_atoms_overlap(left_atom, right_atom):
                        if excluded(left_wp, right_atom) or excluded(
                            right_wp, left_atom
                        ):
                            continue
                        collisions.append((left_wp, right_wp, left_atom, right_atom))
    return collisions


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
    triage_path: Path,
    plan_path: Path,
    wp_path: Path,
    finding_path: Path,
    dag_path: Path,
) -> tuple[bool, list[str], dict[str, object]]:
    failures: list[str] = []
    document = triage_path.read_text(encoding="utf-8")
    failures.extend(validate_summary(document))
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
    source_to_items: dict[str, list[str]] = defaultdict(list)
    for row in rows:
        source_to_items[row.source].append(row.item)
    association_mismatches = {
        source: {
            "expected": expected,
            "got": tuple(source_to_items.get(source, [])),
        }
        for source, expected in EXPECTED_SOURCE_TO_AU.items()
        if tuple(source_to_items.get(source, [])) != expected
    }
    if association_mismatches:
        failures.append(
            f"canonical source/AU association mismatch: {association_mismatches}"
        )
    kind_mismatches = {
        row.item: {"expected": EXPECTED_AU_KINDS.get(row.item), "got": row.kind}
        for row in rows
        if EXPECTED_AU_KINDS.get(row.item) != row.kind
    }
    if kind_mismatches:
        failures.append(f"canonical AU kind mismatch: {kind_mismatches}")

    (
        declarations,
        extensions,
        declaration_waves,
        declaration_scopes,
        declaration_errors,
    ) = parse_wp_declarations(document)
    failures.extend(declaration_errors)
    if not dag_path.exists():
        failures.append(f"canonical dispatch DAG is missing: {dag_path}")
        dag_waves: dict[str, int] = {}
        dag_phases: dict[str, str] = {}
        dag_predecessors: dict[str, tuple[str, ...]] = {}
        dag_scopes: dict[str, tuple[str, ...]] = {}
        dag_exclusions: dict[str, tuple[str, ...]] = {}
    else:
        (
            dag_waves,
            dag_phases,
            dag_predecessors,
            dag_scopes,
            dag_exclusions,
            dag_errors,
        ) = parse_dispatch_dag(dag_path)
        failures.extend(dag_errors)
    declaration_waves = {wp: dag_waves[wp] for wp in declarations if wp in dag_waves}
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
        main_items, main_invariants, _main_scopes, main_waves = read_wp_catalog(wp_path)
    except (OSError, SyntaxError, ValueError) as exc:
        failures.append(f"cannot read principal WP catalog: {exc}")
        main_items, main_invariants, _main_scopes, main_waves = {}, {}, {}, {}
    try:
        principal_findings = read_finding_catalog(finding_path)
    except (OSError, SyntaxError, ValueError) as exc:
        failures.append(f"cannot read principal finding catalog: {exc}")
        principal_findings = set()

    main_names = set(main_items)
    proposal_names = set(declarations)
    # After applying the three coordinated renames, proposal names must be
    # disjoint from principal names.  Do not merge proposal items into the
    # principal catalog: they are not suite items until a later delta lands.
    collisions = sorted(proposal_names & main_names)
    if collisions:
        failures.append(f"proposal/principal WP name collisions: {collisions}")

    if proposal_names != EXPECTED_PROPOSAL_WPS:
        failures.append(
            f"proposal WPs differ from the triage contract: expected "
            f"{sorted(EXPECTED_PROPOSAL_WPS)}, got {sorted(proposal_names)}"
        )
    if set(extensions) != EXPECTED_EXTENSION_WPS:
        failures.append(
            f"AU extensions differ from the triage contract: expected "
            f"{sorted(EXPECTED_EXTENSION_WPS)}, got {sorted(extensions)}"
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
        if dag_waves.get(row.owner)
        != EXPECTED_PROPOSAL_WAVES.get(
            row.owner,
            EXPECTED_EXTENSION_WAVES.get(row.owner, main_waves.get(row.owner)),
        )
    )
    if row_wave_mismatch:
        failures.append(f"placement/DAG phase mismatch: {row_wave_mismatch}")

    plan_document = plan_path.read_text(encoding="utf-8")
    known_wps = set(dag_waves)
    unknown_wps = sorted(mentioned_wps - known_wps)
    if unknown_wps:
        failures.append(f"placement rows mention unknown WP ids: {unknown_wps}")
    unknown_extensions = sorted(set(extensions) - known_wps)
    if unknown_extensions:
        failures.append(
            f"AU extensions target unknown principal WPs: {unknown_extensions}"
        )

    scope_disagreements = {
        wp: {
            "triage": scope_atoms(scope),
            "DAG": dag_scopes.get(wp, ()),
        }
        for wp, scope in declaration_scopes.items()
        if set(scope_atoms(scope)) != set(dag_scopes.get(wp, ()))
    }
    if scope_disagreements:
        failures.append(
            f"triage/DAG exact scope-atom disagreement: {scope_disagreements}"
        )
    # Compare individual path/glob atoms, including exact paths nested under
    # another WP's broad glob.  Whole-cell string equality misses that class.
    scope_collisions = dag_scope_collisions(
        dag_phases, dag_predecessors, dag_scopes, dag_exclusions
    )
    if scope_collisions:
        failures.append(f"parallel DAG path/glob scope collisions: {scope_collisions}")

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
    decisions, armings, relays, registry_errors = principal_dependency_registries(
        plan_document
    )
    failures.extend(registry_errors)
    dependencies = [(row.item, row.dependency) for row in rows]
    dependencies.extend(
        (
            wp,
            next(
                (
                    line
                    for line in document.splitlines()
                    if line.startswith(f"| **{wp}**")
                ),
                "",
            ),
        )
        for wp in declarations
    )
    failures.extend(
        validate_dependency_targets(
            dependencies,
            wps=known_wps,
            acceptance=a_ids,
            findings=principal_findings,
            decisions=decisions,
            armings=armings,
            relays=relays,
        )
    )
    failures.extend(
        validate_dependency_targets(
            [
                (node, ", ".join(predecessors))
                for node, predecessors in dag_predecessors.items()
            ],
            wps=known_wps,
            acceptance=set(),
            findings=principal_findings,
            decisions=decisions,
            armings=armings,
            relays=relays,
        )
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
    parser.add_argument(
        "--plan-check",
        type=Path,
        default=Path(__file__).with_name("plan-check.py"),
        help="principal finding gate (used as a literal source-id catalog)",
    )
    parser.add_argument(
        "--dag",
        type=Path,
        default=Path(__file__).with_name("2026-09-01-reconciled-dispatch-dag.md"),
        help="canonical reconciled dispatch DAG",
    )
    args = parser.parse_args(argv)

    try:
        ok, failures, context = check(
            args.triage, args.plan, args.wp_check, args.plan_check, args.dag
        )
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
            f"\nau-check: AU STAGING PASS — {len(EXPECTED_SOURCE_FINDINGS)} source findings / "
            f"{len(EXPECTED_AU)} proposed AU acceptance ids structurally owned exactly once; "
            "not freeze evidence"
        )
        return 0
    print("\nau-check: BLOCKED")
    return 1


if __name__ == "__main__":
    sys.exit(main())
