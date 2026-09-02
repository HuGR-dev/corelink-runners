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
    "T3-W5": 4,
}
EXPECTED_EXTENSION_WAVES = {
    "T4-W1": 2,
    "T8-W1": 2,
    "T6-W10": 3,
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
EXPECTED_PLACEMENT_BUCKETS = {
    finding.removesuffix("p"): frozenset({bucket})
    for bucket, findings in EXPECTED_BUCKET_FINDINGS.items()
    for finding in findings
}
# The three source findings split across a repo packet and a live-proof packet.
EXPECTED_PLACEMENT_BUCKETS.update(
    {
        "union-06": frozenset({"W2-serial-worker", "W3-live-proof"}),
        "union-09": frozenset({"W1-parallel", "W3-live-proof"}),
        "union-23": frozenset({"RELAY", "DOCS-sweep"}),
        "M3": frozenset({"W2-serial-worker", "W3-live-proof"}),
    }
)
EXPECTED_PROPOSAL_WPS = set(EXPECTED_PROPOSAL_WAVES)
EXPECTED_EXTENSION_WPS = set(EXPECTED_EXTENSION_WAVES)
STAGED_DECISION_IDS = {"D11", "D12", "D13"}
STAGED_OWNER_IDS = {
    "O-CFINVENTORY",
    "O-CFCANCEL",
    "O-CFRATE",
    "O-MONITORHOST",
}
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
READY_SET_HEADING = "## Deterministic ready sets and proof"
READY_SET_FENCE_OPEN = "```text"
READY_SET_FENCE_CLOSE = "```"
EXPECTED_DAG_VERTICES = 69
O_CFINVENTORY_ARTIFACT = "docs/plan/evidence/O-CFINVENTORY-provider-capabilities.json"
O_CFCANCEL_ARTIFACT = "docs/plan/evidence/O-CFCANCEL-provider-cancellation.json"
MONITOR_REARM_TUPLE = (
    "monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,"
    "ingress_key_epoch_map_digest,expected_source_registry_digest,"
    "delivery_route_policy_digest,provider_adapter_api_capability_digest,"
    "attestation_ack_signer_trust_revocation_digest)"
)
A6_17_WINDOW_TUPLE = (
    "A6.17_window_tuple=(monitor_rearm_tuple_digest,"
    "sensitivity_scheduler_deployed_runtime_digest,"
    "sensitivity_scheduler_config_digest,"
    "sensitivity_scheduler_key_id_credential_epoch_digest,"
    "receipt_verifier_deployed_runtime_digest,"
    "receipt_verifier_config_digest,on_call_escalation_schedule_digest)"
)
SIGNED_ACK_SCHEMA = (
    "(ack_version,event_id,producer_seq,payload_digest,source,service,application,"
    "key_id,credential_epoch,monitor_rearm_tuple_digest,ingest_commit_id,"
    "committed_at,signer_key_id,signer_epoch,signature)"
)
PAGE_ACK_SCHEMA = (
    "page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,"
    "on_call_identity,on_call_schedule_digest,action,payload_digest,"
    "monitor_rearm_tuple_digest,acknowledged_at,expires_at,signer_key_id,"
    "signer_epoch,signature)"
)
SIGNER_ROTATION_MANIFEST = (
    "signer_rotation_manifest=(manifest_version,active_signer_key_id,"
    "active_signer_epoch,next_signer_key_id,next_signer_epoch,"
    "revoked_signer_set_digest,overlap_started_at,overlap_expires_at,"
    "recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,"
    "issued_at,signature)"
)
ACK_RECOVERY_SCHEMA = (
    "ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,"
    "service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,"
    "ingest_commit_id,original_ack_digest,revocation_record_digest,"
    "signer_rotation_manifest_digest,current_monitor_rearm_tuple_digest,"
    "recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)"
)
CANARY_ACTIVATION_TUPLE = (
    "canary_activation_tuple=(activation_version,lifecycle_source,lifecycle_service,"
    "lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,"
    "synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,"
    "monitor_rearm_tuple_digest,producer_image_digest,"
    "producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,"
    "synthetic_flag_value,activated_at)"
)
O_CFRATE_EVIDENCE = (
    "O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,"
    "accountable_role,attested_at,review_input_sha,deployed_image_digest,provider,"
    "provider_api_or_export_version,account_id,plan,billing_period_start,"
    "billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_receipt_id,"
    "threshold_receipt_sha256,budget_interval_start,budget_interval_end,source,"
    "source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,"
    "complete_provider_cursor,invoice_line_id,invoice_line_description,quantity,unit,"
    "currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,"
    "rate_effective_to,attempt_count,failed_attempt_count,retry_count,"
    "idle_wakeup_count,served_count,failure_rate_numerator_formula,"
    "failure_rate_denominator_formula,failure_rate_numerator,"
    "failure_rate_denominator,observed_failure_rate,failure_rate_threshold,"
    "billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,"
    "cost_per_served_attempt,cost_per_served_attempt_threshold,owner_signature)"
)
STAGED_WP_HEADER = [
    "wp",
    "owns",
    "count",
    "exclusive x",
    "exact acceptance prerequisite",
    "pre-decided implementation contract",
]
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
    buckets: frozenset[str]
    dependency: str


def markdown_cells(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def plain_markdown(value: str) -> str:
    value = value.replace("*", "").replace("`", "")
    return re.sub(r"\s+", " ", value).strip()


def _canonical_registry_content(block: str) -> bool:
    """Return whether a hidden Markdown block contains a canonical registry."""

    block = block.replace("<!--", "").replace("-->", "")
    canonical_headings = {
        "## 1. Summary counts",
        "## 2. Per-finding placement",
        "## 3. Staged ownership consequences",
        "## Canonical node table",
        "## 3. Acceptance proposals — reserved, not promoted",
        "## 4. Proposed WP packet contracts (not a second dispatch DAG)",
    }
    canonical_markers = {
        "**New WPs and their item counts** (all ≤ the four-item ceiling):",
        "**Extensions to existing WPs:**",
    }
    headers = (
        SUMMARY_HEADER,
        PLACEMENT_HEADER,
        DECLARATION_HEADER,
        DAG_HEADER,
        STAGED_WP_HEADER,
        STAGED_ACCEPTANCE_HEADER,
    )
    for line in block.splitlines():
        visible_line = line.lstrip(" ")
        if visible_line in canonical_headings or visible_line in canonical_markers:
            return True
        if not visible_line.startswith("|"):
            continue
        cells = markdown_cells(visible_line)
        normalized = [plain_markdown(cell).lower() for cell in cells]
        if normalized in headers:
            return True
        first_cell = plain_markdown(cells[0]) if cells else ""
        if (
            AU_RE.search(visible_line)
            or re.fullmatch(
                r"(?:union-\d{2}|RH[59](?: \(partial\))?|M(?:3|5|19)(?: \(partial\))?)",
                first_cell,
            )
            or re.fullmatch(
                r"(?:new|existing) T\d+-W\d+[A-Za-z]*", first_cell, re.IGNORECASE
            )
            or re.fullmatch(r"T\d+-W\d+[A-Za-z]*", first_cell)
            or re.fullmatch(r"A\d+\.\d+", first_cell)
        ):
            return True
    return False


def markdown_visible_text(document: str) -> tuple[str, list[tuple[str, int]]]:
    """Mask fenced code and HTML comments while preserving lines and offsets.

    CommonMark fences may use backticks or tildes, have any marker length of at
    least three, carry an info string, and close only with the same marker and
    at least the opening length.  Masking (rather than deleting) keeps all
    diagnostics on physical source line numbers.
    """

    characters = list(document)
    hidden: list[tuple[str, int]] = []
    lines = document.splitlines(keepends=True)
    offsets: list[int] = []
    cursor = 0
    for line in lines:
        offsets.append(cursor)
        cursor += len(line)

    index = 0
    while index < len(lines):
        opening = re.match(r"^ {0,3}(`{3,}|~{3,})([^\r\n]*)", lines[index])
        if opening is None or (
            opening.group(1).startswith("`") and "`" in opening.group(2)
        ):
            index += 1
            continue
        marker = opening.group(1)[0]
        minimum = len(opening.group(1))
        close_pattern = re.compile(
            rf"^ {{0,3}}{re.escape(marker)}{{{minimum},}}[ \t]*(?:\r?\n)?$"
        )
        end_index = index + 1
        while end_index < len(lines) and not close_pattern.fullmatch(lines[end_index]):
            end_index += 1
        if end_index < len(lines):
            end_index += 1
        start_offset = offsets[index]
        end_offset = offsets[end_index] if end_index < len(lines) else len(document)
        block = document[start_offset:end_offset]
        if _canonical_registry_content(block):
            hidden.append(("fenced code block", index + 1))
        for position in range(start_offset, end_offset):
            if characters[position] not in "\r\n":
                characters[position] = " "
        index = end_index

    fence_masked = "".join(characters)
    cursor = 0
    while True:
        start = fence_masked.find("<!--", cursor)
        if start < 0:
            break
        close = fence_masked.find("-->", start + 4)
        end = len(document) if close < 0 else close + 3
        block = document[start:end]
        if _canonical_registry_content(block):
            hidden.append(("HTML comment", document.count("\n", 0, start) + 1))
        for position in range(start, end):
            if characters[position] not in "\r\n":
                characters[position] = " "
        cursor = end
        if close < 0:
            break
    return "".join(characters), hidden


def hidden_canonical_table_errors(document: str, label: str) -> list[str]:
    """Reject canonical registries hidden by Markdown rendering constructs."""

    _, hidden = markdown_visible_text(document)
    return [
        f"{label} hides canonical AU registry content in a {kind} at line {line_no}"
        for kind, line_no in hidden
    ]


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


def exact_ready_set_rows(document: str) -> tuple[list[tuple[str, str]], list[str]]:
    """Read ready sets only from one exact, closed canonical text fence.

    The ready-set proof is intentionally source-parsed rather than visible
    Markdown, but that must not make its claimed fence boundary advisory.  A
    moved row, a second lookalike section, or a different/unterminated fence is
    ambiguous dispatch input and therefore fails closed.
    """

    errors: list[str] = []
    lines = document.splitlines()
    headings = [index for index, line in enumerate(lines) if line == READY_SET_HEADING]
    heading_lookalikes = [
        index
        for index, line in enumerate(lines)
        if re.fullmatch(
            r" {0,3}#{1,6}[ \t]+deterministic[ \t]+ready[ \t]+sets"
            r"[ \t]+and[ \t]+proof[ \t]*#*[ \t]*",
            line,
            re.IGNORECASE,
        )
    ]
    if len(headings) != 1:
        errors.append(
            f"DAG ready-set heading must occur exactly once as {READY_SET_HEADING!r}: "
            f"got {len(headings)}"
        )
        return [], errors
    if heading_lookalikes != headings:
        errors.append(
            "DAG ready-set heading has additional or non-canonical lookalikes at "
            f"lines {[index + 1 for index in heading_lookalikes]}"
        )

    heading = headings[0]
    section_end = next(
        (
            index
            for index in range(heading + 1, len(lines))
            if re.match(r"^##(?:[ \t]+|$)", lines[index])
        ),
        len(lines),
    )
    fence_lines = [
        index
        for index in range(heading + 1, section_end)
        if re.match(r"^ {0,3}(?:`{3,}|~{3,})", lines[index])
    ]
    expected_fences = [READY_SET_FENCE_OPEN, READY_SET_FENCE_CLOSE]
    actual_fences = [lines[index] for index in fence_lines]
    if actual_fences != expected_fences:
        errors.append(
            "DAG ready sets must use exactly one closed canonical ```text fence; "
            f"got {actual_fences} at lines {[index + 1 for index in fence_lines]}"
        )
        body_indices: set[int] = set()
    else:
        body_indices = set(range(fence_lines[0] + 1, fence_lines[1]))

    ready_row_candidates = [
        index
        for index, line in enumerate(lines)
        if re.match(r"^[ \t]*(?:>[ \t]*)?B\d{2}[ \t]*:", line)
    ]
    outside = [index + 1 for index in ready_row_candidates if index not in body_indices]
    if outside:
        errors.append(
            f"DAG ready-set Bnn rows occur outside the canonical text fence: {outside}"
        )

    rows: list[tuple[str, str]] = []
    for index in sorted(body_indices):
        match = re.fullmatch(r"B(\d{2}): ([^\s]+(?: [^\s]+)*)", lines[index])
        if match is None:
            errors.append(
                f"DAG ready-set fence contains a non-canonical row at line {index + 1}: "
                f"{lines[index]!r}"
            )
            continue
        rows.append((match.group(1), match.group(2)))
    if body_indices and not rows:
        errors.append("DAG ready-set canonical text fence contains no batch rows")
    return rows, errors


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
                    frozenset(bucket_tokens),
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
    """Parse new-WP rows and the four existing-WP AU extensions."""

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


def read_staged_wp_registry(
    path: Path,
) -> tuple[set[str], set[str], set[str], list[str]]:
    """Read the exhaustive new/existing WP registry from the staged delta table."""

    raw_document = path.read_text(encoding="utf-8")
    errors = hidden_canonical_table_errors(raw_document, "staged principal WP registry")
    document, _ = markdown_visible_text(raw_document)
    section = section_between(
        document,
        "## 4. Proposed WP packet contracts (not a second dispatch DAG)",
        "## 5. One eligible baseline and required review sequence",
    )
    rows, table_errors = exact_markdown_table(
        section, STAGED_WP_HEADER, "staged principal WP packets"
    )
    errors.extend(table_errors)
    new_wps: set[str] = set()
    existing_wps: set[str] = set()
    packet_rows: dict[str, str] = {}
    for line_no, columns in rows:
        label = plain_markdown(columns[0])
        match = re.fullmatch(
            r"(new|existing) (T\d+-W\d+[A-Za-z]*)", label, re.IGNORECASE
        )
        if not match:
            errors.append(
                f"staged principal WP row {line_no} has an opaque WP label: {columns[0]!r}"
            )
            continue
        kind, wp = match.group(1).lower(), match.group(2)
        registry = new_wps if kind == "new" else existing_wps
        if wp in new_wps | existing_wps:
            errors.append(f"staged principal WP registry repeats {wp}")
        registry.add(wp)
        packet_rows[wp] = columns[1]
    if not new_wps:
        errors.append("staged principal WP registry contains no new WP packets")

    acceptance_section = section_between(
        document,
        "## 3. Acceptance proposals — reserved, not promoted",
        "### 3.1 Binding A3.29 liveness/safety matrix",
    )
    acceptance_rows, table_errors = exact_markdown_table(
        acceptance_section,
        STAGED_ACCEPTANCE_HEADER,
        "staged principal acceptance proposals",
    )
    errors.extend(table_errors)
    kinds: dict[str, str] = {}
    for line_no, columns in acceptance_rows:
        ids = A_RE.findall(columns[0])
        if len(ids) != 1:
            errors.append(
                f"staged principal acceptance row {line_no} has an opaque id: {columns[0]!r}"
            )
            continue
        kinds[ids[0]] = plain_markdown(columns[1]).lower()

    probe_wps: set[str] = set()
    for wp, owns in packet_rows.items():
        owns_lower = owns.lower()
        test_half = re.search(r"\brepo(?:/test)? half\b", owns_lower) is not None
        live_half = re.search(r"\blive(?:-probe)? half\b", owns_lower) is not None
        if test_half and live_half:
            errors.append(
                f"staged principal WP {wp} declares both test-only and live-probe phases"
            )
            continue
        owned_ids = A_RE.findall(owns)
        if test_half:
            continue
        if live_half or any("probe" in kinds.get(item, "") for item in owned_ids):
            probe_wps.add(wp)
    return new_wps, existing_wps, probe_wps, errors


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


def validate_provider_credential_contract(delta_document: str) -> list[str]:
    """Freeze provider-read isolation and the separate cancellation obstacle.

    O-CFINVENTORY remains one owner token/artifact but uses three isolated
    read-only principals.  O-CFCANCEL is a different token/artifact and grants
    no list access.  This is intentionally checked from the rendered normative
    obstacle sections rather than learned from incidental prose elsewhere.
    """

    errors: list[str] = []
    visible, _ = markdown_visible_text(delta_document)
    start_marker = "**O-CFINVENTORY (reserved obstacle).**"
    cancel_marker = "**O-CFCANCEL (reserved obstacle).**"
    monitor_marker = "**O-MONITORHOST ("
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    cancels = [
        match.start() for match in re.finditer(re.escape(cancel_marker), visible)
    ]
    monitors = [
        match.start() for match in re.finditer(re.escape(monitor_marker), visible)
    ]
    if (
        len(starts) != 1
        or len(cancels) != 1
        or len(monitors) != 1
        or not starts[0] < cancels[0] < monitors[0]
    ):
        return [
            "staged delta must contain one visible, ordered "
            "O-CFINVENTORY/O-CFCANCEL/O-MONITORHOST contract "
            f"(inventory={len(starts)}, cancel={len(cancels)}, "
            f"monitor={len(monitors)})"
        ]

    inventory_section = plain_markdown(visible[starts[0] : cancels[0]])
    cancel_section = plain_markdown(visible[cancels[0] : monitors[0]])
    artifact_count = inventory_section.count(O_CFINVENTORY_ARTIFACT)
    if artifact_count != 1:
        errors.append(
            "O-CFINVENTORY must name its one canonical capability artifact exactly once: "
            f"{O_CFINVENTORY_ARTIFACT}={artifact_count}"
        )

    requirements = {
        "three separately revocable read-only principals": (
            r"\bthree separately revocable, read-only provider principals\b"
        ),
        "reconciliation-only principal": (
            r"\bT3-W16's reconciliation/cross-check principal\b"
        ),
        "rearm-probe-only principal": r"\bT1-W6's rearm-probe principal\b",
        "external-monitor-only principal": r"\bT6-W12's external monitor principal\b",
        "isolated provider quotas and revocation": (
            r"\brate-limit/quota allocations and revocation controls are isolated\b"
        ),
        "three credential schemas": (
            r"\ball three credential schemas/permission matrices\b"
        ),
        "independent quota and revocation tests": (
            r"\bindependent quota/rate-limit and revocation tests\b"
        ),
        "T1-W6 isolated-principal routing": (
            r"\bT1-W6 therefore requires O-CFINVENTORY and uses only its dedicated "
            r"rearm-probe principal, never the reconciliation or monitor principal\b"
        ),
    }
    for label, pattern in requirements.items():
        if re.search(pattern, inventory_section, re.IGNORECASE) is None:
            errors.append(f"O-CFINVENTORY omits required {label} contract")

    cancel_artifact_count = cancel_section.count(O_CFCANCEL_ARTIFACT)
    if cancel_artifact_count != 1:
        errors.append(
            "O-CFCANCEL must name its one canonical cancellation artifact exactly once: "
            f"{O_CFCANCEL_ARTIFACT}={cancel_artifact_count}"
        )
    cancel_requirements = {
        "linearizable no-future-materialization barrier": (
            r"\blinearizable barrier after which that start cannot materialize\b"
        ),
        "no numeric-timeout substitution": (
            r"\ba locally measured latency or a numeric timeout is never a substitute\b"
        ),
        "indefinite fail-closed refusal": (
            r"\bO-CFCANCEL remains unresolved, cancellation stays "
            r"RECONCILIATION_REFUSED indefinitely\b"
        ),
        "no provider-list capability": r"\bO-CFCANCEL grants no provider-list access\b",
        "independent dual-token route": (
            r"\bthe two obstacles are independent and T3-W16 requires both\b"
        ),
    }
    for label, pattern in cancel_requirements.items():
        if re.search(pattern, cancel_section, re.IGNORECASE) is None:
            errors.append(f"O-CFCANCEL omits required {label} contract")
    return errors


def validate_delta_canary_activation_authority(delta_document: str) -> list[str]:
    """Keep A6.22 bind-only; only T6-W10 may seal or arm activation."""

    visible, _ = markdown_visible_text(delta_document)
    rows = [line for line in visible.splitlines() if line.startswith("| **A6.22** |")]
    if len(rows) != 1:
        return [
            "round-3 delta must contain exactly one visible A6.22 acceptance row, "
            f"got {len(rows)}"
        ]
    row = plain_markdown(rows[0])
    requirements = {
        "T6-W14 bind-only/default-off boundary": (
            r"\bT6-W14 remains bind-only/default-off and may neither seal "
            r"canary_activation_tuple nor change either flag\b"
        ),
        "T6-W10 sole seal/arm authority": (
            r"\bonly later T6-W10 may seal the byte-exact tuple and change only the "
            r"flag value committed for that phase\b"
        ),
        "RED on T6-W14 seal/arm": r"\bT6-W14 seals or arms\b",
        "green only under T6-W10": (
            r"\bonly T6-W10 seals one byte-exact canary_activation_tuple per phase, "
            r"changes only its committed flag value to exact 1, and arms/proves the "
            r"lifecycle and synthetic phases in order\b"
        ),
    }
    errors = [
        f"A6.22 activation authority omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, row, re.IGNORECASE) is None
    ]
    forbidden = re.search(
        r"\b(?:before\s+)?T6-W14\s+may\s+(?:set|change|seal|arm)|"
        r"\bT6-W14\s+owner-arm",
        row,
        re.IGNORECASE,
    )
    if forbidden:
        errors.append(
            "A6.22 acceptance row grants forbidden T6-W14 activation authority: "
            f"{forbidden.group(0)!r}"
        )
    return errors


def normalized_contract_text(document: str) -> str:
    """Collapse presentation-only Markdown whitespace for semantic clauses."""

    return re.sub(r"\s+", " ", plain_markdown(document)).strip()


def contract_section_between(document: str, start: str, end: str) -> str:
    """Return one ordered contract section, or empty on missing/ambiguous markers."""

    if document.count(start) != 1 or document.count(end) != 1:
        return ""
    start_index = document.index(start)
    end_index = document.index(end, start_index + len(start))
    return document[start_index:end_index] if start_index < end_index else ""


def canonical_schema_section(document: str, label: str) -> str:
    """Return the normative section for a schema, never a trailing note/example."""

    if "# Go-Live Remediation Plan" in document:
        if label == "O-CFRATE evidence":
            return contract_section_between(
                document, "### Wave 4", "## 6. Owner arming"
            )
        return contract_section_between(
            document, "## 1. The live picture", "## 2. Scope"
        )
    if "# Round-3 remediation delta" in document:
        if label == "canary activation":
            return contract_section_between(
                document, "## 3. Acceptance proposals", "### 3.1"
            )
        return contract_section_between(
            document, "## 2. Decisions", "## 3. Acceptance proposals"
        )
    if "# Reconciled dispatch DAG" in document:
        return contract_section_between(
            document, "## Registry contract", "## Canonical node table"
        )
    if "# Union catalog" in document and label == "O-CFRATE evidence":
        return document[: document.index("# Union catalog")]
    return ""


def validate_canary_split_contract(document: str, label: str) -> list[str]:
    """Freeze the default-off implementation/evidence-only two-phase split."""

    normalized = normalized_contract_text(
        canonical_schema_section(document, "canary activation")
    )
    requirements = {
        "T6-W14 exact 0/0 default-off with zero action": (
            r"T6-W14's deterministic default-off phase keeps both.{0,100}"
            r"FABRIC_PROBES_ENABLED.{0,80}SYNTHETIC_SLOT_PROBES_ENABLED.{0,80}exact `?0`?"
            r".{0,120}proves zero outer-route requests.{0,100}lifecycle envelopes.{0,100}"
            r"container fetches.{0,80}starts.{0,100}(?:usage|active minutes)"
        ),
        "T6-W14 receives no live arm/probe credit": (
            r"(?:no activation, re-enable or probe credit.{0,100}T6-W14|"
            r"T6-W14.{0,800}(?:earns|claims?|receives?).{0,40}no.{0,80}(?:live|probe) credit)"
        ),
        "T6-W10 Phase 1 exact 12-count no-wake seal": (
            r"phase[- ]?1.{0,800}(?:exactly )?12.{0,80}lifecycle ticks.{0,120}"
            r"(?:exactly )?12.{0,80}(?:outer-route|passive outer-route) requests.{0,120}"
            r"(?:exactly )?12.{0,80}(?:durably )?acknowledged lifecycle envelopes.{0,180}"
            r"(?:zero|0).{0,80}(?:container(?:-proxy)? )?(?:fetch|fetches).{0,120}"
            r"(?:start|starts).{0,120}(?:usage|active minutes)"
        ),
        "T6-W10 Phase 2 exact 20 transactions": (
            r"phase[- ]?2.{0,900}(?:exactly )?20.{0,120}transactions"
        ),
        "Phase 1 seal precedes Phase 2": (
            r"only after.{0,80}phase[- ]?1.{0,100}sealed.{0,100}phase[- ]?2"
        ),
        "Phase 2 cannot contaminate Phase 1 artifact": (
            r"phase[- ]?2.{0,900}excluded.{0,180}cannot.{0,100}"
            r"(?:amend|rerun|falsify)"
        ),
        "T6-W10 evidence-only no implementation/double ownership": (
            r"T6-W10 implements no.{0,150}driver.{0,100}detector.{0,160}credential."
            r"{0,180}monitor.{0,700}(?:does not double-own|no second A6\.22 ownership|"
            r"does not make it an A6\.22 owner|not make it an A6\.22 owner)"
        ),
        "Phase 2 changes only synthetic flag and preserves both lane identities": (
            r"phase[- ]?2.{0,900}(?:change|changes).{0,100}only.{0,100}"
            r"synthetic_flag_value.{0,350}(?:preserv|keep|remain).{0,160}"
            r"(?:both|lifecycle.{0,60}synthetic).{0,100}identit"
        ),
    }
    return [
        f"{label} canary split omits {requirement}"
        for requirement, pattern in requirements.items()
        if re.search(pattern, normalized, re.IGNORECASE) is None
    ]


def validate_cf_rate_cross_document(document: str, label: str) -> list[str]:
    """Require provider-issued, predeclared and half-open O-CFRATE evidence."""

    normalized = normalized_contract_text(document)
    prose = normalized.replace(plain_markdown(O_CFRATE_EVIDENCE), "")
    requirements = {
        "threshold declaration precedes observation": (
            r"threshold_declared_at.{0,140}(?:<|before).{0,80}budget_interval_start"
        ),
        "threshold policy receipt binds the declaration": (
            r"threshold_policy_digest.{0,260}threshold_receipt_id.{0,120}"
            r"threshold_receipt_sha256"
        ),
        "provider-issued invoice/usage source": (
            r"provider-issued (?:invoice|usage export|invoice or usage export)"
        ),
        "half-open budget interval": (
            r"(?:half-open|inclusive-start/exclusive-end|"
            r"\[budget_interval_start,budget_interval_end\))"
        ),
    }
    errors = [
        f"{label} O-CFRATE omits {requirement}"
        for requirement, pattern in requirements.items()
        if re.search(pattern, prose, re.IGNORECASE) is None
    ]
    if re.search(
        r"\bclosed interval(?:\s|>)*`?\[budget_interval_start",
        prose,
        re.IGNORECASE,
    ):
        errors.append(f"{label} O-CFRATE mislabels its half-open interval as closed")
    return errors


def validate_handoff_contract(handoff_path: Path) -> list[str]:
    """Keep handoff schemas and canary sequencing aligned with normative inputs."""

    errors: list[str] = []
    try:
        visible, _ = markdown_visible_text(handoff_path.read_text(encoding="utf-8"))
    except OSError as exc:
        return [f"cannot read handoff contract {handoff_path}: {exc}"]
    checkpoint = contract_section_between(
        visible, "## Compaction checkpoint", "## 1. Production containment"
    )
    compact = re.sub(r"\s+", "", visible)
    compact_checkpoint = re.sub(r"\s+", "", checkpoint)
    for label, literal in (
        ("page ACK", PAGE_ACK_SCHEMA),
        ("ACK_RECOVERY", ACK_RECOVERY_SCHEMA),
    ):
        compact_literal = re.sub(r"\s+", "", literal)
        section_count = compact_checkpoint.count(compact_literal)
        total_count = compact.count(compact_literal)
        if section_count != 1 or total_count != 1:
            errors.append(
                f"{label} exact schema must occur once in the handoff checkpoint "
                f"and once visibly, got section={section_count}, total={total_count}"
            )
    normalized = normalized_contract_text(visible)
    flow_section = contract_section_between(
        visible,
        "## 5. Provenance and future DAG",
        "## 6. Rules and operational traps",
    )
    exact_canary_chain = "`T6-W13 → T6-W14 → T6-W10`"
    if (
        flow_section.count(exact_canary_chain) != 1
        or visible.count(exact_canary_chain) != 1
    ):
        errors.append(
            "handoff must contain the exact T6-W13 -> T6-W14 -> T6-W10 "
            "canary chain once in its provenance section"
        )
    if re.search(
        r"later no-wake re-enable is.{0,40}T6-W13.{0,20}T6-W14",
        normalized,
        re.IGNORECASE,
    ):
        errors.append(
            "handoff falsely assigns no-wake re-enable to T6-W14 instead of "
            "T6-W10's evidence-only phase"
        )
    if (
        re.search(r"T6-W13.{0,100}T6-W14.{0,100}T6-W10", normalized, re.IGNORECASE)
        is None
    ):
        errors.append(
            "handoff must preserve the T6-W13 -> T6-W14 -> T6-W10 canary chain"
        )
    return errors


def validate_cf_rate_contract(dag_document: str) -> list[str]:
    """Freeze the human-owner Cloudflare rate obstacle and its read-only consumer."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "`O-CFRATE` is an owner-of-record arming token"
    end_marker = "`T7-W3` is the evidence-schema gate"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered O-CFRATE contract "
            f"(starts={len(starts)}, ends={len(ends)})"
        ]
    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "exact ordered evidence schema": re.escape(O_CFRATE_EVIDENCE),
        "human owner/Billing Administrator": (
            r"\bhuman Cloudflare account owner or an authorized Cloudflare Billing "
            r"Administrator\b"
        ),
        "manual console/export/sign action": (
            r"\bmanual action is to open.+provider billing console.+export and preserve "
            r"the provider invoice/usage receipt byte-for-byte, and sign\b"
        ),
        "canonical artifact": re.escape(
            "docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json"
        ),
        "named invoice line": r"\bone named Cloudflare Containers invoice line\b",
        "account period currency SKU unit": (
            r"\baccount/billing-period identifier, currency, exact provider SKU and unit\b"
        ),
        "quantity amount rate formula": (
            r"\bbilled quantity and amount, resulting per-unit rate\b"
        ),
        "receipt and signature provenance": (
            r"\bsource-receipt digest, capture time and owner signature\b"
        ),
        "credits/tax and effective dates": (
            r"\beffective dates, credits/discounts/tax treatment and explicit rate formula\b"
        ),
        "contiguous bounded budget interval": (
            r"\bbudget interval is one contiguous inclusive-start/exclusive-end interval "
            r"wholly covered by the billing/export evidence\b"
        ),
        "predeclared thresholds": r"\bthresholds are predeclared before observation\b",
        "failure numerator formula": (
            r"\bcanonical failure-rate numerator formula is "
            r"failed_attempt_count \+ retry_count \+ idle_wakeup_count\b"
        ),
        "failure denominator formula": (
            r"\bits denominator formula is attempt_count \+ retry_count \+ "
            r"idle_wakeup_count\b"
        ),
        "zero failure denominator RED": r"\bfailure_rate_denominator=0 is RED\b",
        "failure threshold comparison": (
            r"\bremain at or below failure_rate_threshold\b"
        ),
        "cost budget comparison": r"\bremain at or below cost_budget\b",
        "cost-per-served formula": (
            r"\bcost_per_served_attempt=observed_cost/served_count\b"
        ),
        "zero served RED": r"\bserved_count=0 is RED\b",
        "cost-per-served threshold comparison": (
            r"\bremain at or below its predeclared threshold\b"
        ),
        "exactly-once complete accounting": (
            r"\bevery retry, idle wakeup, failed attempt, served attempt, billable "
            r"vCPU-hour, billable GiB-hour and cost unit in the interval is included "
            r"exactly once\b"
        ),
        "missing accounting cannot pass": (
            r"\bmissing or unjoined accounting cannot PASS\b"
        ),
        "no estimate substitution": (
            r"\bpublic list price, calculator, proxy-provider price, dashboard estimate "
            r"or unsigned transcription does not resolve it\b"
        ),
        "single read-only consumer": (
            r"\bonly T7-W5 consumes this token, reads but does not rewrite its artifact\b"
        ),
        "no re-enable or dispatch authority": (
            r"\bauthorizes no provider mutation, Cloudflare re-enable, proof credit or dispatch\b"
        ),
    }
    return [
        f"O-CFRATE contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_final_monitor_deploy_contract(dag_document: str) -> list[str]:
    """Freeze the final-monitor reproof that must precede PG rearm.

    T6-W12 is allowed to reopen base implementation seams, but it does not own
    T6-W15's tests.  The canonical DAG must therefore state an execution and
    evidence obligation in visible prose: run the unchanged complete base
    suite against the candidate and again against the active post-cutover
    deployment, bind both precise tuples and rollback result, fail closed, and
    only then allow T1-W6 to rearm.
    """

    errors: list[str] = []
    visible, _ = markdown_visible_text(dag_document)
    start_marker = "`T6-W12` is the serialized pre-rearm follow-on"
    end_marker = "`T6-W14` later"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered T6-W12 final-monitor "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "serialized pre-rearm placement": r"\bserialized pre-rearm follow-on\b",
        "final external deployment": (
            r"\bperforms the final external-[ \t]*service deployment\b"
        ),
        "PG-disabled finalization": r"\bwith FABRIC_PG_DISABLED=1\b",
        "pre-cutover candidate execution": (
            r"\bT6-W12 must first execute every unchanged T6-W15 test against its "
            r"candidate image before cutover\b"
        ),
        "candidate-pass cutover gate": (
            r"\ba candidate PASS permits an atomic cutover\b"
        ),
        "post-cutover active-final rerun": (
            r"\bT6-W12 must then rerun the entire unchanged T6-W15 suite against "
            r"the active final deployed tuple before PG rearm\b"
        ),
        "both complete executions": r"\brecords both complete executions\b",
        "canonical monitor-rearm tuple": re.escape(MONITOR_REARM_TUPLE),
        "complete test-tree binding": r"\bT6-W15 test-tree digest/results\b",
        "previous-version binding": r"\bprevious active version\b",
        "atomic cutover/rollback result": r"\batomic cutover/rollback outcome\b",
        "canonical T6-W12 evidence": re.escape(
            "docs/plan/evidence/T6-W12-independent-monitor.json"
        ),
        "candidate failure blocks cutover": r"\ba candidate failure forbids cutover\b",
        "post-cutover failure rollback": (
            r"\bany post-cutover failure rolls back, keeps PG disabled and forbids "
            r"T6-W12 completion\b"
        ),
        "active-final completion gate": (
            r"\bonly a post-cutover PASS against the active final tuple completes "
            r"T6-W12\b"
        ),
        "execution-only ownership boundary": r"\bexecution obligation only\b",
        "T6-W15 exclusive test ownership": (
            r"\bT6-W15 retains exclusive ownership of every base test file\b"
        ),
        "no T6-W12 test rewrite": (
            r"\bT6-W12 may not copy, weaken or rewrite the suite\b"
        ),
        "base-suite regression fails closed": (
            r"\bany T6-W15-suite regression fail closed\b"
        ),
        "final version before rearm": (
            r"\bonly this final green monitor/provider version may precede T1-W6\b"
        ),
        "last version-bound rearm check": (
            r"\brecords the exact sealed monitor_rearm_tuple digest, verifies all seven "
            r"fields unchanged and performs (?:a fresh version-bound monitor/provider "
            r"poll|the last version-bound monitor/provider-polling check) immediately "
            r"before rearming PG\b"
        ),
        "pre-rearm tuple drift scope": (
            r"\bany tuple-field drift after T6-W12's post-cutover PASS and before rearm "
            r"invalidates both packets and keeps PG disabled\b"
        ),
        "post-rearm tuple drift sequence": (
            r"\bafter rearm, any proposed tuple-field drift must first atomically restore "
            r"FABRIC_PG_DISABLED=1, then repeat T6-W12's candidate/cutover/active-suite "
            r"proof and T1-W6's final poll\b"
        ),
        "tuple drift fails closed": (
            r"\bdrift without that sequence is hard RED and fails closed\b"
        ),
        "T6-W12 sensitivity ownership": (
            r"\bT6-W12 also owns the A6\.17 sensitivity-window implementation\b"
        ),
        "isolated sensitivity scheduler and credential": (
            r"\ban O-MONITORHOST scheduler and credential distinct from the monitor "
            r"application\b"
        ),
        "isolated sensitivity receipt verification": (
            r"\ban external receipt verifier isolated from monitor application "
            r"configuration\b"
        ),
        "non-vacuous sensitivity window": (
            r"\bdisabling or desensitizing the monitored detector/delivery path cannot "
            r"green the seven-day window\b"
        ),
        "T6-W10 evidence-only sensitivity role": (
            r"\bT6-W10 owns only the evidence-only consumption of those seven-day "
            r"sensitivity results\b"
        ),
        "canonical A6.17 window tuple": re.escape(A6_17_WINDOW_TUPLE),
        "A6.17 all-field drift reset": (
            r"\bany constituent drift during the window invalidates all elapsed time and "
            r"restarts a full seven-day window\b"
        ),
        "monitor drift has broader PG scope": (
            r"\bdrift of monitor_rearm_tuple_digest additionally invokes the broader "
            r"PG-disable/reproof rule above\b"
        ),
        "A6.17-only drift scope": (
            r"\bdrift confined to the other six A6\.17 fields invalidates only the "
            r"A6\.17 window and does not by itself invalidate the PG-rearm proof\b"
        ),
        "unchanged A6.17 digest consumption": (
            r"\bT6-W10 must record and consume one unchanged A6\.17_window_tuple digest "
            r"for the complete window\b"
        ),
        "sensitivity excluded from PG interlock": (
            r"\bsensitivity receipt/health is excluded from the rearm attestation and "
            r"PG latch\b"
        ),
        "sensitivity-only failure scope": (
            r"\ba missing or overdue sensitivity receipt alerts and restarts only the "
            r"A6\.17 window unless an independent tuple, provider-poll or core delivery "
            r"failure separately triggers the interlock\b"
        ),
        "sensitivity-specific freshness bound": (
            r"\bthe sensitivity receipt is overdue only relative to its configured "
            r"cadence of at most six hours, never the rearm attestation's 60-second "
            r"observation bound\b"
        ),
    }
    for label, pattern in requirements.items():
        if re.search(pattern, contract, re.IGNORECASE) is None:
            errors.append(f"T6-W12 final-monitor contract omits required {label}")

    registration_start = "Before T6-W12 seals its final deployed tuple"
    registration_end = "`T6-W4` durably enqueues"
    registration_starts = [
        match.start() for match in re.finditer(re.escape(registration_start), visible)
    ]
    registration_ends = [
        match.start() for match in re.finditer(re.escape(registration_end), visible)
    ]
    if (
        len(registration_starts) != 1
        or len(registration_ends) != 1
        or registration_starts[0] >= registration_ends[0]
    ):
        errors.append(
            "canonical DAG must contain one visible, ordered T6-W12/T1-W6 "
            "pre-registration contract"
        )
        return errors

    registration = plain_markdown(
        visible[registration_starts[0] : registration_ends[0]]
    )
    registration_requirements = {
        "future T1-W6 source ids": (
            r"\bpre-registers the exact future T1-W6 fabric-server and fabricd-proxy "
            r"source ids\b"
        ),
        "isolated write-only credentials": (
            r"\bissues (?:both|four) isolated write-only key-id/credential-[ \t]*epoch "
            r"pairs\b"
        ),
        "credentials bound to both suite runs": r"\binputs to the sealed tuple\b",
        "future inactive T6-W14 source ids": (
            r"\b(?:pre-registers|and) the exact future T6-W14 canary-lifecycle and "
            r"canary-synthetic source ids\b"
        ),
        "stable accepted registrations": (
            r"\ball four registry entries and credential authorizations are accepted "
            r"and byte-stable before both complete T6-W15-suite executions\b"
        ),
        "four pairwise-distinct producer credentials": (
            r"\b(?:issues four isolated write-only key-id/credential-epoch pairs|"
            r"all four (?:key-id/credential-epoch )?pairs are pairwise distinct)\b"
        ),
        "no accepted-bit activation drift": (
            r"\bT6-W14 never changes an accepted/active bit at bind time\b"
        ),
        "emission-only default off": (
            r"\b(?:only the T6-W14 producer emission|both T6-W14 producer lanes) "
            r"remain locally default-off\b"
        ),
        "missing-source clock after first emission": (
            r"\bmonitor missing-source clock starts with its first accepted emitted "
            r"envelope\b"
        ),
        "T1-W6 bind-only boundary": (
            r"\bT1-W6 and T6-W14 may only bind their already-issued pairs\b"
        ),
        "T6-W14 bind-only boundary": (
            r"\bneither may mint, rotate, substitute, register or mutate monitor "
            r"acceptance\b"
        ),
        "no bind-time monitor mutation": (
            r"\bany mismatch stays emission-off and is tuple drift\b"
        ),
    }
    for label, pattern in registration_requirements.items():
        if re.search(pattern, registration, re.IGNORECASE) is None:
            errors.append(f"T6-W12/T1-W6 pre-registration contract omits {label}")
    return errors


def validate_monitor_host_capability_contract(dag_document: str) -> list[str]:
    """Keep O-MONITORHOST capability-only and freeze its isolated hosts."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "`O-MONITORHOST` is a pre-implementation capability token"
    end_marker = "`O-CFINVENTORY` is one atomic obstacle"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered O-MONITORHOST "
            f"capability contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "canonical capability artifact": re.escape(
            "docs/plan/evidence/O-MONITORHOST-external-monitor.json"
        ),
        "external monitor capability domains": (
            r"\bnames a viable runtime, scheduler, durable incident store, "
            r"alert-delivery transport and credential domain that are all outside "
            r"Cloudflare and outside every monitored component\b"
        ),
        "monitor permissions/idempotency/SLO support": (
            r"\bwith documented permissions/idempotency/SLO support\b"
        ),
        "distinct sensitivity scheduler and verifier": (
            r"\ba sensitivity scheduler and an external receipt verifier that are "
            r"distinct from the monitor application and from each other\b"
        ),
        "separate accounts and credentials": (
            r"\bthe artifact names their separate accounts, credential ids\b"
        ),
        "version/config and durable state": (
            r"\bversion/config digests, durable state\b"
        ),
        "delivery-read capability": r"\bdelivery-read permissions\b",
        "bounded control cadence": r"\bcontrol cadence of at most six hours\b",
        "journal retention capability": (
            r"\bappend-only(?:/WORM)? journal (?:storage|capability).+at least "
            r"(?:8|eight) days of retention\b"
        ),
        "journal atomic append/read verification": (
            r"\batomic append, read-after-write verification, immutable record ids\b"
        ),
        "journal independent location and read authority": (
            r"\bjournal capability outside Cloudflare.+export/read permissions for the "
            r"external receipt verifier\b"
        ),
        "mutable storage is not a journal": (
            r"\ba mutable monitor database row or object overwrite is not that capability\b"
        ),
        "independent failure domains": (
            r"\bindependent failure/configuration/control domains\b"
        ),
        "capability-only boundary": (
            r"\bthese are capability-only properties; O-MONITORHOST does not claim "
            r"application behavior, a deployed control, a receipt or (?:a )?delivery "
            r"(?:or journal )?result\b"
        ),
        "T6-W15 integration ownership": (
            r"\bT6-W15 alone owns integration, deployed-version binding and "
            r"crash/timing/kill-path proof in T6-W15-monitor-base\.json\b"
        ),
    }
    return [
        f"O-MONITORHOST contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_monitor_journal_contract(dag_document: str) -> list[str]:
    """Freeze the exhaustive, immutable A6.17 journal and manifest boundary."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "T6-W12 owns and deploys an append-only/WORM journal"
    end_marker = "Before T6-W12 seals its final deployed tuple"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered exhaustive-monitor-journal "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "T6-W12 journal ownership": re.escape(start_marker),
        "minimum eight-day retention": (r"\bretained for at least (?:8|eight) days\b"),
        "exhaustive record classes": (
            r"\brecords every page, page acknowledgement, sensitivity control, rearm "
            r"attestation and ingest ACK\b"
        ),
        "no sampling or mutable replacement": (
            r"\bwithout sampling or mutable replacement\b"
        ),
        "exact runtime-complete A6.17 tuple": (
            r"\bbinds the exact A6\.17_window_tuple\b"
        ),
        "closed interval manifest": (r"\brecords the inclusive start, exclusive end\b"),
        "exhaustive ordered record ids": r"\bexhaustive ordered record ids\b",
        "record count": r"\brecord count\b",
        "initial and terminal chain roots": (
            r"\binitial and terminal hash-chain roots\b"
        ),
        "provider retention receipts": (
            r"\bstorage-provider retention/immutability receipts\b"
        ),
        "manifest-root-only consumption": (
            r"\bT6-W10 consumes only the sealed manifest root and its "
            r"retention/immutability receipts\b"
        ),
        "rewrite mutant": r"\brewrite\b",
        "first/middle/last omission mutants": (
            r"\bomitted first/\s*middle/last record\b"
        ),
        "sequence/time-gap mutants": r"\bsequence/time gap\b",
        "mixed-window and mixed-tuple mutants": (
            r"\bmixed-window/mixed-tuple substitutions\b"
        ),
        "all mutants fail": (
            r"\b(?:requires every mutant to fail|all invalidate the window and restart "
            r"seven days at zero)\b"
        ),
        "no summary or reconstruction substitute": (
            r"\bno copied journal, summary counter, selected receipt set or "
            r"evidence-time reconstruction can substitute for that exhaustive root\b"
        ),
        "candidate and active-final mutant matrix": (
            r"\bT6-W12 runs the complete journal mutant matrix against both the "
            r"candidate and active-final deployments\b"
        ),
        "write-ahead intent before effects": (
            r"\bbefore sending a page, applying a sensitivity control or returning a "
            r"rearm attestation, T6-W12 durably appends the exact immutable intent\b"
        ),
        "stable operation and previous root": (
            r"\bdeterministic operation id and previous hash root\b"
        ),
        "provider result receipt": (
            r"\bafter the external effect it appends the exact provider result/receipt\b"
        ),
        "journal reconciler": re.escape(
            "deploy/cost-monitor/src/journal_reconciler.ts"
        ),
        "same-id provider reconciliation": (
            r"\breading the provider with the same idempotency key\b"
        ),
        "no ambiguous retry or guess": (
            r"\bnever guesses absence or issues a second effect while provider state "
            r"is unavailable or ambiguous\b"
        ),
        "single successor root CAS": r"\bonly one successor may CAS from a journal root\b",
        "bidirectional exhaustiveness": (
            r"\ba provider record without its local intent, a local intent absent from "
            r"the provider after a complete read\b"
        ),
        "fork refuses seal and rearm": r"\bany fork makes sealing and rearm RED\b",
        "write-ahead fixture": re.escape(
            "deploy/cost-monitor/test/window-journal-writeahead.test.ts"
        ),
        "provider-reconciliation fixture": re.escape(
            "deploy/cost-monitor/test/window-journal-reconcile.test.ts"
        ),
        "non-equivocation fixture": re.escape(
            "deploy/cost-monitor/test/window-journal-fork.test.ts"
        ),
    }
    return [
        f"exhaustive monitor journal contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_trusted_time_contract(dag_document: str) -> list[str]:
    """Freeze trusted-clock identity, monotonic watermark and freshness refusal."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "All window, freshness, ACK and receipt times pass through"
    end_marker = "Before T6-W12 seals its final deployed tuple"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered trusted-time contract "
            f"(starts={len(starts)}, ends={len(ends)})"
        ]
    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "trusted clock implementation": re.escape("deploy/cost-monitor/src/clock.ts"),
        "durable monotonic high-water": r"\bpersists a monotonic high-water value\b",
        "trusted commit/watermark/receipt domains": (
            r"\bverifies the monitor's durable ingest-commit checkpoint, provider-"
            r"authenticated monotonic watermark/as_of and immutable delivery/control "
            r"receipt against the named O-MONITORHOST trusted time/checkpoint capability\b"
        ),
        "producer time evidence-only": (
            r"\bproducer, process and scheduler wall times are evidence only\b"
        ),
        "rollback/skew/outage refusal": (
            r"\bcheckpoint rollback, excessive forward skew, unavailable time authority, "
            r"restart below high-water.+or a timestamp outside its contract refuses\b"
        ),
        "no manufactured freshness": (
            r"\bnever manufactures freshness or shortens a deadline\b"
        ),
        "trusted-time fixture": re.escape(
            "deploy/cost-monitor/test/clock-freshness.test.ts"
        ),
        "candidate and active-final proof": (
            r"\bcandidate and active-final runs of deploy/cost-monitor/test/"
            r"clock-freshness\.test\.ts\b"
        ),
        "deterministic time mutants": (
            r"\brollback, forward jump, boundary, restart and time-source outage cases\b"
        ),
    }
    return [
        f"trusted-time contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_rearm_interlock_contract(dag_document: str) -> list[str]:
    """Freeze nonce-bound current-tuple checks and the durable PG latch."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "**Nonce-bound rearm attestation and durable interlock.**"
    end_marker = "T6-W12 also owns the A6.17 sensitivity-window implementation."
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered nonce-bound rearm "
            f"interlock contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "T6-W12 attestation module": re.escape(
            "deploy/cost-monitor/src/rearm_attestation.ts"
        ),
        "current effective tuple derivation": (
            r"\bderives the current effective monitor_rearm_tuple from the running "
            r"deployment\b"
        ),
        "fresh nonce signed fields": (
            r"\bfor each fresh caller nonce it returns a signature over that nonce, "
            r"the exact effective tuple digest and separately timestamped provider-poll "
            r"and delivery-route health\b"
        ),
        "attestation response freshness": (
            r"\bthe challenge response age is at most 10 seconds\b"
        ),
        "provider/delivery freshness": (
            r"\bprovider-poll and delivery health observations are at most 60 seconds "
            r"old\b"
        ),
        "shared attestation/ACK signer registry": (
            r"\bthe seventh tuple field commits the exact accepted "
            r"\(signer_key_id,signer_epoch\) registry, trust-anchor digests and "
            r"revocation state for both rearm attestations and authenticated ingest ACKs\b"
        ),
        "wrong-valid signer refusal": (
            r"\ba cryptographically valid signature from a signer/epoch not in that "
            r"digest is invalid\b"
        ),
        "signer input drift": (r"\bchanging any of those inputs is tuple drift\b"),
        "T1-W6 interlock module": re.escape(
            "crates/corelink-fabric-server/src/monitor_interlock.rs"
        ),
        "fresh challenge before every PG surface": (
            r"\bobtains a new response before every readiness answer, PG-backed "
            r"mutation, and PG/exporter socket/init/pool use\b"
        ),
        "attestation verification": (
            r"\bverifies the bound tuple digest, nonce echo, signature and each "
            r"applicable freshness rule\b"
        ),
        "no cached/replayed success": (
            r"\ba cached success, nonce replay or earlier valid response is never "
            r"reusable\b"
        ),
        "atomic durable latch triggers": (
            r"\bany tuple mismatch, unavailable attestation, stale provider-poll or "
            r"delivery health, bad signature or nonce failure first atomically arms a "
            r"durable non-PG disable latch\b"
        ),
        "generation-scoped permit on every surface": (
            r"\bevery readiness, mutation and socket/init path first obtains a "
            r"generation-scoped coordinator permit\b"
        ),
        "shared transaction fence through commit": (
            r"\bevery PG transaction additionally holds the same generation's shared "
            r"transaction-scoped PostgreSQL advisory fence and validates the durable "
            r"fence-row generation inside that transaction\b"
        ),
        "server authority, not app check": (
            r"\ban application-side check alone is never authority\b"
        ),
        "exact fence state machine": r"\bOPEN -> FENCING -> CLOSING -> LATCHED\b",
        "exclusive fence drains prior transactions": (
            r"\bobtains the exclusive transaction-scoped fence lock, waits for every "
            r"earlier shared transaction to commit or roll back\b"
        ),
        "durable generation before CLOSING": (
            r"\batomically advances the durable PG generation/latch and commits, and "
            r"only then publishes CLOSING\b"
        ),
        "transaction order around linearization": (
            r"\ba transaction ordered before the exclusive lock may commit only before "
            r"CLOSING is published; one ordered after it sees the new generation and "
            r"aborts before mutation\b"
        ),
        "ambiguous fence fail-closed reconciliation": (
            r"\ban unavailable or ambiguous exclusive-lock/update outcome never "
            r"publishes CLOSING or LATCHED.+keeps new work refused, alerts, and "
            r"reconciles the durable fence row\b"
        ),
        "PG fence implementation paths": (
            re.escape("crates/corelink-fabric/src/pg_monitor_fence.rs")
            + r".+"
            + re.escape(
                "crates/corelink-fabric-server/src/monitor_transaction_fence.rs"
            )
        ),
        "PG fence test paths": (
            re.escape("crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs")
            + r".+"
            + re.escape(
                "crates/corelink-fabric-server/tests/monitor_transaction_fence.rs"
            )
        ),
        "two-instance transaction pause matrix": (
            r"\bpause two independent instances before lock, after the shared lock and "
            r"immediately before commit\b"
        ),
        "LATCHED only after complete accounting": (
            r"\bonly after every action/socket is durably accounted for may arming return\b"
        ),
        "latched 503/zero-action behavior": (
            r"\breadiness/mutation are typed 503 and zero socket or mutation actions "
            r"survive the fence\b"
        ),
        "planned-change latch and drain": (
            r"\ba planned monitor_rearm_tuple change must enter CLOSING and complete "
            r"the permit/action/socket drain and pool discard before the change begins\b"
        ),
        "T1-W6 interlock test": re.escape(
            "crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs"
        ),
        "seven-field and attestation negatives": (
            r"\bindependently mutates all seven tuple fields and injects unavailable, "
            r"missing, stale, bad-signature, wrong-nonce, replayed and cached "
            r"attestations\b"
        ),
        "T6-W12 attestation test": re.escape(
            "deploy/cost-monitor/test/rearm-tuple-attestation.test.ts"
        ),
        "effective tuple and two health classes": (
            r"\bproves the signed effective tuple,? (?:and )?the two correctly bounded health "
            r"classes\b"
        ),
        "attestation signer rejection matrix": (
            r"\brefusal of a wrong-but-valid signer, stale signer epoch and revoked "
            r"signer under the seventh field\b"
        ),
        "negative-case observable result": (
            r"\bevery negative case is green only when the latch is durable, "
            r"pools/sockets are gone, typed 503 is returned and zero action occurs\b"
        ),
        "manual-reset-only recovery": (
            r"\bthe latch may clear only after complete T6-W12 "
            r"candidate/cutover/active-final reproof, exact T1-W6 rebinding to the new "
            r"tuple and an explicit manual reset; none alone restores PG\b"
        ),
        "pre-commit and in-commit pause fixtures": (
            r"\bmonitor_tuple_interlock_race\.rs pauses each action immediately before "
            r"and during durable commit\b"
        ),
        "CLOSING rejects new generations": (r"\bCLOSING admits no new generation\b"),
        "old work drained before LATCHED": (
            r"\bevery old permit/action/socket is cancelled, rolled back or closed "
            r"before LATCHED\b"
        ),
        "no side effects across restart": (
            r"\bzero post-latch side effects before and after restart\b"
        ),
    }
    return [
        f"nonce-bound rearm interlock omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_signed_ack_contract(dag_document: str) -> list[str]:
    """Freeze the one post-CAS signed ACK and every producer's action gate."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "Every producer consumes the same signed ACK token"
    end_marker = "`T3-W16` directly waits for T6-W15"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered signed-durable-ACK "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "exact ordered ACK schema": re.escape(SIGNED_ACK_SCHEMA),
        "signature covers preceding ordered fields": (
            r"\bsignature authenticates the preceding fourteen fields in that order\b"
        ),
        "post-CAS emission": (
            r"\b(?:the )?token (?:is )?emitted (?:by T6-W15 )?only after the matching "
            r"(?:ingest )?CAS commit(?:s)?\b"
        ),
        "byte-identical stable duplicate": (
            r"\ba byte-identical duplicate returns the byte-identical stable token\b"
        ),
        "2xx and unsigned refusal": (
            r"\ban arbitrary HTTP 2xx, (?:an )?unsigned body or (?:a )?newly minted duplicate "
            r"response is not an ACK\b"
        ),
        "verification before gated action": (
            r"\bbefore any (?:producer performs the )?gated next action, (?:that |the )"
            r"producer verifies the signature and frozen fields against its durable head\b"
        ),
        "old/wrong event refusal": r"\brejects .+old or wrong event\b",
        "wrong ACK-version refusal": r"\brejects a wrong ACK version\b",
        "body/payload refusal": r"\b(?:body/)?payload digest\b",
        "sequence refusal": r"\bsequence\b",
        "source refusal": r"\bsource\b",
        "service/application refusal": r"\bservice/application\b",
        "credential-epoch refusal": r"\b(?:key id or )?credential epoch\b",
        "monitor-tuple refusal": r"\bmonitor-tuple digest\b",
        "ingest-commit/time refusal": r"\bingest commit or commit time\b",
        "stale/revoked/wrong-valid signer refusal": (
            r"\brejects a stale, revoked or wrong-but-currently-valid signer under "
            r"the seventh (?:monitor_rearm_tuple|tuple) field\b"
        ),
        "monitor-side fixture boundary": (
            r"\bin both candidate and active-final passes, T6-W12's monitor-side "
            r"fixtures submit isolated exact authenticated envelopes under every "
            r"accepted lane and prove only ingest/CAS/stable-token behavior\b"
        ),
        "no future producer credit": (
            r"\bnever execute, activate or claim a producer fixture\b"
        ),
        "producer refusal/recovery owners": (
            r"\bT6-W4, T3-W16, T1-W6 and T6-W14 each own their producer-side "
            r"refusal/recovery suite\b"
        ),
        "post-bind pre-action proof": (
            r"\bT1-W6/T6-W14 must pass it after bind but before their first gated "
            r"socket, fetch, start, acquire, spawn, release or successor emission\b"
        ),
        "no producer-test dependency cycle": (
            r"\bthis split removes any producer-test dependency from T6-W12 back to "
            r"consumers that follow it\b"
        ),
        "zero gated action on refusal": r"\bperforms zero gated action and fails closed\b",
    }
    return [
        f"signed durable ACK contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_page_ack_and_recovery_contract(dag_document: str) -> list[str]:
    """Freeze authenticated human ACKs and signer-rotation ACK recovery."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "An on-call page is acknowledged only by the exact signed"
    end_marker = "`T3-W16` directly waits for T6-W15"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered human-ACK/recovery "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]
    contract = plain_markdown(visible[starts[0] : ends[0]])
    required_literals = {
        "exact page ACK schema": PAGE_ACK_SCHEMA,
        "page ACK implementation": "deploy/cost-monitor/src/page_ack.ts",
        "page ACK test": "deploy/cost-monitor/test/page-ack-auth.test.ts",
        "exact signer rotation manifest": SIGNER_ROTATION_MANIFEST,
        "exact ACK_RECOVERY schema": ACK_RECOVERY_SCHEMA,
        "recovery implementation": "deploy/cost-monitor/src/ack_recovery.ts",
        "recovery server test": "deploy/cost-monitor/test/ack-recovery.test.ts",
        "tick recovery owner": (
            "deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts"
        ),
        "attempt recovery owner": (
            "deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts"
        ),
        "fabric server recovery owner": (
            "crates/corelink-fabric-server/tests/monitor_ack_recovery.rs"
        ),
        "fabricd recovery owner": (
            "deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts"
        ),
        "lifecycle synthetic recovery owner": (
            "deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts"
        ),
    }
    errors = [
        f"human page ACK / ACK_RECOVERY contract omits required {label}"
        for label, literal in required_literals.items()
        if literal not in contract
    ]
    requirements = {
        "page ACK signature coverage": (
            r"\bsignature authenticates the preceding fourteen fields in that order\b"
        ),
        "current signer trust": (
            r"\bverifies the signer/epoch against current trust/revocation\b"
        ),
        "journal and tuple binding": (
            r"\bjoins the page/delivery to its immutable journal record and monitor tuple\b"
        ),
        "human destination authorization": (
            r"\bproves destination, on-call identity and schedule digest were authorized "
            r"for that exact action and payload\b"
        ),
        "effective tuple binding": (
            r"\btoken's tuple digest must equal the effective monitor_rearm_tuple\b"
        ),
        "trusted expiry bound": (
            r"\btrusted acknowledgement time must be no later than expires_at\b"
        ),
        "page ACK refusal matrix": (
            r"\barbitrary HTTP 2xx, provider delivery receipt, unsigned/manual state "
            r"change, replay from another page/incident, stale schedule, wrong "
            r"action/payload/tuple/destination/identity, expired token or revoked/"
            r"wrong-valid signer cannot acknowledge, "
            r"suppress escalation or start recovery\b"
        ),
        "stale/future/closed refusal": (
            r"\ba stale/future ACK or ACK after close is invalid\b"
        ),
        "duplicate ACK idempotency": (
            r"\bbyte-identical duplicate is idempotently journaled once and neither "
            r"resets the escalation deadline nor erases a later update\b"
        ),
        "journal before incident advance": (
            r"\bvalid token is journaled before incident state advances\b"
        ),
        "manifest signature coverage": (
            r"\bsignature authenticates the preceding twelve fields in that order\b"
        ),
        "manifest monotonic hash chain": (
            r"\bmanifests form one monotonic hash-linked sequence through "
            r"previous_manifest_digest\b"
        ),
        "manifest presealed authority": (
            r"\bactive/next epochs, bounded overlap, complete revoked set, recovery "
            r"custody and exact tuple are presealed before rotation\b"
        ),
        "manifest RED matrix": (
            r"\brollback, fork, missing predecessor, epoch regression, overlap outside "
            r"its bounds, revoked active/next key, wrong tuple, unavailable custody or "
            r"an untrusted manifest signer is RED\b"
        ),
        "canonical manifest digest": (
            r"\bcanonical digest of all thirteen manifest fields is "
            r"signer_rotation_manifest_digest\b"
        ),
        "durable ACK_RECOVERY state": (
            r"\bproducer enters durable ACK_RECOVERY with the byte-identical original "
            r"head, identity and deadline\b"
        ),
        "recovery blocks action/successor": (
            r"\bcannot resample, create a successor or perform the gated action\b"
        ),
        "recovery signature coverage": (
            r"\bsignature authenticates the preceding eighteen fields in that order\b"
        ),
        "no second ingest": (
            r"\bonly from the persisted original ingest CAS and ACK, with no second "
            r"ingest effect\b"
        ),
        "current recovery signer": (
            r"\bseparate token signed by a currently trusted recovery signer\b"
        ),
        "recovery manifest resolution": (
            r"\bmanifest digest must resolve to the unique current hash-linked manifest "
            r"that proves the original signer's revocation, the recovery signer's "
            r"active custody/epoch, the bounded overlap and both the original and "
            r"current tuple binding\b"
        ),
        "recovery manifest RED": (
            r"\bmissing, stale, forked or mismatched manifest is RED\b"
        ),
        "same committed head only": (
            r"\bsatisfies only that committed head's exact original ACK gate and can "
            r"authorize only its one original gated action, never a different or second action\b"
        ),
        "deadline not reset": (
            r"\bif recovery completes after the original 60-second deadline.+the old "
            r"action remains forbidden and a later action requires a new envelope with "
            r"its own clock\b"
        ),
        "ambiguous recovery refusal": (
            r"\bmissing original CAS/ACK, identity mismatch, untrusted signer or "
            r"ambiguous revocation record remains fail-closed\b"
        ),
    }
    errors.extend(
        f"human page ACK / ACK_RECOVERY contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    )
    return errors


def validate_canary_flag_contract(dag_document: str) -> list[str]:
    """Keep lifecycle probing off unless the canary flag is exactly ``1``."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "For staged A6.22"
    end_marker = "## Canonical node table"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered T6-W14 canary-flag "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "exact canary activation tuple": re.escape(CANARY_ACTIVATION_TUPLE),
        "passive edge lifecycle authority": (
            r"\bnon-waking lifecycle endpoint is implemented in the fabricd edge "
            r"Worker and reads only Durable Object lifecycle state; it never calls "
            r"container fetch\b"
        ),
        "no route wake loop": (
            r"\bthe route is passive: it never owns a timer, monitor credential, "
            r"delivery sequence or heartbeat\b"
        ),
        "external missing-source timers": (
            r"\bthe external monitor owns both missing-sample timers, incident state "
            r"and page-delivery outbox, not either Cloudflare producer\b"
        ),
        "exact-one enablement": (
            r"\b(?:the canary performs its outer-route fetch only when |"
            r"FABRIC_PROBES_ENABLED enables lifecycle sampling if and only if )"
            r"FABRIC_PROBES_ENABLED(?:'s value)? is the exact string 1\b"
        ),
        "absent/empty off": (
            r"\bunset, blank, whitespace, case variants, numeric lookalikes and every "
            r"other value also perform zero fetches\b"
        ),
        "malformed and truthy-looking off": (
            r"\bnot silently treated as healthy/off\b"
        ),
        "synthetic exact-one rule": (
            r"\bthe same exact-0/exact-1 and fail-visible rule governs owner arming of "
            r"SYNTHETIC_SLOT_PROBES_ENABLED: only the exact string 1 arms the synthetic "
            r"driver\b"
        ),
        "zero off-state effects": (
            r"\binvalid values perform zero synthetic acquire, spawn or release actions\b"
        ),
        "exact-zero contained state": (
            r"\bexact 0 is the valid contained state and performs zero fabric fetches\b"
        ),
        "fail-visible config module": re.escape(
            "deploy/cloudflare-canary/src/config.ts"
        ),
        "typed config-unavailable readiness": (
            r"\breturns typed config-unavailable readiness\b"
        ),
        "deduplicated invalid-config signal": (
            r"\bdurably emits one deduplicated CANARY_CONFIG_INVALID signal for "
            r"external delivery\b"
        ),
        "fail-visible test": re.escape(
            "deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts"
        ),
        "four-state fail-visible result": (
            r"\bSKIPPED, FAILED, UNKNOWN or SERVED, plus reason, deployed version, "
            r"monitor_rearm_tuple digest and trusted observed_at\b"
        ),
        "exact state classification": (
            r"\bvalid exact-0 is SKIPPED, an authoritative exact-1 response alone may "
            r"be SERVED, an observed negative is FAILED, and missing, invalid or "
            r"unverifiable evidence is UNKNOWN\b"
        ),
        "non-green degraded states": (
            r"\bSKIPPED, FAILED and UNKNOWN never count as green, quiet, rearm or "
            r"re-enable evidence\b"
        ),
        "failure returns to zero": (
            r"\bany failure keeps or returns (?:the flag|both flags) to 0\b"
        ),
        "PG exact-zero enablement": (
            r"\bexact FABRIC_PG_DISABLED=0 is the only value that permits a "
            r"PG/container path\b"
        ),
        "invalid PG values no wake": (
            r"\bevery such non-0 value is disabled at the edge Worker, strips "
            r"DATABASE_URL, opens zero PG/exporter sockets and returns typed 503 before "
            r"any container handle, fetch, start or wake\b"
        ),
        "PG config fail-visible": (
            r"\binvalid values additionally emit the configured deduplicated external "
            r"config signal and cannot masquerade as intentional containment\b"
        ),
        "PG flag test": re.escape(
            "deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts"
        ),
        "idle no-wake test": re.escape(
            "deploy/cloudflare-fabricd/test/idle-no-wake.test.ts"
        ),
        "idle restart proof": (
            r"\bproves cold start, idle sleep, restart, bounded/no retry and "
            r"malformed-config cases have zero scheduled wake, container "
            r"handle/fetch/start, PG/exporter socket and billable active-minute delta\b"
        ),
        "nineteen-field sole activation authority": (
            r"\bthose nineteen ordered fields and their canonical digest are the sole "
            r"activation authority\b"
        ),
        "three-way byte-identical activation": (
            r"\bcanary producer, independent external verifier and rearm decision have "
            r"byte-identical tuple bytes/digest\b"
        ),
        "activation identity/image/config/flag/time binding": (
            r"\blifecycle and synthetic (?:lane )?identities equal the pre-registered "
            r"binds, that image/config equal the running producer, that both named flags "
            r"have their exact intended values, that the monitor tuple is current, and "
            r"that activated_at is trusted and fresh\b"
        ),
        "activation drift fail-closed": (
            r"\bmissing, stale, malformed or contradictory bytes, any field/digest "
            r"drift, post-check config/runtime/key/flag change, or disagreement among "
            r"canary, verifier and rearm atomically disables both lanes, returns both "
            r"flags to exact-0, emits fail-visible UNKNOWN, invalidates prior probe "
            r"credit and requires a new T6-W10 activation proof\b"
        ),
        "T6-W10 exclusive activation ownership": (
            r"\bT6-W10 exclusively owns this later activation plus the 20/20 AU6.17 "
            r"execution\b"
        ),
        "T6-W14 cannot author activation": (
            r"\bT6-W14 remains default-off/bind-only and may neither author nor mutate "
            r"the activation tuple\b"
        ),
    }
    return [
        f"T6-W14 canary-flag contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


def validate_producer_lane_contract(dag_document: str) -> list[str]:
    """Freeze capacity, ACK gating and rotation safety for monitor producers."""

    visible, _ = markdown_visible_text(dag_document)
    start_marker = "`T6-W4` durably enqueues each scheduled tick"
    end_marker = "The worker scope is a total order"
    starts = [match.start() for match in re.finditer(re.escape(start_marker), visible)]
    ends = [match.start() for match in re.finditer(re.escape(end_marker), visible)]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        return [
            "canonical DAG must contain one visible, ordered monitor-producer "
            f"contract (starts={len(starts)}, ends={len(ends)})"
        ]

    contract = plain_markdown(visible[starts[0] : ends[0]])
    requirements = {
        "T6-W4 capacity-one/60-second proof": (
            r"\bT6-W4 durably enqueues each scheduled tick before transmission through "
            r"a Durable Object outbox\b.*\bprove durable capacity one and a total bound "
            r"of at most 60 seconds from enqueue to the external monitor's committed ACK "
            r"or typed terminal\b"
        ),
        "T3-W16 waits for deployed monitor": (
            r"\bT3-W16 directly waits for T6-W15 so its attempt/binding producer can use "
            r"only the deployed external monitor\b"
        ),
        "T3-W16 capacity-one lane": (
            r"\bthat lane has durable capacity one: it may hold at most one nonterminal "
            r"head\b"
        ),
        "single total 60-second interval": (
            r"\bthe total interval from durable enqueue to the external monitor's "
            r"committed ACK or typed terminal is at most 60 seconds\b"
        ),
        "ACK before start or next action": (
            r"\bthe exact head must be externally ACKed before container start or before "
            r"any next immutable lifecycle/cost action\b"
        ),
        "nonterminal head fails closed": (
            r"\bif the head is not terminal within 60 seconds, the packet is hard RED and "
            r"the start/action fails closed\b"
        ),
        "T1-W6 capacity-one producer scopes": (
            r"\bT1-W6 applies the same already-required action gate to its two "
            r"non-interchangeable write-only producer scopes \(fabric-server and "
            r"fabricd-proxy\), each with its own durable capacity-one ordered outbox, "
            r"key id and credential epoch\b"
        ),
        "T6-W14 successor ACK gate": (
            r"\bT6-W14's periodic lifecycle and synthetic lanes are independently "
            r"capacity one and cannot enqueue a successor or perform the successor's "
            r"immutable action until the current head receives its external ACK or typed "
            r"terminal\b"
        ),
        "rotation cannot reset or bypass": (
            r"\bcredential rotation cannot reset the original enqueue clock or bypass "
            r"a head\b"
        ),
        "safe rotation choices": (
            r"\bthe producer must either drain that exact head under its original "
            r"credential or perform a signed epoch migration that preserves its exact "
            r"bytes, source/event/sequence identity, original timestamps and original "
            r"deadline\b"
        ),
        "migration preserves deadline": (
            r"\bmigration never restarts the 60-second bound; exceeding it is hard RED "
            r"and all dependent actions remain fail closed\b"
        ),
    }
    return [
        f"monitor-producer contract omits required {label}"
        for label, pattern in requirements.items()
        if re.search(pattern, contract, re.IGNORECASE) is None
    ]


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
    """Extract path/glob atoms without destroying dots, stars or globstars."""

    fragments: list[str] = []
    for segment in scope.split(";"):
        segment_fragments = re.findall(r"`([^`]+)`", segment)
        carveout = re.search(
            r"(?:^|\s+)(?:excluding|explicitly excludes|minus)\s*:?\s+",
            segment,
            re.IGNORECASE,
        )
        if carveout and segment_fragments:
            if plain_markdown(segment[: carveout.start()]):
                segment_fragments = segment_fragments[:1]
            else:
                segment_fragments = []
        fragments.extend(segment_fragments)
    atoms: list[str] = []
    for fragment in fragments:
        atom = fragment.strip()
        if not atom or atom.startswith("probe:"):
            continue
        if "/" not in atom and not any(char in atom for char in ".*?["):
            continue
        if atom.endswith("/"):
            atom += "**"
        atoms.append(atom)
    return tuple(dict.fromkeys(atoms))


def scope_exclusions(scope: str) -> tuple[str, ...]:
    """Resolve backticked exclusions relative to their preceding broad atom."""

    exclusions: list[str] = []
    previous_atom = ""
    for segment in scope.split(";"):
        fragments = re.findall(r"`([^`]+)`", segment)
        carveout = re.search(
            r"(?:^|\s+)(?:excluding|explicitly excludes|minus)\s*:?\s+",
            segment,
            re.IGNORECASE,
        )
        if carveout is None:
            if fragments:
                previous_atom = fragments[-1].strip()
            continue
        has_inline_base = bool(plain_markdown(segment[: carveout.start()]))
        if has_inline_base and fragments:
            base_atom = fragments[0].strip()
            raw_exclusions = fragments[1:]
            previous_atom = base_atom
        else:
            base_atom = previous_atom
            raw_exclusions = fragments
        if not base_atom:
            continue
        base = re.split(r"[*?[{]", base_atom, maxsplit=1)[0]
        for raw in raw_exclusions:
            atom = raw.strip()
            if not atom:
                continue
            if not atom.startswith(base):
                atom = base + atom.lstrip("/")
            if atom.endswith("/"):
                atom += "**"
            exclusions.append(atom)
        # This canonical carveout is deliberately prose because "canary" is
        # a lane name, not a literal basename beside the broad deploy glob.
        if not raw_exclusions and re.search(
            r"\bexcluding\s*:?\s+canary\b", segment, re.IGNORECASE
        ):
            exclusions.append("deploy/cloudflare-canary/**")
    return tuple(dict.fromkeys(exclusions))


def path_atoms_overlap(left: str, right: str) -> bool:
    """Conservatively detect exact/glob path intersections."""

    left_glob = any(char in left for char in "*?[{")
    right_glob = any(char in right for char in "*?[{")
    if not left_glob and not right_glob:
        return left == right
    if left_glob and not right_glob:
        return glob_matches(left, right)
    if right_glob and not left_glob:
        return glob_matches(right, left)
    if left == right:
        return True
    for witness in glob_witnesses(left) | glob_witnesses(right):
        if glob_matches(left, witness) and glob_matches(right, witness):
            return True
    left_prefix = re.split(r"[*?[{]", left, maxsplit=1)[0]
    right_prefix = re.split(r"[*?[{]", right, maxsplit=1)[0]
    if not (
        left_prefix.startswith(right_prefix) or right_prefix.startswith(left_prefix)
    ):
        return False
    left_suffix = re.split(r"[*?\]}]", left)[-1]
    right_suffix = re.split(r"[*?\]}]", right)[-1]
    return left_suffix.endswith(right_suffix) or right_suffix.endswith(left_suffix)


def glob_matches(pattern: str, path: str) -> bool:
    """Match a repo path with slash-aware ``*`` and recursive ``**`` semantics."""

    expression = ""
    index = 0
    while index < len(pattern):
        character = pattern[index]
        if character == "*":
            if index + 1 < len(pattern) and pattern[index + 1] == "*":
                index += 1
                if index + 1 < len(pattern) and pattern[index + 1] == "/":
                    expression += "(?:.*/)?"
                    index += 1
                else:
                    expression += ".*"
            else:
                expression += "[^/]*"
        elif character == "?":
            expression += "[^/]"
        elif character == "[":
            close = pattern.find("]", index + 1)
            if close < 0:
                expression += re.escape(character)
            else:
                expression += pattern[index : close + 1]
                index = close
        elif character == "{":
            close = pattern.find("}", index + 1)
            if close < 0:
                expression += re.escape(character)
            else:
                choices = pattern[index + 1 : close].split(",")
                expression += "(?:" + "|".join(map(re.escape, choices)) + ")"
                index = close
        else:
            expression += re.escape(character)
        index += 1
    return re.fullmatch(expression, path) is not None


def glob_witnesses(pattern: str) -> set[str]:
    """Create small concrete paths that witness the common repo glob forms."""

    variants = [pattern]
    while any("**" in item for item in variants):
        expanded: list[str] = []
        for item in variants:
            if "**/" in item:
                expanded.extend(
                    (item.replace("**/", "", 1), item.replace("**/", "x/", 1))
                )
            elif "**" in item:
                expanded.extend((item.replace("**", "", 1), item.replace("**", "x", 1)))
            else:
                expanded.append(item)
        variants = expanded
    witnesses: set[str] = set()
    for item in variants:
        item = re.sub(r"\{([^{}]+)\}", lambda match: match.group(1).split(",")[0], item)
        item = re.sub(
            r"\[([^]]+)\]", lambda match: match.group(1).lstrip("!^")[:1] or "x", item
        )
        item = item.replace("*", "x").replace("?", "x")
        witnesses.add(item)
    return witnesses


def path_atom_covers(container: str, candidate: str) -> bool:
    """Return true only when a carveout covers the complete competing atom."""

    if container == candidate:
        return True
    candidate_glob = any(char in candidate for char in "*?[{")
    if not candidate_glob:
        return glob_matches(container, candidate)
    if container.endswith("/**"):
        prefix = container[: -len("**")]
        candidate_prefix = re.split(r"[*?[{]", candidate, maxsplit=1)[0]
        return candidate_prefix.startswith(prefix)
    return False


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

    raw_document = dag_path.read_text(encoding="utf-8")
    errors = hidden_canonical_table_errors(raw_document, "canonical dispatch DAG")
    errors.extend(validate_final_monitor_deploy_contract(raw_document))
    errors.extend(validate_cf_rate_contract(raw_document))
    errors.extend(validate_monitor_host_capability_contract(raw_document))
    errors.extend(validate_monitor_journal_contract(raw_document))
    errors.extend(validate_trusted_time_contract(raw_document))
    errors.extend(validate_rearm_interlock_contract(raw_document))
    errors.extend(validate_signed_ack_contract(raw_document))
    errors.extend(validate_page_ack_and_recovery_contract(raw_document))
    errors.extend(validate_canary_flag_contract(raw_document))
    errors.extend(validate_producer_lane_contract(raw_document))
    batch_rows, batch_errors = exact_ready_set_rows(raw_document)
    errors.extend(batch_errors)
    document, _ = markdown_visible_text(raw_document)
    section = section_between(
        document, "## Canonical node table", "## Deterministic ready sets and proof"
    )
    rows, table_errors = exact_markdown_table(
        section, DAG_HEADER, "canonical dispatch DAG"
    )
    errors.extend(table_errors)
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

    if len(waves) != EXPECTED_DAG_VERTICES:
        errors.append(
            f"DAG must contain exactly {EXPECTED_DAG_VERTICES} WP vertices, got {len(waves)}"
        )

    rendered_batches: list[tuple[str, ...]] = []
    # The rendered ready-set proof is intentionally a code block.  Parse it
    # from raw source only after proving its exact heading and fence boundary;
    # canonical node tables themselves remain visible-Markdown-only.
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
    if "T6-W10" not in predecessors.get("T1-W4", ()):
        errors.append("DAG T1-W4/A1.9 must declare T6-W10 as an exact hard predecessor")
    if "T6-W10" in rendered_nodes and "T1-W4" in rendered_nodes:
        t6_batch = next(
            index for index, batch in enumerate(rendered_batches) if "T6-W10" in batch
        )
        t1_batch = next(
            index for index, batch in enumerate(rendered_batches) if "T1-W4" in batch
        )
        if t6_batch >= t1_batch:
            errors.append(
                "DAG ready sets must serialize T6-W10 before T1-W4/A1.9 in a later batch"
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
        return any(path_atom_covers(item, atom) for item in exclusions.get(owner, ()))

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


def dag_depends_on(
    node: str, target: str, predecessors: dict[str, tuple[str, ...]]
) -> bool:
    """Return whether ``target`` is a direct or transitive DAG predecessor."""

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


def validate_au_dag_routing(
    rows: list[Placement],
    predecessors: dict[str, tuple[str, ...]],
    scopes: dict[str, tuple[str, ...]],
    exclusions: dict[str, tuple[str, ...]],
    staged_probe_wps: set[str],
) -> list[str]:
    """Mechanize declared AU and cross-registry hard routes and scopes.

    Round 10 deliberately orders the final external-monitor deploy before PG
    rearm: T6-W12 consumes the base monitor, then T1-W6 consumes that final
    deploy.  Keep that direction explicit here so a regenerated ready-set
    proof cannot silently restore the old inverse edge.
    """

    errors: list[str] = []
    owners = {row.item: row.owner for row in rows}
    required_routes = {
        "AU5.10": ("O-APP",),
        # The repo half must land before the complete image freshness, deploy
        # and image-ship route.  Checking every named vertex prevents the live
        # proof from silently treating a local entrypoint edit as a deployed
        # image.
        "AU3.26b": ("T8-W4", "T2-W2a", "T2-W2b", "T2-W4"),
        # AU6.17 is live evidence owned by T6-W10, but its synthetic producer
        # and independent external-monitor ingestion are implemented by the
        # later no-wake packet.  Evidence-only ownership must not bypass that
        # implementation route.
        "AU6.17": ("T6-W14", "T6-W15"),
    }
    for item, targets in required_routes.items():
        owner = owners.get(item)
        if owner is None:
            continue
        missing = [
            target
            for target in targets
            if not dag_depends_on(owner, target, predecessors)
        ]
        if missing:
            errors.append(
                f"{item}/{owner} declared prerequisite routing is absent from DAG: {missing}"
            )

    for row in rows:
        if row.kind != "probe":
            continue
        if "T7-W4b" not in predecessors.get(row.owner, ()):
            errors.append(
                f"{row.item}/{row.owner} probe does not route through evidence gate T7-W4b"
            )
    for wp in sorted(staged_probe_wps):
        if "T7-W4b" not in predecessors.get(wp, ()):
            errors.append(
                f"staged test+probe packet {wp} does not route through evidence gate T7-W4b"
            )

    required_direct_predecessors = {
        "T2-W2b": {"T3-W18", "O-FLEETBUSY"},
        "T6-W6": {"T6-W9", "T6-W12"},
        "T4-W2": {"R2"},
        "T4-W7": {"O-BILLING", "T4-W2", "T9-W1"},
        "T4-W8": {"O-BILLING", "T4-W2", "T9-W1"},
        "T6-W15": {"T6-W4", "O-MONITORHOST"},
        "T6-W13": {"T6-W4", "T6-W9", "O-CANARY", "T7-W4b"},
        "T6-W10": {"T6-W6", "T6-W12", "T6-W14"},
        "T5-W4": {"T5-W1"},
        "T8-W7": {"T8-W4", "T2-W2a", "T2-W2b", "T2-W4"},
        "T3-W16": {"T6-W15", "O-CFINVENTORY", "O-CFCANCEL"},
    }
    for wp, required in required_direct_predecessors.items():
        missing = sorted(required - set(predecessors.get(wp, ())))
        if missing:
            errors.append(
                f"DAG node {wp} is missing exact hard predecessor(s): {missing}"
            )

    # These Round-10 rows are intentionally exact, not minimum dependency
    # sets.  In particular, accepting an extra T1-W6 predecessor on T6-W12
    # would restore the unsafe inverse edge even if all new edges remained.
    round10_exact_direct_predecessors = {
        "T1-W5": {"T3-W10", "T3-W18", "O-MINTKEY", "T7-W4b"},
        "T1-W6": {"D12", "T1-W5", "T6-W12", "O-CFINVENTORY", "T7-W4b"},
        "T6-W12": {
            "T6-W15",
            "T6-W9",
            "T3-W16",
            "O-CFINVENTORY",
            "T7-W4b",
        },
        "T6-W14": {
            "T6-W13",
            "T6-W12",
            "T1-W6",
            "O-CANARY",
            "O-MONITORHOST",
            "T7-W4b",
        },
    }
    for wp, expected in round10_exact_direct_predecessors.items():
        actual = set(predecessors.get(wp, ()))
        if actual != expected:
            errors.append(
                f"DAG node {wp} Round-10 exact hard predecessors differ: "
                f"expected={sorted(expected)}, got={sorted(actual)}"
            )

    forbidden_direct_predecessors = {
        # The immediate key lane must not wait on its own later live proof.
        # T6-W6 remains a prerequisite of T6-W10, not T6-W13.
        "T6-W13": {"T6-W6"},
        # The monitor must exist before the attempt producer.  Restoring the
        # inverse route would either admit starts without ACKs or form a cycle.
        "T6-W15": {"T3-W16"},
        # Provider reads and exact-handle cancellation are separate authority
        # domains.  These read-only consumers must not acquire cancellation.
        "T1-W6": {"O-CFCANCEL"},
        # Round 10 reverses the former monitor/rearm edge.  T6-W12 must be
        # deployable and provable before T1-W6 may rearm PG.
        "T6-W12": {"O-CFCANCEL", "T1-W6"},
    }
    for wp, forbidden in forbidden_direct_predecessors.items():
        present = sorted(forbidden & set(predecessors.get(wp, ())))
        if present:
            errors.append(f"DAG node {wp} has forbidden hard predecessor(s): {present}")

    # D7 is the non-waivable key-rotation safety interlock for D3.  Requiring
    # it on each direct D3 consumer keeps a newly added release/publication
    # lane from relying on prose or on another consumer's unrelated route.
    d3_consumers = sorted(wp for wp, deps in predecessors.items() if "D3" in deps)
    for wp in d3_consumers:
        if "D7" not in predecessors.get(wp, ()):
            errors.append(
                f"DAG node {wp} consumes D3 without the mandatory D7 interlock"
            )

    required_transitive_predecessors = {
        "T1-W6": {"T6-W15"},
        "T6-W14": {"T6-W15"},
    }
    for wp, required in required_transitive_predecessors.items():
        missing = sorted(
            target
            for target in required
            if not dag_depends_on(wp, target, predecessors)
        )
        if missing:
            errors.append(
                f"DAG node {wp} is missing transitive hard predecessor(s): {missing}"
            )

    if dag_depends_on("T6-W12", "T1-W6", predecessors):
        errors.append(
            "DAG node T6-W12 follows the forbidden old inverse rearm edge to T1-W6"
        )
    if dag_depends_on("T6-W15", "T3-W16", predecessors):
        errors.append(
            "DAG node T6-W15 follows the forbidden inverse attempt-producer edge "
            "to T3-W16"
        )

    required_scope_atoms = {
        "T6-W4": {
            "deploy/cloudflare-canary/src/config.ts",
            "deploy/cloudflare-canary/src/tick_outbox.ts",
            "deploy/cloudflare-canary/wrangler.jsonc",
            "deploy/cloudflare-canary/package.json",
            "deploy/cloudflare-canary/package-lock.json",
            "deploy/cloudflare-canary/test/scheduled-tick-envelope.test.ts",
            "deploy/cloudflare-canary/test/scheduled-tick-outbox-recovery.test.ts",
            "deploy/cloudflare-canary/test/scheduled-tick-order.test.ts",
            "deploy/cloudflare-canary/test/scheduled-tick-ack.test.ts",
            "deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts",
            "deploy/cloudflare-canary/test/fabric-probe-flag-failclosed.test.ts",
            "deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts",
        },
        "T1-W6": {
            "crates/corelink-fabric/src/pg_monitor_fence.rs",
            "crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs",
            "crates/corelink-fabric-server/src/monitor_outbox.rs",
            "crates/corelink-fabric-server/src/monitor_interlock.rs",
            "crates/corelink-fabric-server/src/monitor_transaction_fence.rs",
            "crates/corelink-fabric-server/tests/monitor_outbox.rs",
            "crates/corelink-fabric-server/tests/monitor_ack.rs",
            "crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs",
            "crates/corelink-fabric-server/tests/monitor_tuple_interlock_race.rs",
            "crates/corelink-fabric-server/tests/monitor_transaction_fence.rs",
            "crates/corelink-fabric-server/tests/monitor_ack_recovery.rs",
            "crates/corelink-fabric/tests/pg_refusal_breaker.rs",
            "deploy/cloudflare-fabricd/src/monitor_outbox.ts",
            "deploy/cloudflare-fabricd/test/monitor-outbox.test.ts",
            "deploy/cloudflare-fabricd/test/monitor-ack.test.ts",
            "deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts",
            "deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts",
            "deploy/cloudflare-fabricd/test/idle-no-wake.test.ts",
            "docs/plan/evidence/T1-W6-pg-durable-live.json",
        },
        "T3-W16": {
            "deploy/cloudflare/test/attempt-monitor-outbox.test.ts",
            "deploy/cloudflare/test/attempt-monitor-ack.test.ts",
            "deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts",
        },
        "T6-W15": {
            "deploy/cost-monitor/Containerfile",
            "deploy/cost-monitor/config.schema.json",
            "deploy/cost-monitor/src/index.ts",
            "deploy/cost-monitor/src/ingest.ts",
            "deploy/cost-monitor/src/acks.ts",
            "deploy/cost-monitor/src/ack_recovery.ts",
            "deploy/cost-monitor/src/page_ack.ts",
            "deploy/cost-monitor/src/incidents.ts",
            "deploy/cost-monitor/src/lifecycle.ts",
            "deploy/cost-monitor/src/scheduler.ts",
            "deploy/cost-monitor/src/state.ts",
            "deploy/cost-monitor/src/delivery.ts",
            "deploy/cost-monitor/src/outbox.ts",
            "deploy/cost-monitor/src/types.ts",
            "deploy/cost-monitor/package.json",
            "deploy/cost-monitor/package-lock.json",
            "deploy/cost-monitor/tsconfig.json",
            "deploy/cost-monitor/vitest.config.ts",
            "deploy/cost-monitor/test/lifecycle-missing.test.ts",
            "deploy/cost-monitor/test/canary-missing-tick.test.ts",
            "deploy/cost-monitor/test/ingest-idempotency.test.ts",
            "deploy/cost-monitor/test/ack-token.test.ts",
            "deploy/cost-monitor/test/ack-recovery.test.ts",
            "deploy/cost-monitor/test/page-ack-auth.test.ts",
            "deploy/cost-monitor/test/incident-state.test.ts",
            "deploy/cost-monitor/test/scheduler.test.ts",
            "deploy/cost-monitor/test/state.test.ts",
            "deploy/cost-monitor/test/delivery.test.ts",
            "deploy/cost-monitor/test/outbox-recovery.test.ts",
            "deploy/cost-monitor/test/outbox-transition-head.test.ts",
            "deploy/cost-monitor/test/outbox-periodic-head.test.ts",
            "deploy/cost-monitor/test/outbox-quarantine.test.ts",
            "deploy/cost-monitor/test/delivery-dedupe.test.ts",
            "deploy/cost-monitor/test/credential-isolation.test.ts",
            "deploy/cost-monitor/test/independence.test.ts",
            "docs/plan/evidence/T6-W15-monitor-base.json",
        },
        "T6-W14": {
            "deploy/cloudflare-fabricd/src/index.ts",
            "deploy/cloudflare-fabricd/src/lifecycle.ts",
            "deploy/cloudflare-fabricd/test/lifecycle-marker.test.ts",
            "deploy/cloudflare-canary/src/index.ts",
            "deploy/cloudflare-canary/src/lifecycle_outbox.ts",
            "deploy/cloudflare-canary/src/synthetic_slot.ts",
            "deploy/cloudflare-canary/src/synthetic_outbox.ts",
            "deploy/cloudflare-canary/src/rules.ts",
            "deploy/cloudflare-canary/src/types.ts",
            "deploy/cloudflare-canary/wrangler.jsonc",
            "deploy/cloudflare-canary/test/no-wake-target.test.ts",
            "deploy/cloudflare-canary/test/lifecycle-monitor-envelope.test.ts",
            "deploy/cloudflare-canary/test/lifecycle-sampler-outbox.test.ts",
            "deploy/cloudflare-canary/test/lifecycle-synthetic-ack.test.ts",
            "deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts",
            "deploy/cloudflare-canary/test/synthetic-slot-lifecycle.test.ts",
            "deploy/cloudflare-canary/test/synthetic-slot-outbox.test.ts",
            "deploy/cloudflare-canary/test/synthetic-slot-default-off.test.ts",
            "deploy/cloudflare-canary/test/synthetic-slot-credential-isolation.test.ts",
            "deploy/cloudflare-canary/test/synthetic-slot-correlation.test.ts",
        },
        "T6-W12": {
            "deploy/cost-monitor/Containerfile",
            "deploy/cost-monitor/config.schema.json",
            "deploy/cost-monitor/package.json",
            "deploy/cost-monitor/package-lock.json",
            "deploy/cost-monitor/src/index.ts",
            "deploy/cost-monitor/src/scheduler.ts",
            "deploy/cost-monitor/src/state.ts",
            "deploy/cost-monitor/src/incidents.ts",
            "deploy/cost-monitor/src/types.ts",
            "deploy/cost-monitor/src/provider.ts",
            "deploy/cost-monitor/src/correlator.ts",
            "deploy/cost-monitor/src/capability_rules.ts",
            "deploy/cost-monitor/src/synthetic_ingest.ts",
            "deploy/cost-monitor/src/sensitivity.ts",
            "deploy/cost-monitor/src/window_journal.ts",
            "deploy/cost-monitor/src/journal_reconciler.ts",
            "deploy/cost-monitor/src/clock.ts",
            "deploy/cost-monitor/src/rearm_attestation.ts",
            "deploy/cost-monitor/migrations/0002-provider-cursors.json",
            "deploy/cost-monitor/migrations/0003-window-journal.json",
            "deploy/cost-monitor/test/provider.test.ts",
            "deploy/cost-monitor/test/provider-stale-frozen.test.ts",
            "deploy/cost-monitor/test/correlator.test.ts",
            "deploy/cost-monitor/test/cursor-crash.test.ts",
            "deploy/cost-monitor/test/provider-unavailable.test.ts",
            "deploy/cost-monitor/test/incident-boundary.test.ts",
            "deploy/cost-monitor/test/acked-incident-update.test.ts",
            "deploy/cost-monitor/test/recovery-horizon.test.ts",
            "deploy/cost-monitor/test/c1-c5-rules.test.ts",
            "deploy/cost-monitor/test/c1-c5-synthetic-ingest.test.ts",
            "deploy/cost-monitor/test/sensitivity-window.test.ts",
            "deploy/cost-monitor/test/window-journal.test.ts",
            "deploy/cost-monitor/test/window-journal-writeahead.test.ts",
            "deploy/cost-monitor/test/window-journal-reconcile.test.ts",
            "deploy/cost-monitor/test/window-journal-fork.test.ts",
            "deploy/cost-monitor/test/clock-freshness.test.ts",
            "deploy/cost-monitor/test/rearm-tuple-attestation.test.ts",
            "docs/plan/evidence/T6-W12-independent-monitor.json",
        },
        "T6-W10": {
            "docs/plan/evidence/T6-W10-alerting-depth.json",
            "docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json",
            "docs/plan/evidence/T6-W14-canary-no-wake.json",
        },
    }
    for wp, required in required_scope_atoms.items():
        missing = sorted(required - set(scopes.get(wp, ())))
        if missing:
            errors.append(f"DAG node {wp} is missing exact scope atom(s): {missing}")
        excluded = {
            atom: sorted(
                exclusion
                for exclusion in exclusions.get(wp, ())
                if path_atom_covers(exclusion, atom)
            )
            for atom in sorted(required & set(scopes.get(wp, ())))
        }
        excluded = {
            atom: carveouts for atom, carveouts in excluded.items() if carveouts
        }
        if excluded:
            errors.append(
                f"DAG node {wp} excludes required atom(s) from effective scope: "
                f"{excluded}"
            )

    t6_w10_implementation = sorted(
        atom
        for atom in scopes.get("T6-W10", ())
        if atom.startswith("deploy/")
        or atom.startswith("crates/")
        or atom.startswith("scripts/")
    )
    if t6_w10_implementation:
        errors.append(
            "DAG node T6-W10 must remain evidence/arming-only after T6-W14's "
            f"default-off implementation, got implementation atoms: {t6_w10_implementation}"
        )
    no_wake_artifact = "docs/plan/evidence/T6-W14-canary-no-wake.json"
    artifact_owners = sorted(
        wp for wp, atoms in scopes.items() if no_wake_artifact in atoms
    )
    if artifact_owners != ["T6-W10"]:
        errors.append(
            "A6.22 no-wake artifact must be produced/sealed only by T6-W10's "
            f"evidence contribution, got scope owners {artifact_owners}"
        )
    return errors


def acceptance_ids(plan_path: Path) -> tuple[set[str], list[str], list[str]]:
    document, _ = markdown_visible_text(plan_path.read_text(encoding="utf-8"))
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
    delta_path: Path | None = None,
    handoff_path: Path | None = None,
) -> tuple[bool, list[str], dict[str, object]]:
    failures: list[str] = []
    raw_document = triage_path.read_text(encoding="utf-8")
    failures.extend(hidden_canonical_table_errors(raw_document, "AU triage"))
    document, _ = markdown_visible_text(raw_document)
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
    placement_bucket_mismatches = {
        row.source: {
            "expected": sorted(EXPECTED_PLACEMENT_BUCKETS.get(row.source, ())),
            "got": sorted(row.buckets),
        }
        for row in rows
        if row.buckets != EXPECTED_PLACEMENT_BUCKETS.get(row.source, frozenset())
    }
    if placement_bucket_mismatches:
        failures.append(
            "placement buckets disagree with frozen summary/source mapping: "
            f"{placement_bucket_mismatches}"
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

    if delta_path is None:
        delta_path = plan_path.with_name("2026-09-01-round3-remediation-delta.md")
    cross_document_contracts = {
        "O-CFRATE evidence": (
            O_CFRATE_EVIDENCE,
            (triage_path, plan_path, delta_path, dag_path),
        ),
        "page ACK": (PAGE_ACK_SCHEMA, (plan_path, delta_path, dag_path)),
        "signer rotation manifest": (
            SIGNER_ROTATION_MANIFEST,
            (plan_path, delta_path, dag_path),
        ),
        "ACK_RECOVERY": (ACK_RECOVERY_SCHEMA, (plan_path, delta_path, dag_path)),
        "canary activation": (
            CANARY_ACTIVATION_TUPLE,
            (plan_path, delta_path, dag_path),
        ),
    }
    cached_contract_documents: dict[Path, str] = {}
    for label, (literal, contract_paths) in cross_document_contracts.items():
        for contract_path in contract_paths:
            if contract_path not in cached_contract_documents:
                try:
                    visible, _ = markdown_visible_text(
                        contract_path.read_text(encoding="utf-8")
                    )
                except OSError as exc:
                    failures.append(
                        f"cannot read contract document {contract_path}: {exc}"
                    )
                    cached_contract_documents[contract_path] = ""
                else:
                    cached_contract_documents[contract_path] = visible
            literal_count = cached_contract_documents[contract_path].count(literal)
            section_count = canonical_schema_section(
                cached_contract_documents[contract_path], label
            ).count(literal)
            if literal_count != 1 or section_count != 1:
                failures.append(
                    f"{label} exact schema must occur once in its canonical section "
                    f"and once visibly in {contract_path.name}, got "
                    f"section={section_count}, total={literal_count}"
                )
    for role, contract_path in (
        ("main plan", plan_path),
        ("round-3 delta", delta_path),
        ("canonical DAG", dag_path),
    ):
        failures.extend(
            validate_canary_split_contract(
                cached_contract_documents.get(contract_path, ""), role
            )
        )
    for contract_path in (plan_path, delta_path, dag_path, triage_path):
        failures.extend(
            validate_cf_rate_cross_document(
                cached_contract_documents.get(contract_path, ""), contract_path.name
            )
        )
    if handoff_path is None:
        handoff_path = (
            Path(__file__).parent.parent
            / "handoff"
            / "2026-09-01-session-state-go-live-remediation.md"
        )
    failures.extend(validate_handoff_contract(handoff_path))
    try:
        delta_document = delta_path.read_text(encoding="utf-8")
        failures.extend(validate_provider_credential_contract(delta_document))
        failures.extend(validate_delta_canary_activation_authority(delta_document))
        (
            staged_wps,
            staged_existing_wps,
            staged_probe_wps,
            staged_errors,
        ) = read_staged_wp_registry(delta_path)
        failures.extend(staged_errors)
    except (OSError, ValueError) as exc:
        failures.append(f"cannot read staged principal WP registry: {exc}")
        staged_wps, staged_existing_wps, staged_probe_wps = set(), set(), set()

    main_names = set(main_items)
    proposal_names = set(declarations)
    # After applying the three coordinated renames, proposal names must be
    # disjoint from principal names.  Do not merge proposal items into the
    # principal catalog: they are not suite items until a later delta lands.
    collisions = sorted(proposal_names & main_names)
    if collisions:
        failures.append(f"proposal/principal WP name collisions: {collisions}")

    bad_staged_existing = sorted(staged_existing_wps - main_names)
    if bad_staged_existing:
        failures.append(
            "staged registry labels non-principal WPs as existing: "
            f"{bad_staged_existing}"
        )
    staged_collisions = sorted(staged_wps & (main_names | proposal_names))
    if staged_collisions:
        failures.append(
            f"staged/principal/AU new-WP registry collisions: {staged_collisions}"
        )

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

    plan_document, _ = markdown_visible_text(plan_path.read_text(encoding="utf-8"))
    authoritative_wps = main_names | staged_wps | proposal_names
    dag_vertex_missing = sorted(authoritative_wps - set(dag_waves))
    dag_vertex_phantoms = sorted(set(dag_waves) - authoritative_wps)
    if dag_vertex_missing or dag_vertex_phantoms:
        failures.append(
            "DAG vertex set differs from exhaustive principal/staged/AU registries: "
            f"missing={dag_vertex_missing}, phantom={dag_vertex_phantoms}"
        )
    known_wps = authoritative_wps
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
    failures.extend(
        validate_au_dag_routing(
            rows,
            dag_predecessors,
            dag_scopes,
            dag_exclusions,
            staged_probe_wps,
        )
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
    parser.add_argument(
        "--delta",
        type=Path,
        default=Path(__file__).with_name("2026-09-01-round3-remediation-delta.md"),
        help="staged principal WP packet registry",
    )
    parser.add_argument(
        "--handoff",
        type=Path,
        default=(
            Path(__file__).parent.parent
            / "handoff"
            / "2026-09-01-session-state-go-live-remediation.md"
        ),
        help="canonical session handoff carrying checkpoint schemas and sequencing",
    )
    args = parser.parse_args(argv)

    try:
        ok, failures, context = check(
            args.triage,
            args.plan,
            args.wp_check,
            args.plan_check,
            args.dag,
            args.delta,
            args.handoff,
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
