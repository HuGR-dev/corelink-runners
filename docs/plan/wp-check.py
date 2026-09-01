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
}


def exact_line_positions(text, heading):
    return [m.start() for m in re.finditer(rf"^{re.escape(heading)}$", text, re.M)]


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


def doc_wave(wave):
    if wave == 0:
        return 0
    if 0 < wave < 2:
        return 1
    return int(wave)


def validate_dispatch_dag(path):
    """Validate the optional schema-v1 DAG as executable registry evidence."""
    dag_fail = []
    text = path.read_text(encoding="utf-8")

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
    scope_owners = {}
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
        scope_parts = [part.strip() for part in row[3].split(";")]
        if any(not part for part in scope_parts):
            dag_fail.append(
                f"DAG {node} scope/artifact cell contains an empty registry atom"
            )
            scopes = []
            artifact = ""
        else:
            scopes = [plain_markdown(part) for part in scope_parts[:-1]]
            artifact = plain_markdown(scope_parts[-1])

        lane = plain_markdown(row[4])
        nodes[node] = {
            "phase": phase,
            "predecessors": predecessors,
            "scopes": scopes,
            "artifact": artifact,
            "lane": lane,
        }
        for scope in set(scopes):
            scope_owners.setdefault(scope, []).append(node)

        if phase not in DAG_PHASES:
            dag_fail.append(f"DAG {node} has unknown phase/wave {phase!r}")
        if not lane:
            dag_fail.append(f"DAG {node} has an empty lane")
        if artifact != "—":
            if not re.fullmatch(r"docs/plan/evidence/[A-Za-z0-9._-]+\.json", artifact):
                dag_fail.append(
                    f"DAG {node} has invalid artifact filename {artifact!r}"
                )
            elif artifact in artifacts:
                dag_fail.append(
                    f"DAG artifact filename {artifact!r} is shared by "
                    f"{artifacts[artifact]} and {node}"
                )
            else:
                artifacts[artifact] = node
        if phase == "W3 live proof" and artifact == "—":
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

    missing_principal = sorted(set(WP) - set(nodes))
    if missing_principal:
        dag_fail.append(f"DAG missing principal WP nodes: {missing_principal}")
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

    for scope, owners in sorted(scope_owners.items()):
        if len(owners) < 2:
            continue
        for index, left in enumerate(owners):
            for right in owners[index + 1 :]:
                if not (
                    transitively_precedes(left, right)
                    or transitively_precedes(right, left)
                ):
                    dag_fail.append(
                        f"DAG exclusive scope {scope!r} is parallel in {left} and {right}"
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


doc = Path(sys.argv[1]).read_text(encoding="utf-8")
fail = []

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

# parallel-scope collision: only within the same integer wave, and wave 2 is serial by design
scopes = {}
for w, (_, _, sc, wave) in WP.items():
    if wave in (2,):  # serial chain — shared scope is the plan's explicit decision
        continue
    scopes.setdefault((wave, sc), []).append(w)
collide = {k: v for k, v in scopes.items() if len(v) > 1}
if collide:
    fail.append(f"PARALLEL scope collisions: {collide}")

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
