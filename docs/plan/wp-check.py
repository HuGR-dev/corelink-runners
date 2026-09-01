#!/usr/bin/env python3
"""wp-check: validate the WP layer's structural consistency.

Blocks unless:
  * every live acceptance item is owned by exactly one WP (or is `judged` -> owner)
  * no WP owns zero items (every WP has structural ownership)
  * no WP owns more than 4 items (the sweet-spot ceiling)
  * every WP declares at least one invariant from the INV catalogue
  * no two PARALLEL WPs share an exclusive file scope
  * the Markdown acceptance/ownership/scope tables match this catalogue exactly
  * the optional schema-v1 dispatch DAG is acyclic and renders exact cap-8 ready sets

This is a structural gate only. It does not assert that a WP is implemented,
falsifiable, green, or ready for production.
Usage: python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
"""

import fnmatch
import re
import sys
from collections import Counter
from pathlib import Path

INV = {f"INV-{i}" for i in range(1, 9)}

# WP -> (items owned, invariants live, exclusive scope, wave)
WP = {
    # wave 0
    "T0-W1": (["A0.1"], ["INV-5", "INV-6"], "docs/plan/union-ledger", 0),
    "T1-W1": (["A1.4"], ["INV-5"], "scripts/ops", 0),
    "T2-W1a": (["A2.1"], ["INV-2"], "build-cf-container-images.yml", 0),
    "T2-W2a": (["A2.3"], ["INV-2", "INV-5"], "scripts/ci/image-pin", 0),
    # wave 1 (parallel unless noted)
    "T3-W4": (
        ["A3.6", "A4.4", "A4.5"],
        ["INV-1", "INV-3"],
        "`crates/corelink-fabric-server/**`",
        1,
    ),
    "T4-W4": (
        ["A4.11", "A4.13"],
        ["INV-3", "INV-4"],
        "`crates/corelink-fabric-server/**`",
        1.5,
    ),  # serial after T3-W4
    "T6-W1": (
        ["A6.1", "A6.2", "A6.15"],
        ["INV-7"],
        "`scripts/*.selftest.sh`, `scripts/pre-merge-gate-check.sh`, `.github/workflows/ci.yml`, new `selftests.yml`",
        1,
    ),
    "T6-W2": (
        ["A6.3"],
        ["INV-7"],
        "`moat-benchmark.yml`, `moat-action-test.yml`, `actions/corelink-memoize/action.yml`",
        1,
    ),
    "T6-W3": (
        ["A6.4"],
        ["INV-1"],
        "new `conformance.yml`, `spawn-worker-ci.yml` (path filter only), `sdk/**` test/CI files",
        1,
    ),
    "T6-W8": (
        ["A6.8"],
        ["INV-7"],
        "new `pg-suite.yml` + `crates/corelink-fabric/**` test cfg",
        1,
    ),
    "T6-W4": (
        ["A6.5", "A6.9", "A6.10"],
        ["INV-8"],
        "new `secret-scan.yml`, `corelink-stress.yml`, `deploy/cloudflare-canary/**` (not its README)",
        1,
    ),
    "T5-W1": (
        ["A5.3"],
        ["INV-5"],
        "new `docs/onboarding/`, `actions/corelink-memoize/README.md`",
        1,
    ),
    "T5-W2": (
        ["A5.2", "A5.5"],
        ["INV-2"],
        "`integrations/**`",
        1,
    ),
    "T7-W1": (["A7.2"], ["INV-5"], "`docs/ROADMAP.md`, `CHANGELOG.md`", 1),
    "T7-W2": (
        ["A7.1"],
        ["INV-5"],
        "`docs/**` minus `plan/`,`handoff/`,`review/`,`audits/`,`onboarding/`,`runbook/`,`ROADMAP.md`; `deploy/**/README.md` minus canary",
        1,
    ),
    "T7-W3": (
        ["A7.4", "A7.5"],
        ["INV-5"],
        "new `scripts/ci/claim-artifact-lint.sh` + `docs/plan/evidence/` schema",
        1,
    ),
    "T9-W0": (
        ["A0.2"],
        ["INV-7"],
        "`deploy/cloudflare/vitest.config.ts`, `deploy/cloudflare/test/devenv-do.test.ts`",
        1,
    ),
    "T2-W3": (["A2.6"], ["INV-2"], "**every** `.github/workflows/*.yml`", 1.9),
    # wave 2 (strictly serial on the worker monolith -> shared scope is EXPECTED)
    "T4-W1": (["A4.1"], ["INV-4"], "`index.ts`", 2),
    "T4-W2": (["A4.2", "A4.3", "A4.6"], ["INV-1", "INV-4"], "`index.ts` + `lib.ts`", 2),
    "T3-W3": (["A3.5", "A3.11"], ["INV-3"], "`lib.ts` reconciler + `index.ts`", 2),
    "T3-W1": (
        ["A3.1", "A3.2", "A3.7"],
        ["INV-1", "INV-8"],
        "`crates/corelink-cloud-engine/**` **+** `index.ts` — one coupled wire change; rev-3 split it across waves and closed the Rust side first",
        2,
    ),
    "T3-W2": (
        ["A3.3", "A3.4", "A3.10", "A3.12"],
        ["INV-3"],
        "`index.ts` + `metrics.ts` + `lib.ts`",
        2,
    ),
    "T8-W1": (
        ["A3.14", "A3.15", "A3.16"],
        ["INV-3", "INV-8"],
        "the RH-class: silent cold-degrade alarm · spawn-token scoping · admission fail-open",
        2,
    ),
    "T8-W3": (
        ["A3.17", "A3.18"],
        ["INV-3", "INV-4"],
        "worker mint-path fail-closed test/probe · atomic spawn claim; **no fabric boot/readiness scope**",
        2,
    ),
    "T8-W2": (
        ["A3.13"],
        ["INV-3", "INV-8"],
        "cross-tenant CAS isolation + per-job credential scope/TTL",
        2,
    ),
    "T9-W1": (["A3.8", "A4.8"], ["INV-7"], "devenv quarantine — **D2**", 2),
    # wave 3 (live proof)
    "T1-W2": (["A1.1", "A1.2", "A1.3", "A1.5"], ["INV-5"], "probe:control-plane", 3),
    "T1-W3": (["A1.6", "A1.7"], ["INV-5"], "probe:resilience", 3),
    "T1-W4": (["A1.8", "A1.9"], ["INV-5"], "probe:boot-rate+uptime", 3),
    "T2-W5": (
        ["A2.11", "A2.12", "A2.13"],
        ["INV-5", "INV-7"],
        "runbook override consumption + compatibility matrix",
        3,
    ),
    "T4-W8": (["A4.14", "A4.15"], ["INV-4"], "probe:billing-reconcile", 3),
    "T3-W8": (["A3.19", "A3.20"], ["INV-5"], "probe:inventory+hitrate", 3),
    "T5-W5": (["A5.10"], ["INV-5"], "probe:stranger-adversarial", 3),
    "T5-W6": (["A5.4"], ["INV-2", "INV-5"], "probe:release-artifacts", 3),
    "T6-W10": (["A6.16", "A6.17", "A6.18"], ["INV-3", "INV-5"], "alerting-depth", 3),
    "T6-W11": (["A6.19"], ["INV-5"], "runbook-execution", 3),
    "T7-W4b": (["A7.6"], ["INV-5"], "probe-artifact freshness schema/check", 1),
    "T2-W2b": (
        ["A2.4", "A2.5", "A2.7", "A2.10"],
        ["INV-2", "INV-5"],
        "probe:deploy",
        3,
    ),
    "T2-W4": (["A2.8", "A2.9"], ["INV-2"], "probe:image-ship", 3),
    "T3-W7": (["A3.9"], ["INV-5"], "probe:moat", 3),
    "T4-W7": (["A4.7", "A4.10", "A4.12"], ["INV-1", "INV-4"], "probe:money", 3),
    "T5-W4": (["A5.6", "A5.8", "A5.9"], ["INV-5"], "probe:stranger", 3),
    "T6-W5": (["A6.7"], ["INV-5"], "probe:e2e", 3),
    "T6-W6": (["A6.6", "A6.13", "A6.14"], ["INV-5"], "probe:canary", 3),
    "T6-W7": (["A6.11"], ["INV-3"], "probe:authz", 3),
    "T6-W9": (["A6.12"], ["INV-5"], "probe:alert-rules", 3),
}

JUDGED_TO_OWNER = {"A4.9", "A5.1", "A7.3"}
WITHDRAWN = {"A2.2", "A5.7"}
ITEM_KINDS = {"test", "probe", "test+probe", "judged", "—"}

# This is the frozen principal suite, not a set of labels learned from the
# document being checked.  In particular, changing a row from test to probe
# materially changes its evidence contract and must never remain a PASS.
FROZEN_ITEM_KINDS = {
    "A0.1": "test",
    "A0.2": "test",
    "A1.1": "probe",
    "A1.2": "probe",
    "A1.3": "probe",
    "A1.4": "test",
    "A1.5": "probe",
    "A1.6": "probe",
    "A1.7": "probe",
    "A1.8": "probe",
    "A1.9": "probe",
    "A2.1": "test",
    "A2.2": "—",
    "A2.3": "test",
    "A2.4": "probe",
    "A2.5": "probe",
    "A2.6": "test",
    "A2.7": "probe",
    "A2.8": "probe",
    "A2.9": "probe",
    "A2.10": "probe",
    "A2.11": "test",
    "A2.12": "probe",
    "A2.13": "probe",
    "A3.1": "test",
    "A3.2": "test",
    "A3.3": "test",
    "A3.4": "test",
    "A3.5": "test",
    "A3.6": "test",
    "A3.7": "test",
    "A3.8": "test",
    "A3.9": "probe",
    "A3.10": "probe",
    "A3.11": "test",
    "A3.12": "test",
    "A3.13": "test",
    "A3.14": "test",
    "A3.15": "test",
    "A3.16": "test",
    "A3.17": "test+probe",
    "A3.18": "test",
    "A3.19": "probe",
    "A3.20": "probe",
    "A4.1": "test",
    "A4.2": "test",
    "A4.3": "test",
    "A4.4": "test",
    "A4.5": "test",
    "A4.6": "test",
    "A4.7": "probe",
    "A4.8": "test",
    "A4.9": "judged",
    "A4.10": "probe",
    "A4.11": "test",
    "A4.12": "test",
    "A4.13": "test",
    "A4.14": "test",
    "A4.15": "test",
    "A5.1": "judged",
    "A5.2": "test",
    "A5.3": "test",
    "A5.4": "probe",
    "A5.5": "test",
    "A5.6": "probe",
    "A5.7": "—",
    "A5.8": "probe",
    "A5.9": "probe",
    "A5.10": "probe",
    "A6.1": "test",
    "A6.2": "test",
    "A6.3": "test",
    "A6.4": "test",
    "A6.5": "test",
    "A6.6": "probe",
    "A6.7": "probe",
    "A6.8": "test",
    "A6.9": "probe",
    "A6.10": "test",
    "A6.11": "probe",
    "A6.12": "test+probe",
    "A6.13": "probe",
    "A6.14": "probe",
    "A6.15": "test",
    "A6.16": "test",
    "A6.17": "probe",
    "A6.18": "test",
    "A6.19": "test",
    "A7.1": "test",
    "A7.2": "test",
    "A7.3": "judged",
    "A7.4": "test",
    "A7.5": "test",
    "A7.6": "test",
}

SUITE_HEADING = "## 3. The acceptance suite (the completeness anchor)"
OWNER_HEADING = "## 4. Owner decisions"
WAVE_HEADINGS = {
    0: "### Wave 0 — unblock (11 findings)",
    1: "### Wave 1 — parallel, partitioned by **named file** (32 findings)",
    2: "### Wave 2 — SERIAL on `index.ts` / `lib.ts` (21 findings)",
    3: "### Wave 3 — live proof (20 findings)",
    4: "### Wave 4 — post-decision (43 findings)",
}
WAVES_HEADING = "## 5. Waves"
ARMING_HEADING = "## 6. Owner arming (config only)"

REV5_HEADING = "### Items added at rev-5 (cold review, round 2)"
CAPABILITY_HEADINGS = {
    1: "### C1 — control plane",
    2: "### C2 — shippability",
    3: "### C3 — job lifecycle",
    4: "### C4 — money",
    5: "### C5 — the stranger",
    6: "### C6 — we find out",
    7: "### C7 — truth",
}
REV4_HEADING = "### Items added at rev-4 (WPs that had none)"

# Physical table partitions are frozen too.  A row cannot be moved beneath a
# different capability (or hidden in an additional table) while preserving the
# same global id set.
ACCEPTANCE_TABLES = (
    (
        REV5_HEADING,
        CAPABILITY_HEADINGS[1],
        ("id", "kind", "item", "gap"),
        (
            "A1.8",
            "A1.9",
            "A2.11",
            "A2.12",
            "A2.13",
            "A4.14",
            "A4.15",
            "A3.19",
            "A3.20",
            "A5.10",
            "A6.16",
            "A6.17",
            "A6.18",
            "A7.6",
            "A6.19",
        ),
    ),
    (
        CAPABILITY_HEADINGS[1],
        CAPABILITY_HEADINGS[2],
        ("id", "kind", "item"),
        tuple(f"A1.{i}" for i in range(1, 8)),
    ),
    (
        CAPABILITY_HEADINGS[2],
        CAPABILITY_HEADINGS[3],
        ("id", "kind", "item"),
        tuple(f"A2.{i}" for i in range(1, 11)),
    ),
    (
        CAPABILITY_HEADINGS[3],
        CAPABILITY_HEADINGS[4],
        ("id", "kind", "item"),
        tuple(f"A3.{i}" for i in range(1, 19)),
    ),
    (
        CAPABILITY_HEADINGS[4],
        CAPABILITY_HEADINGS[5],
        ("id", "kind", "item"),
        tuple(f"A4.{i}" for i in range(1, 14)),
    ),
    (
        CAPABILITY_HEADINGS[5],
        CAPABILITY_HEADINGS[6],
        ("id", "kind", "item"),
        tuple(f"A5.{i}" for i in range(1, 10)),
    ),
    (
        CAPABILITY_HEADINGS[6],
        CAPABILITY_HEADINGS[7],
        ("id", "kind", "item"),
        tuple(f"A6.{i}" for i in range(1, 15)),
    ),
    (
        CAPABILITY_HEADINGS[7],
        REV4_HEADING,
        ("id", "kind", "item"),
        tuple(f"A7.{i}" for i in range(1, 6)),
    ),
    (
        REV4_HEADING,
        OWNER_HEADING,
        ("id", "kind", "item", "owner"),
        ("A0.1", "A0.2", "A6.15"),
    ),
)

WAVE2_CHAIN = (
    "T4-W1",
    "T4-W2",
    "T3-W3",
    "T3-W1",
    "T3-W2",
    "T8-W1",
    "T8-W3",
    "T8-W2",
    "T9-W1",
)

DAG_FILENAME = "2026-09-01-reconciled-dispatch-dag.md"
DAG_SCHEMA_MARKER = "**Date:** 2026-09-01 · **Schema:** `dispatch-dag/v1` · **Status: NOT DISPATCHABLE**"
DAG_TABLE_HEADING = "## Canonical node table"
DAG_BATCH_HEADING = "## Deterministic ready sets and proof"
DAG_HEADER = [
    "node",
    "phase / wave",
    "exact hard predecessors",
    "exclusive path atoms; artifact filename",
    "lane",
]
DAG_EXTERNAL_NODES = {
    *(f"D{i}" for i in range(1, 14)),
    *(f"R{i}" for i in range(1, 7)),
    "O1",
    "O-DEVENV-PIN",
    "O-BILLING",
    "O-ALLOWLIST",
    "O-PIN",
    "O-APP",
    "O-CANARY",
    "O-FLEETBUSY",
    "O-MINTKEY",
    "O-CHECKHOST",
    "O-CFTOKEN",
    "O-ROTATE",
    "O-PUBLISH",
    "O-CFINVENTORY",
    "O-CFRATE",
}
DAG_PHASES = {
    "W0 unblock",
    "W0 containment (post-freeze)",
    "W1 parallel",
    "W1 serial",
    "W1 closer",
    "W2 worker",
    "W2 separate lane",
    "W3 live proof",
    "W4 post-decision",
    "W1 serial test+probe",
    "W2 worker test+probe",
}

STAGED_FILENAME = "2026-09-01-round3-remediation-delta.md"
STAGED_PACKET_HEADING = "## 4. Proposed WP packet contracts (not a second dispatch DAG)"
STAGED_PACKET_STOP = "## 5. One eligible baseline and required review sequence"
STAGED_PACKET_HEADER = [
    "wp",
    "owns",
    "count",
    "exclusive x",
    "exact acceptance prerequisite",
    "pre-decided implementation contract",
]
STAGED_PRINCIPAL_WPS = {
    "T1-W5",
    "T1-W6",
    "T3-W15",
    "T3-W16",
    "T3-W17",
    "T3-W18",
    "T6-W12",
    "T6-W13",
    "T6-W14",
}

AU_FILENAME = "union-triage-remaining.md"
AU_NEW_WP_MARKER = "**New WPs and their item counts** (all ≤ the four-item ceiling):"
AU_EXTENSION_MARKER = "**Extensions to existing WPs:**"
AU_NEW_WP_HEADER = ["wp", "owns", "exact exclusive write scope (the x)"]
AU_DECLARED_NEW_WPS = {
    "T2-W6",
    "T3-W9",
    "T3-W10",
    "T3-W14",
    "T3-W5",
    "T5-W3",
    "T7-W4",
    "T7-W5",
    "T8-W4",
    "T8-W5",
    "T8-W6",
    "T8-W7",
}
AU_EXTENSION_WPS = {"T4-W1", "T5-W1", "T6-W10", "T8-W1"}
AU_DAG_WPS = AU_DECLARED_NEW_WPS

STAGED_ACCEPTANCE_HEADING = "## 3. Acceptance proposals — reserved, not promoted"
STAGED_ACCEPTANCE_STOP = "### 3.1 Binding A3.29 liveness/safety matrix"
STAGED_ACCEPTANCE_HEADER = [
    "id",
    "kind",
    "wp (≤4)",
    "x",
    "deps",
    "invariants",
    "fixed threshold",
    "red → green test",
]
AU_PLACEMENT_HEADING = "## 2. Per-finding placement"
AU_PLACEMENT_STOP = "## 3. Staged ownership consequences"
AU_PLACEMENT_HEADER = [
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

WP_ID_RE = re.compile(r"T\d+-W\d+[A-Za-z]*")
EVIDENCE_ARTIFACT_RE = re.compile(r"docs/plan/evidence/[A-Za-z0-9._-]+\.json")


def exact_line_positions(text, heading):
    return [m.start() for m in re.finditer(rf"^{re.escape(heading)}$", text, re.M)]


def prefix_line_positions(text, prefix):
    return [m.start() for m in re.finditer(rf"^{re.escape(prefix)}", text, re.M)]


def markdown_cells(line):
    """Split a table row without treating a pipe in inline code as a cell."""
    content = line.strip()
    if content.startswith("|"):
        content = content[1:]
    if content.endswith("|"):
        content = content[:-1]

    cells = []
    current = []
    in_code = False
    escaped = False
    for character in content:
        if escaped:
            current.append(character)
            escaped = False
        elif character == "\\":
            current.append(character)
            escaped = True
        elif character == "`":
            current.append(character)
            in_code = not in_code
        elif character == "|" and not in_code:
            cells.append("".join(current).strip())
            current = []
        else:
            current.append(character)
    cells.append("".join(current).strip())
    return cells


def plain_markdown(value):
    value = value.replace("**", "").replace("`", "")
    return re.sub(r"\s+", " ", value).strip()


def rendered_markdown(text, label):
    """Remove HTML comments while retaining offsets and line boundaries.

    Markdown inside ``<!-- ... -->`` is not rendered and therefore cannot be
    accepted as a visible registry.  Replacing non-newline characters with
    spaces keeps line-oriented diagnostics stable and leaves historical
    comments otherwise unconstrained.
    """
    rendered = list(text)
    position = 0
    errors = []
    while True:
        start = text.find("<!--", position)
        if start < 0:
            break
        end = text.find("-->", start + 4)
        if end < 0:
            errors.append(f"{label} contains an unterminated Markdown HTML comment")
            end = len(text) - 3
        for index in range(start, min(end + 3, len(rendered))):
            if rendered[index] not in "\r\n":
                rendered[index] = " "
        if end + 3 >= len(text):
            break
        position = end + 3
    return "".join(rendered), errors


def acceptance_cell_id(value):
    """Return an A id despite Markdown emphasis, but reject trailing prose."""
    normalized = value.replace("★", "")
    for marker in ("**", "~~", "*", "`"):
        normalized = normalized.replace(marker, "")
    match = re.fullmatch(r"\s*(A\d+\.\d+)\s*", normalized)
    return match.group(1) if match else None


def first_table_after(text, heading, stop_heading):
    """Return (header, rows), or an error string for the heading's first table."""
    starts = exact_line_positions(text, heading)
    stops = exact_line_positions(text, stop_heading)
    if len(starts) != 1 or len(stops) != 1 or starts[0] >= stops[0]:
        return None, None, f"cannot isolate table under exact heading {heading!r}"

    lines = text[starts[0] : stops[0]].splitlines()[1:]
    table = []
    started = False
    for line in lines:
        if line.startswith("|"):
            table.append(line)
            started = True
        elif started:
            break
    if len(table) < 3:
        return None, None, f"missing Markdown table under {heading!r}"

    header = [plain_markdown(c).lower() for c in markdown_cells(table[0])]
    separator = markdown_cells(table[1])
    if len(separator) != len(header) or any(
        not re.fullmatch(r":?-{3,}:?", c) for c in separator
    ):
        return None, None, f"malformed Markdown table separator under {heading!r}"

    rows = [markdown_cells(line) for line in table[2:]]
    if any(len(row) != len(header) for row in rows):
        return None, None, f"wrong cell count in Markdown table under {heading!r}"
    return header, rows, None


def first_table_between_offsets(text, start, stop, label):
    """Return the first complete table in a pre-isolated visible range."""
    lines = text[start:stop].splitlines()[1:]
    table = []
    started = False
    for line in lines:
        if line.startswith("|"):
            table.append(line)
            started = True
        elif started:
            break
    if len(table) < 3:
        return None, None, f"missing Markdown table in {label}"
    header = [plain_markdown(cell).lower() for cell in markdown_cells(table[0])]
    separator = markdown_cells(table[1])
    if len(separator) != len(header) or any(
        not re.fullmatch(r":?-{3,}:?", cell) for cell in separator
    ):
        return None, None, f"malformed Markdown table separator in {label}"
    rows = [markdown_cells(line) for line in table[2:]]
    if any(len(row) != len(header) for row in rows):
        return None, None, f"wrong cell count in Markdown table in {label}"
    return header, rows, None


def extract_wp_registry(table_rows, *, label, required_prefix=None):
    """Read one WP id per registry row without learning ids from prose."""
    registry = set()
    errors = []
    for row in table_rows:
        label_cell = plain_markdown(row[0])
        matches = WP_ID_RE.findall(label_cell)
        if len(matches) != 1:
            errors.append(f"{label} has an opaque WP label {row[0]!r}")
            continue
        if required_prefix and not label_cell.startswith(required_prefix):
            continue
        node = matches[0]
        if node in registry:
            errors.append(f"{label} physically repeats WP {node}")
        registry.add(node)
    return registry, errors


def load_supplemental_registries(directory):
    """Return the frozen staged-principal and AU-only DAG vertex registries."""
    errors = []

    staged_path = directory / STAGED_FILENAME
    try:
        staged_text, visibility_errors = rendered_markdown(
            staged_path.read_text(encoding="utf-8"), staged_path.name
        )
    except OSError as exc:
        return set(), set(), [f"cannot read staged WP registry: {exc}"]
    errors.extend(visibility_errors)
    header, rows, error = first_table_after(
        staged_text, STAGED_PACKET_HEADING, STAGED_PACKET_STOP
    )
    if error:
        errors.append(f"staged WP registry: {error}")
        staged = set()
    elif header != STAGED_PACKET_HEADER:
        errors.append(
            f"staged WP registry header mismatch: expected {STAGED_PACKET_HEADER}, "
            f"got {header}"
        )
        staged = set()
    else:
        staged, row_errors = extract_wp_registry(
            rows, label="staged WP registry", required_prefix="new "
        )
        errors.extend(row_errors)
    if staged != STAGED_PRINCIPAL_WPS:
        errors.append(
            "staged principal WP registry mismatch: "
            f"missing {sorted(STAGED_PRINCIPAL_WPS - staged)}, "
            f"unexpected {sorted(staged - STAGED_PRINCIPAL_WPS)}"
        )

    au_path = directory / AU_FILENAME
    try:
        au_text, visibility_errors = rendered_markdown(
            au_path.read_text(encoding="utf-8"), au_path.name
        )
    except OSError as exc:
        return staged, set(), errors + [f"cannot read AU WP registry: {exc}"]
    errors.extend(visibility_errors)
    au_starts = exact_line_positions(au_text, AU_NEW_WP_MARKER)
    extension_starts = prefix_line_positions(au_text, AU_EXTENSION_MARKER)
    if len(au_starts) != 1 or len(extension_starts) != 1:
        header, rows, error = (
            None,
            None,
            (
                "cannot isolate AU new-WP table: "
                f"new marker={len(au_starts)}, extension marker={len(extension_starts)}"
            ),
        )
    else:
        header, rows, error = first_table_between_offsets(
            au_text, au_starts[0], extension_starts[0], "AU new-WP registry"
        )
    if error:
        errors.append(f"AU WP registry: {error}")
        au_declared = set()
    elif header != AU_NEW_WP_HEADER:
        errors.append(
            f"AU WP registry header mismatch: expected {AU_NEW_WP_HEADER}, got {header}"
        )
        au_declared = set()
    else:
        au_declared, row_errors = extract_wp_registry(rows, label="AU new-WP registry")
        errors.extend(row_errors)
    if au_declared != AU_DECLARED_NEW_WPS:
        errors.append(
            "AU declared-new WP registry mismatch: "
            f"missing {sorted(AU_DECLARED_NEW_WPS - au_declared)}, "
            f"unexpected {sorted(au_declared - AU_DECLARED_NEW_WPS)}"
        )

    if len(extension_starts) != 1:
        extensions = set()
        errors.append(
            "AU extension registry marker count mismatch: "
            f"expected 1, got {len(extension_starts)}"
        )
    else:
        extension_tail = au_text[extension_starts[0] :]
        paragraph = extension_tail.split("\n\n", 1)[0]
        extension_ids = WP_ID_RE.findall(paragraph)
        duplicate_extensions = sorted(
            node for node, count in Counter(extension_ids).items() if count > 1
        )
        if duplicate_extensions:
            errors.append(
                f"AU extension registry physically repeats WPs: {duplicate_extensions}"
            )
        extensions = set(extension_ids)
    if extensions != AU_EXTENSION_WPS:
        errors.append(
            "AU extension WP registry mismatch: "
            f"missing {sorted(AU_EXTENSION_WPS - extensions)}, "
            f"unexpected {sorted(extensions - AU_EXTENSION_WPS)}"
        )

    au_vertices = au_declared | (extensions - set(WP))
    if au_vertices != AU_DAG_WPS:
        errors.append(
            "AU DAG vertex registry mismatch: "
            f"missing {sorted(AU_DAG_WPS - au_vertices)}, "
            f"unexpected {sorted(au_vertices - AU_DAG_WPS)}"
        )
    return staged, au_vertices, errors


def normalize_path_atom(value):
    """Normalize a path/glob without stripping leading dots or wildcards."""
    atom = re.sub(r"\s+", " ", value.replace("`", "")).strip(" ;:")
    atom = re.sub(r"^(?:new|every)\s+", "", atom, flags=re.I)
    atom = re.sub(r"\s+\([^)]*\)$", "", atom)
    atom = atom.removeprefix("./")
    if atom.endswith("/"):
        atom += "**"
    return atom


def path_like(value):
    return bool(
        value
        and value != "—"
        and (
            "/" in value
            or any(character in value for character in "*?[")
            or re.search(r"(?:^|/)[.A-Za-z0-9_-]+\.[A-Za-z0-9*?{}_-]+$", value)
        )
    )


def resolve_exclusion(base, exclusion):
    exclusion = normalize_path_atom(exclusion)
    if not exclusion:
        return ""
    root = re.split(r"[*?[{]", base, maxsplit=1)[0]
    if exclusion.startswith(root) or exclusion.startswith("."):
        return exclusion
    if base.startswith("deploy/**/") and exclusion == "canary":
        return "deploy/cloudflare-canary/**"
    # ``deploy/**/README.md excluding canary`` means a canary-bearing path
    # segment, whereas ``docs/** excluding plan/`` is rooted below ``docs/``.
    if "/**/" in base and "/" not in exclusion:
        prefix, suffix = base.split("/**/", 1)
        return f"{prefix}/**/*{exclusion}*/{suffix}"
    return root + exclusion.lstrip("/")


def parse_scope_declaration(scope):
    """Return real path/glob atoms and their explicit carve-outs."""
    atoms = []
    exclusions = []
    for segment in scope.split(";"):
        code_fragments = re.findall(r"`([^`]+)`", segment)
        fragments = code_fragments or [segment]
        carveout = re.search(r"\s+(?:excluding|minus)\s+", segment, re.I)
        if carveout and code_fragments:
            base = normalize_path_atom(fragments[0])
            if path_like(base):
                atoms.append(base)
                raw_exclusions = fragments[1:]
                if not raw_exclusions:
                    raw_exclusions = re.split(
                        r"\s*,\s*", plain_markdown(segment[carveout.end() :])
                    )
                exclusions.extend(
                    item
                    for item in (resolve_exclusion(base, raw) for raw in raw_exclusions)
                    if item
                )
            continue
        for fragment in fragments:
            split = re.split(
                r"\s+(?:excluding|minus)\s+", fragment, maxsplit=1, flags=re.I
            )
            base_text = split[0]
            for raw in re.split(r"\s*(?:,|\+)\s*", base_text):
                atom = normalize_path_atom(raw)
                if path_like(atom) and not atom.startswith("probe:"):
                    atoms.append(atom)
            if len(split) == 2:
                base = normalize_path_atom(base_text)
                exclusions.extend(
                    item
                    for item in (
                        resolve_exclusion(base, raw)
                        for raw in re.split(r"\s*,\s*", split[1])
                    )
                    if item
                )
    return tuple(dict.fromkeys(atoms)), tuple(dict.fromkeys(exclusions))


def path_atoms_overlap(left, right):
    """Conservatively detect an intersection between exact paths and globs."""

    def segment_overlap(left_segment, right_segment):
        left_glob = any(character in left_segment for character in "*?[")
        right_glob = any(character in right_segment for character in "*?[")
        if not left_glob and not right_glob:
            return left_segment == right_segment
        if not left_glob:
            return fnmatch.fnmatchcase(left_segment, right_segment)
        if not right_glob:
            return fnmatch.fnmatchcase(right_segment, left_segment)
        if left_segment == right_segment:
            return True
        left_prefix = re.split(r"[*?\[]", left_segment, maxsplit=1)[0]
        right_prefix = re.split(r"[*?\[]", right_segment, maxsplit=1)[0]
        left_suffix = re.split(r"[*?\[]", left_segment[::-1], maxsplit=1)[0][::-1]
        right_suffix = re.split(r"[*?\[]", right_segment[::-1], maxsplit=1)[0][::-1]
        return (
            left_prefix.startswith(right_prefix) or right_prefix.startswith(left_prefix)
        ) and (left_suffix.endswith(right_suffix) or right_suffix.endswith(left_suffix))

    left_parts = tuple(left.split("/"))
    right_parts = tuple(right.split("/"))
    memo = {}

    def intersects(left_index, right_index):
        key = (left_index, right_index)
        if key in memo:
            return memo[key]
        if left_index == len(left_parts) and right_index == len(right_parts):
            result = True
        elif left_index == len(left_parts):
            result = all(part == "**" for part in right_parts[right_index:])
        elif right_index == len(right_parts):
            result = all(part == "**" for part in left_parts[left_index:])
        elif left_parts[left_index] == "**":
            result = intersects(left_index + 1, right_index) or intersects(
                left_index, right_index + 1
            )
        elif right_parts[right_index] == "**":
            result = intersects(left_index, right_index + 1) or intersects(
                left_index + 1, right_index
            )
        else:
            result = segment_overlap(
                left_parts[left_index], right_parts[right_index]
            ) and intersects(left_index + 1, right_index + 1)
        memo[key] = result
        return result

    return intersects(0, 0)


def path_atom_covers(cover, candidate):
    """Return whether a carve-out covers the complete candidate atom."""
    cover_glob = any(character in cover for character in "*?[")
    candidate_glob = any(character in candidate for character in "*?[")
    if not candidate_glob:
        return (
            path_atoms_overlap(cover, candidate) if cover_glob else candidate == cover
        )
    if not cover_glob:
        return False
    if cover == candidate:
        return True
    if cover.endswith("/**"):
        return re.split(r"[*?[{]", candidate, maxsplit=1)[0].startswith(cover[:-2])
    return False


def scope_overlap(left_atoms, left_exclusions, right_atoms, right_exclusions):
    overlaps = []
    for left in left_atoms:
        for right in right_atoms:
            if not path_atoms_overlap(left, right):
                continue
            if any(path_atom_covers(item, right) for item in left_exclusions):
                continue
            if any(path_atom_covers(item, left) for item in right_exclusions):
                continue
            overlaps.append((left, right))
    return overlaps


def doc_wave(wave):
    if wave == 0:
        return 0
    if 0 < wave < 2:
        return 1
    return int(wave)


def parse_dag_scope_cell(value):
    """Split a DAG registry cell into path atoms, carve-outs, and artifacts."""
    artifacts = []
    scope_parts = []
    opaque_artifacts = []
    for segment in value.split(";"):
        fragments = re.findall(r"`([^`]+)`", segment)
        if not fragments:
            fragment = plain_markdown(segment)
            if fragment and fragment != "—":
                scope_parts.append(segment)
            continue
        remaining = segment
        for fragment in fragments:
            normalized = normalize_path_atom(fragment)
            if normalized.startswith("docs/plan/evidence/"):
                if EVIDENCE_ARTIFACT_RE.fullmatch(normalized):
                    artifacts.append(normalized)
                else:
                    opaque_artifacts.append(normalized)
                remaining = remaining.replace(f"`{fragment}`", "", 1)
        if plain_markdown(remaining) not in {"", "—"}:
            scope_parts.append(remaining)
    atoms, exclusions = parse_scope_declaration(";".join(scope_parts))
    return atoms, exclusions, tuple(artifacts), tuple(opaque_artifacts)


def load_probe_node_registry(directory):
    """Derive probe/test+probe nodes from the three acceptance registries."""
    node_kinds = {node: set() for node in WP}
    errors = []
    for node, (owned_items, _, _, _) in WP.items():
        node_kinds[node].update(
            FROZEN_ITEM_KINDS[item] for item in owned_items if item in FROZEN_ITEM_KINDS
        )

    staged_path = directory / STAGED_FILENAME
    try:
        staged_text, visibility_errors = rendered_markdown(
            staged_path.read_text(encoding="utf-8"), staged_path.name
        )
    except OSError as exc:
        return set(), [f"cannot read staged acceptance registry: {exc}"]
    errors.extend(visibility_errors)
    header, proposal_rows, error = first_table_after(
        staged_text, STAGED_ACCEPTANCE_HEADING, STAGED_ACCEPTANCE_STOP
    )
    staged_item_kinds = {}
    if error:
        errors.append(f"staged acceptance registry: {error}")
    elif header != STAGED_ACCEPTANCE_HEADER:
        errors.append(
            "staged acceptance registry header mismatch: "
            f"expected {STAGED_ACCEPTANCE_HEADER}, got {header}"
        )
    else:
        for row in proposal_rows:
            ids = re.findall(r"\bA\d+\.\d+\b", plain_markdown(row[0]))
            kind = plain_markdown(row[1])
            if len(ids) != 1 or kind not in ITEM_KINDS - {"judged", "—"}:
                errors.append(
                    f"staged acceptance registry has opaque id/kind: {row[0]!r}, {row[1]!r}"
                )
                continue
            staged_item_kinds[ids[0]] = kind

    packet_header, packet_rows, error = first_table_after(
        staged_text, STAGED_PACKET_HEADING, STAGED_PACKET_STOP
    )
    if error:
        errors.append(f"staged packet kind registry: {error}")
    elif packet_header != STAGED_PACKET_HEADER:
        errors.append(
            f"staged packet kind header mismatch: expected {STAGED_PACKET_HEADER}, "
            f"got {packet_header}"
        )
    else:
        for row in packet_rows:
            node_ids = WP_ID_RE.findall(plain_markdown(row[0]))
            item_ids = re.findall(r"\bA\d+\.\d+\b", plain_markdown(row[1]))
            if len(node_ids) != 1 or not item_ids:
                errors.append(
                    f"staged packet has opaque WP/item ownership: {row[0]!r}, {row[1]!r}"
                )
                continue
            owner = node_ids[0]
            owns = plain_markdown(row[1]).lower()
            for item_id in item_ids:
                if "repo half" in owns:
                    kind = "test"
                elif "live half" in owns:
                    kind = "probe"
                else:
                    kind = staged_item_kinds.get(
                        item_id, FROZEN_ITEM_KINDS.get(item_id)
                    )
                if kind is None:
                    errors.append(
                        f"staged packet {owner} references unknown item kind for {item_id}"
                    )
                    continue
                node_kinds.setdefault(owner, set()).add(kind)

    au_path = directory / AU_FILENAME
    try:
        au_text, visibility_errors = rendered_markdown(
            au_path.read_text(encoding="utf-8"), au_path.name
        )
    except OSError as exc:
        return set(), errors + [f"cannot read AU acceptance registry: {exc}"]
    errors.extend(visibility_errors)
    header, placement_rows, error = first_table_after(
        au_text, AU_PLACEMENT_HEADING, AU_PLACEMENT_STOP
    )
    if error:
        errors.append(f"AU acceptance registry: {error}")
    elif header != AU_PLACEMENT_HEADER:
        errors.append(
            f"AU acceptance registry header mismatch: expected {AU_PLACEMENT_HEADER}, "
            f"got {header}"
        )
    else:
        declaration_re = re.compile(
            r"\*\*(AU\d+\.\d+(?:[a-z])?)\s+—\s+(test|probe):\*\*"
        )
        for row in placement_rows:
            declarations = declaration_re.findall(row[7])
            owners = re.findall(r"\*\*(T\d+-W\d+[A-Za-z]*)\*\*", row[4])
            if len(declarations) != len(owners) or not declarations:
                errors.append(
                    "AU placement row has opaque item/WP ownership: "
                    f"items={declarations}, owners={owners}"
                )
                continue
            for (_, kind), owner in zip(declarations, owners):
                node_kinds.setdefault(owner, set()).add(kind)

    probe_nodes = {
        node for node, kinds in node_kinds.items() if kinds & {"probe", "test+probe"}
    }
    return probe_nodes, errors


def validate_dispatch_dag(path):
    """Validate the optional schema-v1 DAG as executable registry evidence."""
    dag_fail = []
    text, visibility_errors = rendered_markdown(
        path.read_text(encoding="utf-8"), path.name
    )
    dag_fail.extend(f"DAG {error}" for error in visibility_errors)
    staged_nodes, au_nodes, registry_errors = load_supplemental_registries(path.parent)
    dag_fail.extend(f"DAG {error}" for error in registry_errors)
    probe_nodes, probe_registry_errors = load_probe_node_registry(path.parent)
    dag_fail.extend(f"DAG {error}" for error in probe_registry_errors)
    expected_nodes = set(WP) | staged_nodes | au_nodes

    if len(exact_line_positions(text, DAG_SCHEMA_MARKER)) != 1:
        dag_fail.append(f"DAG exact schema marker mismatch in {path.name}")
    for heading in (DAG_TABLE_HEADING, DAG_BATCH_HEADING):
        count = len(exact_line_positions(text, heading))
        if count != 1:
            dag_fail.append(
                f"DAG EXACT HEADING count for {heading!r}: expected 1, got {count}"
            )

    header, table_rows, error = first_table_after(
        text, DAG_TABLE_HEADING, DAG_BATCH_HEADING
    )
    if error:
        dag_fail.append(f"DAG {error}")
        return dag_fail
    if header != DAG_HEADER:
        dag_fail.append(
            f"DAG table header mismatch: expected {DAG_HEADER}, got {header}"
        )
        return dag_fail

    nodes = {}
    physical_nodes = []
    opaque_nodes = []
    artifacts = {}
    scopes_by_node = {}
    exclusions_by_node = {}
    for row in table_rows:
        node = plain_markdown(row[0])
        if not re.fullmatch(r"T\d+-W\d+[a-z]?", node):
            opaque_nodes.append(node)
            continue
        physical_nodes.append(node)
        if node in nodes:
            continue

        phase = plain_markdown(row[1])
        predecessor_cell = plain_markdown(row[2])
        predecessors = (
            []
            if predecessor_cell == "—"
            else [token.strip() for token in predecessor_cell.split(",")]
        )
        if any(not part.strip() for part in row[3].split(";")):
            dag_fail.append(
                f"DAG {node} scope/artifact cell contains an empty registry atom"
            )
        scopes, exclusions, node_artifacts, opaque_artifacts = parse_dag_scope_cell(
            row[3]
        )
        if opaque_artifacts:
            dag_fail.append(
                f"DAG {node} has invalid artifact filename(s) {list(opaque_artifacts)}"
            )

        lane = plain_markdown(row[4])
        nodes[node] = {
            "phase": phase,
            "predecessors": predecessors,
            "scopes": scopes,
            "exclusions": exclusions,
            "artifacts": node_artifacts,
            "lane": lane,
        }
        scopes_by_node[node] = scopes
        exclusions_by_node[node] = exclusions

        if phase not in DAG_PHASES:
            dag_fail.append(f"DAG {node} has unknown phase/wave {phase!r}")
        if not lane:
            dag_fail.append(f"DAG {node} has an empty lane")
        for artifact in node_artifacts:
            if artifact in artifacts:
                dag_fail.append(
                    f"DAG artifact filename {artifact!r} is shared by "
                    f"{artifacts[artifact]} and {node}"
                )
            else:
                artifacts[artifact] = node
        if phase == "W3 live proof" and not node_artifacts:
            dag_fail.append(f"DAG live-proof node {node} has no artifact filename")
        if any(scope == "docs/plan/evidence/**" for scope in scopes):
            dag_fail.append(f"DAG {node} owns forbidden broad evidence scope")

    duplicate_nodes = sorted(
        node for node, count in Counter(physical_nodes).items() if count > 1
    )
    if duplicate_nodes:
        dag_fail.append(f"DAG duplicate physical node rows: {duplicate_nodes}")
    if opaque_nodes:
        dag_fail.append(f"DAG opaque/invalid node rows: {opaque_nodes}")

    missing_vertices = sorted(expected_nodes - set(nodes))
    unexpected_vertices = sorted(set(nodes) - expected_nodes)
    if missing_vertices or unexpected_vertices:
        dag_fail.append(
            "DAG exact registry vertex mismatch: "
            f"missing {missing_vertices}, unexpected {unexpected_vertices}"
        )
    for node in sorted(set(WP) & set(nodes)):
        expected_wave_prefix = f"W{doc_wave(WP[node][3])} "
        if not nodes[node]["phase"].startswith(expected_wave_prefix):
            dag_fail.append(
                f"DAG principal phase mismatch for {node}: expected "
                f"{expected_wave_prefix.strip()}, got {nodes[node]['phase']!r}"
            )

    graph_predecessors = {}
    for node, record in nodes.items():
        graph_predecessors[node] = set()
        duplicate_predecessors = sorted(
            token
            for token, count in Counter(record["predecessors"]).items()
            if count > 1
        )
        if duplicate_predecessors:
            dag_fail.append(
                f"DAG {node} repeats predecessors: {duplicate_predecessors}"
            )
        for predecessor in record["predecessors"]:
            if predecessor in nodes:
                graph_predecessors[node].add(predecessor)
            elif predecessor not in DAG_EXTERNAL_NODES:
                dag_fail.append(
                    f"DAG {node} has unknown predecessor token {predecessor!r}"
                )

    # Prove acyclicity and retain transitive predecessor sets for exclusive
    # scope serialization checks.
    remaining = {node: set(preds) for node, preds in graph_predecessors.items()}
    emitted = set()
    deterministic_batches = []
    while remaining:
        ready = sorted(
            node for node, predecessors in remaining.items() if predecessors <= emitted
        )
        if not ready:
            dag_fail.append(f"DAG cycle/residual nodes: {sorted(remaining)}")
            break
        batch = ready[:8]
        deterministic_batches.append(batch)
        emitted.update(batch)
        for node in batch:
            del remaining[node]

    def transitively_precedes(left, right):
        pending = list(graph_predecessors.get(right, ()))
        seen = set()
        while pending:
            predecessor = pending.pop()
            if predecessor == left:
                return True
            if predecessor not in seen:
                seen.add(predecessor)
                pending.extend(graph_predecessors.get(predecessor, ()))
        return False

    scoped_nodes = sorted(scopes_by_node)
    for index, left in enumerate(scoped_nodes):
        for right in scoped_nodes[index + 1 :]:
            if transitively_precedes(left, right) or transitively_precedes(right, left):
                continue
            overlaps = scope_overlap(
                scopes_by_node[left],
                exclusions_by_node[left],
                scopes_by_node[right],
                exclusions_by_node[right],
            )
            if overlaps:
                dag_fail.append(
                    f"DAG parallel path/glob scope collision in {left} and {right}: "
                    f"{overlaps}"
                )

    for node in sorted(probe_nodes & set(nodes)):
        if node != "T7-W4b" and "T7-W4b" not in nodes[node]["predecessors"]:
            dag_fail.append(
                f"DAG probe/test+probe node {node} does not declare T7-W4b "
                "as an exact hard predecessor"
            )
        if not nodes[node]["artifacts"]:
            dag_fail.append(
                f"DAG probe/test+probe node {node} has no evidence artifact filename"
            )
    unregistered_probe_nodes = sorted(probe_nodes - set(nodes))
    if unregistered_probe_nodes:
        dag_fail.append(
            f"DAG probe/test+probe registry nodes missing from graph: {unregistered_probe_nodes}"
        )

    batch_section_starts = exact_line_positions(text, DAG_BATCH_HEADING)
    rendered_batches = []
    rendered_ordinals = []
    if len(batch_section_starts) == 1:
        batch_section = text[batch_section_starts[0] :]
        for ordinal, node_cell in re.findall(
            r"^B(\d{2}):\s+((?:T\d+-W\d+[a-z]?(?:\s+|$))+)",
            batch_section,
            re.M,
        ):
            rendered_ordinals.append(ordinal)
            rendered_batches.append(node_cell.split())

    expected_ordinals = [f"{index:02d}" for index in range(len(rendered_batches))]
    if rendered_ordinals != expected_ordinals:
        dag_fail.append(
            f"DAG ready-set ordinals mismatch: expected {expected_ordinals}, "
            f"got {rendered_ordinals}"
        )
    rendered_nodes = [node for batch in rendered_batches for node in batch]
    duplicate_rendered = sorted(
        node for node, count in Counter(rendered_nodes).items() if count > 1
    )
    if duplicate_rendered:
        dag_fail.append(f"DAG ready sets repeat nodes: {duplicate_rendered}")
    if set(rendered_nodes) != set(nodes):
        dag_fail.append(
            f"DAG ready-set vertex mismatch: missing {sorted(set(nodes) - set(rendered_nodes))}, "
            f"unexpected {sorted(set(rendered_nodes) - set(nodes))}"
        )
    oversized_batches = [
        f"B{index:02d}"
        for index, batch in enumerate(rendered_batches)
        if len(batch) > 8
    ]
    if oversized_batches:
        dag_fail.append(f"DAG ready sets over cap 8: {oversized_batches}")
    if emitted == set(nodes) and rendered_batches != deterministic_batches:
        dag_fail.append(
            f"DAG rendered ready sets are not deterministic Kahn output: "
            f"expected {deterministic_batches}, got {rendered_batches}"
        )

    return dag_fail


raw_doc = Path(sys.argv[1]).read_text(encoding="utf-8")
doc, visibility_errors = rendered_markdown(raw_doc, Path(sys.argv[1]).name)
fail = list(visibility_errors)

required_headings = [
    SUITE_HEADING,
    OWNER_HEADING,
    WAVES_HEADING,
    REV5_HEADING,
    *CAPABILITY_HEADINGS.values(),
    REV4_HEADING,
    *WAVE_HEADINGS.values(),
    ARMING_HEADING,
]
for heading in required_headings:
    count = len(exact_line_positions(doc, heading))
    if count != 1:
        fail.append(f"EXACT HEADING count for {heading!r}: expected 1, got {count}")

suite_starts = exact_line_positions(doc, SUITE_HEADING)
owner_starts = exact_line_positions(doc, OWNER_HEADING)
if (
    len(suite_starts) == 1
    and len(owner_starts) == 1
    and suite_starts[0] < owner_starts[0]
):
    sec = doc[suite_starts[0] : owner_starts[0]]
else:
    sec = ""
    fail.append("acceptance suite cannot be isolated between its exact headings")

# Scan every physical table line in the suite, rather than only rows that fit
# the expected decoration.  This is deliberately broad enough to see e.g.
# ``| *A8.1* | ... |`` and then reject it as an unexpected frozen-suite row.
rows = []
opaque_acceptance_rows = []
for line_number, line in enumerate(sec.splitlines(), start=1):
    if not line.startswith("|"):
        continue
    cells = markdown_cells(line)
    if len(cells) < 2:
        continue
    item_id = acceptance_cell_id(cells[0])
    if item_id:
        rows.append((item_id, cells[1].strip()))
    elif re.search(r"\bA\d+\.\d+\b", cells[0]):
        opaque_acceptance_rows.append((line_number, cells[0]))

if opaque_acceptance_rows:
    fail.append(f"OPAQUE physical acceptance rows: {opaque_acceptance_rows}")

physical = Counter(i for i, _ in rows)
duplicate_rows = sorted(i for i, count in physical.items() if count > 1)
if duplicate_rows:
    fail.append(f"DUPLICATE physical acceptance rows: {duplicate_rows}")

items = {i: k.strip() for i, k in rows}
unknown_kinds = sorted((i, k) for i, k in items.items() if k not in ITEM_KINDS)
if unknown_kinds:
    fail.append(f"UNKNOWN acceptance kind labels: {unknown_kinds}")

missing_items = sorted(set(FROZEN_ITEM_KINDS) - set(items))
unexpected_items = sorted(set(items) - set(FROZEN_ITEM_KINDS))
if missing_items or unexpected_items:
    fail.append(
        f"FROZEN acceptance id mismatch: missing {missing_items}, "
        f"unexpected {unexpected_items}"
    )

kind_drift = sorted(
    (item_id, FROZEN_ITEM_KINDS[item_id], items[item_id])
    for item_id in set(items) & set(FROZEN_ITEM_KINDS)
    if items[item_id] != FROZEN_ITEM_KINDS[item_id]
)
if kind_drift:
    fail.append(f"FROZEN per-item kind drift (id, expected, got): {kind_drift}")

# Validate every named acceptance table as a complete, ordered partition.  The
# global row scan above catches extra ids; this catches moved rows, opaque row
# labels, renamed capability headings, and tables with a changed shape.
for heading, stop_heading, expected_header, expected_ids in ACCEPTANCE_TABLES:
    header, table_rows, error = first_table_after(doc, heading, stop_heading)
    if error:
        fail.append(error)
        continue
    if header != list(expected_header):
        fail.append(
            f"Acceptance table header mismatch under {heading!r}: "
            f"expected {list(expected_header)}, got {header}"
        )
        continue

    table_ids = []
    opaque_labels = []
    for row in table_rows:
        item_id = acceptance_cell_id(row[0])
        if item_id is None:
            opaque_labels.append(row[0])
            continue
        table_ids.append(item_id)
        expected_kind = FROZEN_ITEM_KINDS.get(item_id)
        if expected_kind is not None and row[1].strip() != expected_kind:
            fail.append(
                f"{item_id} kind mismatch under {heading!r}: "
                f"expected {expected_kind!r}, got {row[1].strip()!r}"
            )
    if opaque_labels:
        fail.append(
            f"Acceptance rows with opaque/invalid ids under {heading!r}: "
            f"{opaque_labels}"
        )
    if tuple(table_ids) != expected_ids:
        fail.append(
            f"Acceptance partition/order mismatch under {heading!r}: "
            f"expected {list(expected_ids)}, got {table_ids}"
        )

# Reconcile the rendered mechanical summary with the same frozen catalogue.
# Whitespace and Markdown wrapping do not matter; every word and count does.
kind_counts = Counter(FROZEN_ITEM_KINDS.values())
expected_summary = plain_markdown(
    f"""**{len(FROZEN_ITEM_KINDS)} rows — {kind_counts["test"]} `test`,
    {kind_counts["probe"]} `probe`, {kind_counts["test+probe"]} `test+probe`,
    {kind_counts["judged"]} `judged` ({", ".join(sorted(JUDGED_TO_OWNER))}), plus
    {kind_counts["—"]} withdrawn rows ({", ".join(sorted(WITHDRAWN))});
    {len(FROZEN_ITEM_KINDS) - len(WITHDRAWN)} rows are live.** `wp-check.py`
    reports {len({i for v, _, _, _ in WP.values() for i in v})} non-judged items
    owned exactly once and routes the three judged rows to their owners. The
    separate `AU` intake is not part of these rows."""
)
rendered_summaries = [
    plain_markdown(match)
    for match in re.findall(r"(?ms)^\*\*\d+ rows\b.*?^of these rows\.$", sec)
]
if rendered_summaries != [expected_summary]:
    fail.append(
        "RENDERED acceptance summary mismatch: "
        f"expected {expected_summary!r}, got {rendered_summaries}"
    )

withdrawn = {i for i, k in items.items() if k == "—"}
if withdrawn != WITHDRAWN:
    fail.append(
        f"WITHDRAWN label mismatch: expected {sorted(WITHDRAWN)}, got {sorted(withdrawn)}"
    )

judged = {i for i, k in items.items() if k == "judged"}
if judged != JUDGED_TO_OWNER:
    fail.append(
        f"JUDGED label mismatch: expected {sorted(JUDGED_TO_OWNER)}, got {sorted(judged)}"
    )

live = set(items) - withdrawn

owned = Counter(i for v, _, _, _ in WP.values() for i in v)

unowned = sorted(live - set(owned) - JUDGED_TO_OWNER)
if unowned:
    fail.append(f"UNOWNED items ({len(unowned)}): {unowned}")

dup = sorted(i for i, c in owned.items() if c > 1)
if dup:
    fail.append(f"DOUBLE-OWNED items: {dup}")

ghost = sorted(set(owned) - live)
if ghost:
    fail.append(f"OWNED BUT NOT IN SUITE: {ghost}")

empty = sorted(w for w, (v, _, _, _) in WP.items() if not v)
if empty:
    fail.append(f"WPs with ZERO items (no structural ownership): {empty}")

oversized = sorted(w for w, (v, _, _, _) in WP.items() if len(v) > 4)
if oversized:
    fail.append(f"WPs over the 4-item sweet-spot ceiling: {oversized}")

noinv = sorted(w for w, (_, iv, _, _) in WP.items() if not iv or not set(iv) <= INV)
if noinv:
    fail.append(f"WPs with no/unknown invariant declared: {noinv}")

# Cross-check the user-visible wave tables against the catalogue. This makes
# edits to ownership, WP ids, wave placement, and declared scopes observable.
seen_doc_wps = {}
for wave in range(4):
    header, table_rows, error = first_table_after(
        doc, WAVE_HEADINGS[wave], WAVE_HEADINGS[wave + 1]
    )
    if error:
        fail.append(error)
        continue

    expected_header = {
        0: ["wp", "owns", "notes"],
        1: ["wp", "owns", "exclusive files (the x)", "route after freeze", "dep"],
        2: ["#", "wp", "owns", "scope"],
        3: ["wp", "owns", "dep"],
    }[wave]
    if header != expected_header:
        fail.append(
            f"Wave {wave} table header mismatch: expected {expected_header}, got {header}"
        )
        continue

    wp_col = header.index("wp")
    owns_col = header.index("owns")
    scope_col = next(
        (
            header.index(name)
            for name in ("exclusive files (the x)", "scope")
            if name in header
        ),
        None,
    )
    wave_rows = {}
    wave_order = []
    wave_ordinals = []
    opaque = []
    for row in table_rows:
        wp_match = re.match(r"^\s*\*\*(T\d+-W\d+[a-z]?)\*\*", row[wp_col])
        if not wp_match:
            # Wave 0 deliberately includes the owner-only O1 row.
            if wave == 0 and plain_markdown(row[wp_col]).startswith("O1 "):
                continue
            opaque.append(plain_markdown(row[wp_col]))
            continue
        wp_id = wp_match.group(1)
        if wp_id in wave_rows or wp_id in seen_doc_wps:
            fail.append(f"DUPLICATE physical WP row: {wp_id}")
            continue
        owned_ids = re.findall(r"\bA\d+\.\d+\b", row[owns_col])
        if len(owned_ids) != len(set(owned_ids)):
            fail.append(
                f"DUPLICATE acceptance id inside {wp_id} ownership cell: {owned_ids}"
            )
        scope = row[scope_col] if scope_col is not None else None
        wave_rows[wp_id] = (owned_ids, scope)
        wave_order.append(wp_id)
        if wave == 2:
            wave_ordinals.append(row[header.index("#")].strip())
        seen_doc_wps[wp_id] = wave

    if opaque:
        fail.append(f"Wave {wave} rows with opaque/invalid WP labels: {opaque}")

    expected_wps = {
        wp_id
        for wp_id, (_, _, _, catalog_wave) in WP.items()
        if doc_wave(catalog_wave) == wave
    }
    actual_wps = set(wave_rows)
    if actual_wps != expected_wps:
        fail.append(
            f"Wave {wave} WP catalogue mismatch: missing {sorted(expected_wps - actual_wps)}, "
            f"unexpected {sorted(actual_wps - expected_wps)}"
        )

    if wave == 2:
        expected_ordinals = [str(i) for i in range(1, len(WAVE2_CHAIN) + 1)]
        if wave_order != list(WAVE2_CHAIN):
            fail.append(
                f"Wave 2 SERIAL chain order mismatch: expected {list(WAVE2_CHAIN)}, "
                f"got {wave_order}"
            )
        if wave_ordinals != expected_ordinals:
            fail.append(
                f"Wave 2 SERIAL ordinals mismatch: expected {expected_ordinals}, "
                f"got {wave_ordinals}"
            )

    for wp_id in sorted(actual_wps & expected_wps):
        doc_owned, doc_scope = wave_rows[wp_id]
        catalog_owned = WP[wp_id][0]
        if doc_owned != catalog_owned:
            fail.append(
                f"{wp_id} Markdown ownership mismatch: expected {catalog_owned}, got {doc_owned}"
            )
        if doc_scope is not None:
            expected_scope = WP[wp_id][2]
            if plain_markdown(doc_scope) != plain_markdown(expected_scope):
                fail.append(
                    f"{wp_id} Markdown scope mismatch: expected {expected_scope!r}, got {doc_scope!r}"
                )

# Principal scope collision: compare path/glob atoms rather than whole prose
# cells, and honor only explicit carve-outs.  Wave 2 is a declared serial chain.
principal_scopes = {}
for node, (_, _, scope, wave) in WP.items():
    atoms, exclusions = parse_scope_declaration(scope)
    principal_scopes[node] = (wave, atoms, exclusions)
principal_collisions = []
principal_nodes = sorted(principal_scopes)
for index, left in enumerate(principal_nodes):
    left_wave, left_atoms, left_exclusions = principal_scopes[left]
    if left_wave == 2:
        continue
    for right in principal_nodes[index + 1 :]:
        right_wave, right_atoms, right_exclusions = principal_scopes[right]
        if right_wave != left_wave or right_wave == 2:
            continue
        overlaps = scope_overlap(
            left_atoms, left_exclusions, right_atoms, right_exclusions
        )
        if overlaps:
            principal_collisions.append((left, right, overlaps))
if principal_collisions:
    fail.append(f"PARALLEL path/glob scope collisions: {principal_collisions}")

dag_path = Path(__file__).with_name(DAG_FILENAME)
if dag_path.exists():
    fail.extend(validate_dispatch_dag(dag_path))

print(
    f"suite rows {len(rows)} physical / {len(items)} unique · live {len(live)} · withdrawn {sorted(withdrawn)}"
)
print(
    f"WPs {len(WP)} · items owned {len(owned)} · judged->owner {sorted(JUDGED_TO_OWNER)}"
)
print(
    f"items per WP: min {min(len(v) for v, _, _, _ in WP.values())} max {max(len(v) for v, _, _, _ in WP.values())}"
)
for f in fail:
    print("  BLOCK:", f)
print(
    "\nwp-check:",
    "BLOCKED"
    if fail
    else "PASS — structural ownership tables are internally consistent",
)
sys.exit(1 if fail else 0)
