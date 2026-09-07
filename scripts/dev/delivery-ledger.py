#!/usr/bin/env python3
"""Mechanical checks for the delivery ledger (stdlib only)."""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SHA = re.compile(r"^[0-9a-f]{40}$")
COMMIT_REF = re.compile(r"^[0-9a-f]{7,64}$")
WP = re.compile(r"\bT\d+-W\d+[a-z]?\b")
TOKEN = re.compile(r"\b(?:T\d+-W\d+[a-z]?|D\d+|R\d+|O(?:1|-[A-Z0-9-]+))\b")
EXPECTED_SCOPE = {
    1: {"T3-W18", "T8-W4b", "T6-W4", "T6-W9", "T6-W13", "T6-W15", "T4-W4", "T3-W10", "T1-W5", "T6-W2", "T2-W3", "T9-W1"},
    2: {"T2-W2b", "T2-W4", "T2-W6", "T4-W1", "T4-W2", "T3-W3", "T3-W1", "T3-W2", "T8-W1", "T8-W3", "T3-W14", "T3-W9", "T8-W5", "T8-W2"},
    3: {"T3-W16", "T3-W15", "T6-W12", "T1-W6", "T6-W6", "T1-W2", "T1-W3", "T2-W5", "T3-W7", "T3-W8", "T4-W7", "T4-W8", "T6-W5", "T6-W7", "T8-W6", "T6-W14", "T6-W10", "T1-W4", "T3-W5", "T5-W1", "T5-W2", "T5-W3", "T5-W4", "T5-W5", "T5-W6", "T6-W11", "T7-W5", "T8-W7"},
}
HISTORICAL_SCOPE = {
    1: EXPECTED_SCOPE[1],
    2: EXPECTED_SCOPE[2],
    3: {"T3-W16", "T3-W15", "T6-W12", "T1-W6", "T6-W6", "T1-W2", "T1-W3", "T2-W5", "T3-W7", "T3-W8", "T4-W7", "T4-W8", "T6-W5", "T6-W7", "T8-W6"},
    4: {"T6-W14", "T6-W10", "T1-W4", "T3-W5", "T5-W1", "T5-W2", "T5-W3", "T5-W4", "T5-W5", "T5-W6", "T6-W11", "T7-W5", "T8-W7"},
}
ISO = re.compile(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:\d\d)$")
LANE = re.compile(r"\bW([0-4])\b")


class Errors:
    def __init__(self) -> None:
        self.items: list[str] = []

    def add(self, message: str) -> None:
        self.items.append(message)

    def require(self, condition: bool, message: str) -> None:
        if not condition:
            self.add(message)


def git(repo: Path, *args: str) -> str:
    try:
        return subprocess.check_output(["git", "-C", str(repo), *args], text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return ""


def canonical(repo: Path, registry: str) -> tuple[set[str], dict[str, set[str]], dict[str, int]]:
    path = repo / registry
    if not path.is_file():
        return set(), {}, {}
    ids: set[str] = set()
    deps: dict[str, set[str]] = {}
    lanes: dict[str, int] = {}
    inside = False
    seen: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip() == "## Canonical node table":
            inside = True
            continue
        if line.startswith("## ") and inside:
            break
        if not inside:
            continue
        if not line.lstrip().startswith("|"):
            continue
        cols = [x.strip() for x in line.split("|")]
        if len(cols) < 4:
            continue
        match = WP.fullmatch(cols[1])
        if not match:
            continue
        node = match.group(0)
        if node in seen:
            raise ValueError(f"duplicate canonical node {node}")
        seen.add(node)
        ids.add(node)
        deps[node] = set(TOKEN.findall(cols[3]))
        lane = LANE.search(cols[2])
        if lane:
            lanes[node] = int(lane.group(1))
    return ids, deps, lanes


def sha(value: object) -> bool:
    return isinstance(value, str) and bool(SHA.fullmatch(value))


def commit_ref(value: object) -> bool:
    """Accept an external source SHA prefix, while gates still require a local full SHA."""
    return isinstance(value, str) and bool(COMMIT_REF.fullmatch(value))


def valid_commit(repo: Path, value: object) -> bool:
    return sha(value) and subprocess.call(["git", "-C", str(repo), "cat-file", "-e", f"{value}^{{commit}}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) == 0


def ancestors(repo: Path, ancestor: str, descendant: str) -> bool:
    return subprocess.call(["git", "-C", str(repo), "merge-base", "--is-ancestor", ancestor, descendant],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) == 0


def list_field(obj: object, name: str, errors: Errors) -> list:
    value = obj.get(name) if isinstance(obj, dict) else None
    errors.require(isinstance(value, list), f"{name} must be an array")
    return value if isinstance(value, list) else []


def structure(value: object) -> list[str]:
    """Reject malformed nested values before set operations or Git checks."""
    errors = Errors()

    def object_at(obj: object, label: str) -> bool:
        errors.require(isinstance(obj, dict), f"{label}: expected object")
        return isinstance(obj, dict)

    def strings(obj: dict, keys: tuple[str, ...], label: str) -> None:
        for key in keys:
            errors.require(isinstance(obj.get(key), str) and bool(obj[key].strip()),
                           f"{label}.{key}: expected nonempty string")

    def arrays(obj: dict, keys: tuple[str, ...], label: str) -> None:
        for key in keys:
            entries = obj.get(key)
            errors.require(isinstance(entries, list) and
                           all(isinstance(x, str) and bool(x.strip()) for x in entries),
                           f"{label}.{key}: expected string array")

    if not object_at(value, "ledger"):
        return errors.items
    strings(value, ("schema_version", "registry"), "ledger")
    errors.require(value.get("registry") == "docs/plan/2026-09-01-reconciled-dispatch-dag.md",
                   "registry must name the canonical dispatch registry")
    baseline = value.get("baseline")
    if object_at(baseline, "baseline"):
        strings(baseline, ("main_commit", "prepared_commit", "source"), "baseline")
        errors.require(type(baseline.get("recorded_delivered")) is int,
                       "baseline.recorded_delivered: expected integer")
    scope = value.get("sprint_scope")
    if object_at(scope, "sprint_scope"):
        errors.require(set(scope) == {"1", "2", "3"}, "sprint_scope needs operational keys 1..3")
        arrays(scope, ("1", "2", "3"), "sprint_scope")
    historical_scope = value.get("historical_sprint_scope")
    if object_at(historical_scope, "historical_sprint_scope"):
        errors.require(set(historical_scope) == {"1", "2", "3", "4"}, "historical_sprint_scope needs keys 1..4")
        arrays(historical_scope, ("1", "2", "3", "4"), "historical_sprint_scope")
    model = value.get("operational_sprint_model")
    if object_at(model, "operational_sprint_model"):
        strings(model, ("id", "rules"), "operational_sprint_model")
        bundles = model.get("bundles")
        errors.require(isinstance(bundles, dict) and set(bundles) == {"1", "2", "3"},
                       "operational_sprint_model.bundles needs keys 1..3")
    for section in ("items", "sprints", "findings", "activity"):
        rows = value.get(section)
        errors.require(isinstance(rows, list), f"{section}: expected array")
        for row in rows if isinstance(rows, list) else []:
            if not object_at(row, section):
                continue
            if section == "items":
                strings(row, ("id", "state", "implementation", "owner", "next_action"), section)
                arrays(row, ("dependencies", "blockers", "commits", "evidence"), section)
                errors.require(type(row.get("sprint")) is int, "item sprint: expected integer")
                errors.require(type(row.get("historical_sprint")) is int, "item historical_sprint: expected integer")
                review = row.get("review")
                if object_at(review, "review"):
                    strings(review, ("status",), "review")
                    arrays(review, ("evidence",), "review")
                    errors.require(review.get("commit") is None or commit_ref(review["commit"]),
                                   "review.commit: expected commit reference or null")
            elif section == "sprints":
                errors.require(type(row.get("id")) is int, "sprint id: expected integer")
                strings(row, ("state",), section)
                for key in ("tip_commit", "merge_commit"):
                    errors.require(row.get(key) is None or sha(row[key]), f"sprint {key}: invalid SHA")
                for key in ("ci", "acceptance"):
                    detail = row.get(key)
                    if object_at(detail, key):
                        strings(detail, ("status",), key)
                        arrays(detail, ("evidence",), key)
                        if key == "ci":
                            errors.require(detail.get("commit") is None or sha(detail["commit"]),
                                           "ci.commit: expected SHA or null")
            elif section == "findings":
                strings(row, ("id", "wp", "state", "severity", "owner", "summary",
                              "reproduction", "acceptance"), section)
                arrays(row, ("commits", "evidence"), section)
                errors.require(type(row.get("sprint")) is int, "finding sprint: expected integer")
                errors.require(type(row.get("blocks_delivery")) is bool,
                               "finding blocks_delivery: expected boolean")
            else:
                strings(row, ("at", "task", "owner", "state"), section)
                arrays(row, ("scope",), section)
    return errors.items


def validate(repo: Path, ledger_path: Path, mode: str, target: int | None = None) -> tuple[list[str], str]:
    errors = Errors()
    try:
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return [f"cannot read ledger: {exc}"], ""
    shape_errors = structure(ledger)
    if shape_errors:
        return shape_errors, ""
    errors.require(isinstance(ledger, dict), "ledger must be an object")
    if not isinstance(ledger, dict):
        return errors.items, ""
    errors.require(ledger.get("schema_version") == "delivery-ledger/v1", "schema_version must be delivery-ledger/v1")
    registry = ledger.get("registry")
    errors.require(isinstance(registry, str), "registry must be a string")
    try:
        canonical_ids, canonical_deps, _ = canonical(repo, registry if isinstance(registry, str) else "")
    except ValueError as exc:
        return [f"canonical registry invalid: {exc}"], ""
    errors.require(bool(canonical_ids), "canonical node table is missing or empty")
    baseline = ledger.get("baseline", {})
    errors.require(isinstance(baseline, dict), "baseline must be an object")
    if not isinstance(baseline, dict):
        baseline = {}
    for key in ("main_commit", "prepared_commit"):
        errors.require(sha(baseline.get(key)), f"baseline.{key} must be a 40-character SHA")
        errors.require(valid_commit(repo, baseline.get(key)), f"baseline.{key} must be an existing commit")
    if valid_commit(repo, baseline.get("main_commit")) and valid_commit(repo, baseline.get("prepared_commit")):
        errors.require(ancestors(repo, baseline["main_commit"], baseline["prepared_commit"]), "baseline prepared_commit must descend from main_commit")
    errors.require(isinstance(baseline.get("recorded_delivered"), int), "baseline.recorded_delivered must be an integer")
    items = list_field(ledger, "items", errors)
    item_map: dict[str, dict] = {}
    for item in items:
        if not isinstance(item, dict):
            errors.add("each item must be an object")
            continue
        ident = item.get("id")
        if not isinstance(ident, str):
            errors.add("item id must be a string")
            continue
        if ident in item_map:
            errors.add(f"duplicate item {ident}")
        item_map[ident] = item
        errors.require(ident in canonical_ids, f"unmapped item {ident}")
        errors.require(isinstance(item.get("sprint"), int) and item.get("sprint") in range(4), f"{ident}: operational sprint must be 0..3")
        errors.require(isinstance(item.get("historical_sprint"), int) and item.get("historical_sprint") in range(5), f"{ident}: historical sprint must be 0..4")
        errors.require(isinstance(item.get("state"), str) and item.get("state") in {"recorded_delivered", "backlog", "active", "prepared", "ready", "delivered"}, f"{ident}: invalid state")
        errors.require(isinstance(item.get("implementation"), str) and item.get("implementation") in {"unknown", "missing", "partial", "complete"}, f"{ident}: invalid implementation")
        if item.get("state") in {"ready", "delivered"}:
            errors.require(item.get("implementation") == "complete",
                           f"{ident}: ready/delivered requires complete implementation")
        errors.require(bool(item.get("owner")) and isinstance(item.get("owner"), str), f"{ident}: owner is required")
        errors.require(bool(item.get("next_action")) and isinstance(item.get("next_action"), str), f"{ident}: next_action is required")
        errors.require(isinstance(item.get("dependencies"), list), f"{ident}: dependencies must be an array")
        for dep in item.get("dependencies", []) if isinstance(item.get("dependencies"), list) else []:
            errors.require(isinstance(dep, str), f"{ident}: dependency tokens must be strings")
        if ident in canonical_deps:
            deps_value = item.get("dependencies")
            actual = set(deps_value) if isinstance(deps_value, list) and all(isinstance(d, str) for d in deps_value) else set()
            errors.require(actual == canonical_deps[ident], f"{ident}: dependencies do not match canonical node table")
        errors.require(isinstance(item.get("blockers"), list), f"{ident}: blockers must be an array")
        errors.require(isinstance(item.get("commits"), list) and all(commit_ref(x) for x in item.get("commits", [])), f"{ident}: invalid commits")
        review = item.get("review")
        errors.require(isinstance(review, dict) and isinstance(review.get("status"), str) and review.get("status") in {"pending", "partial", "approved"}, f"{ident}: invalid review")
        if isinstance(review, dict) and review.get("commit") is not None:
            errors.require(commit_ref(review.get("commit")), f"{ident}: invalid review commit")
        if isinstance(review, dict) and review.get("status") == "approved":
            errors.require(bool(review.get("evidence")), f"{ident}: approved review needs evidence")
    for ident, item in item_map.items():
        for dep in item.get("dependencies", []) if isinstance(item.get("dependencies"), list) else []:
            if isinstance(dep, str) and dep in item_map:
                errors.require(item.get("sprint", 9) >= item_map[dep].get("sprint", -1), f"{ident}: dependency {dep} is assigned later")
    errors.require(set(item_map) == canonical_ids, f"item coverage mismatch: expected {len(canonical_ids)}, got {len(item_map)}")

    scope = ledger.get("sprint_scope", {})
    errors.require(isinstance(scope, dict), "sprint_scope must be an object")
    scoped: set[str] = set()
    for sprint, expected in ((1, 12), (2, 14), (3, 28)):
        values = scope.get(str(sprint), []) if isinstance(scope, dict) else []
        errors.require(isinstance(values, list), f"sprint_scope.{sprint} must be an array")
        if not isinstance(values, list):
            values = []
        errors.require(len(values) == expected, f"sprint_scope.{sprint} must contain {expected} WPs")
        errors.require(all(isinstance(v, str) for v in values), f"sprint_scope.{sprint} entries must be strings")
        errors.require(len(values) == len(set(values)) if all(isinstance(v, str) for v in values) else False, f"sprint_scope.{sprint} contains duplicate WPs")
        errors.require(set(values) == EXPECTED_SCOPE[sprint], f"sprint_scope.{sprint} does not match the frozen membership for operational sprint {sprint}")
        for ident in values:
            errors.require(ident in canonical_ids, f"sprint_scope.{sprint}: unmapped WP {ident}")
            errors.require(ident not in scoped, f"WP {ident} appears in multiple sprint scopes")
            scoped.add(ident)
            if ident in item_map:
                errors.require(item_map[ident].get("sprint") == sprint, f"{ident}: item sprint disagrees with scope")
    errors.require(len(scoped) == 54, f"operational sprint scopes must cover 54 WPs, got {len(scoped)}")
    historical_scope = ledger.get("historical_sprint_scope", {})
    historical_scoped: set[str] = set()
    for sprint, expected in ((1, 12), (2, 14), (3, 15), (4, 13)):
        values = historical_scope.get(str(sprint), []) if isinstance(historical_scope, dict) else []
        errors.require(isinstance(values, list) and set(values) == HISTORICAL_SCOPE[sprint], f"historical_sprint_scope.{sprint} does not match frozen historical membership")
        errors.require(len(values) == expected, f"historical_sprint_scope.{sprint} must contain {expected} WPs")
        for ident in values if isinstance(values, list) else []:
            errors.require(ident not in historical_scoped, f"WP {ident} appears in multiple historical sprint scopes")
            historical_scoped.add(ident)
            if ident in item_map:
                errors.require(item_map[ident].get("historical_sprint") == sprint, f"{ident}: historical sprint disagrees with historical scope")
    errors.require(historical_scoped == scoped, "historical and operational sprint scopes must cover the same 54 WPs")
    model = ledger.get("operational_sprint_model")
    errors.require(isinstance(model, dict), "operational_sprint_model must be an object")
    bundles = model.get("bundles") if isinstance(model, dict) else None
    expected_bundles = {
        "1": ("B1", 12, None),
        "2": ("B2", 14, "B1"),
        "3": ("B3", 28, "B2"),
    }
    errors.require(isinstance(bundles, dict) and set(bundles) == set(expected_bundles),
                   "operational_sprint_model must define exactly B1, B2, and B3")
    if isinstance(bundles, dict):
        for sprint, (bundle, count, predecessor) in expected_bundles.items():
            row = bundles.get(sprint)
            errors.require(isinstance(row, dict) and row.get("bundle") == bundle and
                           row.get("work_packages") == count and row.get("starts_after_merge") == predecessor,
                           f"operational sprint {sprint} has an invalid bundle or serial predecessor")
    for ident, item in item_map.items():
        if item.get("sprint") == 0:
            errors.require(ident not in scoped, f"baseline WP {ident} is in sprint scope")
            errors.require(item.get("state") == "recorded_delivered", f"baseline WP {ident} must be recorded_delivered")
            errors.require(item.get("historical_sprint") == 0, f"baseline WP {ident} must retain historical sprint 0")
    errors.require(sum(1 for i in item_map.values() if i.get("sprint") == 0) == 16, "baseline complement must contain 16 WPs")
    errors.require(baseline.get("recorded_delivered") == 16, "baseline.recorded_delivered must equal computed 16")

    findings = list_field(ledger, "findings", errors)
    finding_ids: set[str] = set()
    for finding in findings:
        if not isinstance(finding, dict):
            errors.add("each finding must be an object")
            continue
        fid = finding.get("id")
        errors.require(isinstance(fid, str) and fid not in finding_ids, f"duplicate or invalid finding {fid}")
        if isinstance(fid, str): finding_ids.add(fid)
        errors.require(finding.get("wp") in item_map, f"finding {fid}: unknown WP")
        if finding.get("wp") in item_map:
            errors.require(finding.get("sprint") == item_map[finding["wp"]].get("sprint"), f"finding {fid}: sprint does not match WP")
        errors.require(isinstance(finding.get("state"), str) and finding.get("state") in {"open", "active", "fixed", "delivered"}, f"finding {fid}: invalid state")
        if finding.get("state") in {"open", "active"}:
            for key in ("owner", "reproduction", "acceptance"):
                errors.require(bool(finding.get(key)), f"finding {fid}: {key} required while open/active")
        if finding.get("state") in {"fixed", "delivered"}:
            errors.require(bool(finding.get("commits")) and all(commit_ref(x) for x in finding.get("commits", [])), f"finding {fid}: fixed finding needs commits")
            errors.require(bool(finding.get("evidence")), f"finding {fid}: fixed finding needs evidence")

    activity = list_field(ledger, "activity", errors)
    active: list[tuple[str, str, str]] = []
    for row in activity:
        if not isinstance(row, dict):
            errors.add("each activity row must be an object")
            continue
        errors.require(bool(ISO.match(str(row.get("at", "")))), f"activity {row.get('task')}: invalid ISO timestamp")
        paths = row.get("scope", [])
        errors.require(isinstance(paths, list), f"activity {row.get('task')}: scope must be an array")
        for path in paths if isinstance(paths, list) else []:
            errors.require(isinstance(path, str) and not any(x in path for x in "*?[]{}"), f"activity {row.get('task')}: scope paths must be simple")
            if row.get("state") == "active" and isinstance(path, str):
                active.append((str(row.get("task")), path.rstrip("/"), str(row.get("owner"))))
    for index, (task, path, owner) in enumerate(active):
        for other, other_path, other_owner in active[index + 1:]:
            overlap = path == other_path or path.startswith(other_path + "/") or other_path.startswith(path + "/")
            if overlap and task != other:
                errors.add(f"active task scopes overlap: {task} and {other}")

    sprints = ledger.get("sprints", [])
    errors.require(isinstance(sprints, list), "sprints must be an array")
    by_sprint = {s.get("id"): s for s in sprints if isinstance(s, dict) and isinstance(s.get("id"), int)}
    errors.require(set(by_sprint) == {1, 2, 3} and len(sprints) == 3, "sprints must contain exactly one record for operational ids 1..3")
    for ident, item in item_map.items():
        if item.get("state") == "delivered":
            errors.require(by_sprint.get(item.get("sprint"), {}).get("state") == "delivered", f"{ident}: delivered item requires delivered sprint")
    for sprint, row in by_sprint.items():
        errors.require(row.get("state") in {"backlog", "implementation", "validation", "acceptance", "delivered"}, f"sprint {sprint}: invalid state")
        for key, allowed in (("ci", {"not_run", "running", "passed", "failed"}), ("acceptance", {"pending", "passed", "failed"})):
            value = row.get(key)
            errors.require(isinstance(value, dict) and isinstance(value.get("status"), str) and value.get("status") in allowed, f"sprint {sprint}: invalid {key}")
            errors.require(isinstance(value, dict) and isinstance(value.get("evidence", []), list), f"sprint {sprint}: {key}.evidence must be an array")
    for sprint, row in by_sprint.items():
        if row.get("state") != "delivered":
            continue
        tip = row.get("tip_commit")
        ci = row.get("ci") if isinstance(row.get("ci"), dict) else {}
        acceptance = row.get("acceptance") if isinstance(row.get("acceptance"), dict) else {}
        errors.require(valid_commit(repo, tip), f"sprint {sprint}: delivered tip_commit is invalid")
        errors.require(ci.get("status") == "passed" and ci.get("commit") == tip and ci.get("evidence"), f"sprint {sprint}: delivered sprint needs tip-bound CI evidence")
        errors.require(acceptance.get("status") == "passed" and acceptance.get("evidence"), f"sprint {sprint}: delivered sprint needs acceptance evidence")
        merge = row.get("merge_commit")
        errors.require(valid_commit(repo, merge) and valid_commit(repo, tip) and ancestors(repo, tip, merge), f"sprint {sprint}: merge_commit must descend from tip")
    for sprint, row in by_sprint.items():
        ci = row.get("ci") if isinstance(row.get("ci"), dict) else {}
        if ci.get("status") in {"running", "passed"}:
            for prior in range(1, sprint):
                errors.require(by_sprint.get(prior, {}).get("state") == "delivered",
                               f"sprint {sprint}: CI cannot run before sprint {prior} is merged")
            for item in (i for i in item_map.values() if i.get("sprint") == sprint):
                errors.require(item.get("implementation") == "complete" and item.get("state") in {"ready", "delivered"} and not item.get("blockers"), f"{item.get('id')}: CI cannot run before implementation readiness")
    def ready_check(sprint: int, delivery: bool) -> None:
        for prior in range(1, sprint):
            errors.require(by_sprint.get(prior, {}).get("state") == "delivered",
                           f"sprint {sprint}: cannot enter a gate before sprint {prior} is merged")
        selected = [i for i in item_map.values() if i.get("sprint") == sprint]
        sprint_row = by_sprint.get(sprint, {})
        tip = sprint_row.get("tip_commit")
        errors.require(len(selected) == {1: 12, 2: 14, 3: 28}.get(sprint, -1), f"sprint {sprint}: wrong item count")
        for item in selected:
            ident = item["id"]
            errors.require(item.get("implementation") == "complete", f"{ident}: implementation is not complete")
            errors.require(item.get("state") in {"ready", "delivered"}, f"{ident}: item is not ready/delivered")
            errors.require(not item.get("blockers"), f"{ident}: blockers remain")
            review = item.get("review", {})
            review_commit = review.get("commit")
            errors.require(
                review.get("status") == "approved" and valid_commit(repo, review_commit) and
                valid_commit(repo, tip) and ancestors(repo, review_commit, tip),
                f"{ident}: approved review commit is not valid for sprint tip",
            )
            for dep in item["dependencies"]:
                if dep in item_map:
                    predecessor = item_map[dep]
                    allowed = {"recorded_delivered"} if predecessor["sprint"] == 0 else {"ready", "delivered"}
                    errors.require(predecessor["state"] in allowed,
                                   f"{ident}: dependency {dep} is not ready")
        errors.require(valid_commit(repo, tip), f"sprint {sprint}: tip_commit is not an existing commit")
        for item in selected:
            for commit in item.get("commits", []):
                errors.require(valid_commit(repo, commit) and valid_commit(repo, tip) and ancestors(repo, commit, tip), f"{item['id']}: commit is not contained by sprint tip")
        if delivery:
            ci = sprint_row.get("ci", {})
            acceptance = sprint_row.get("acceptance", {})
            errors.require(ci.get("status") == "passed" and ci.get("commit") == tip and ci.get("evidence"), f"sprint {sprint}: CI must be passed for the exact tip with evidence")
            errors.require(acceptance.get("status") == "passed" and acceptance.get("evidence"), f"sprint {sprint}: acceptance must be passed with evidence")
            errors.require(not any(isinstance(f.get("state"), str) and f.get("state") in {"open", "active"} and f.get("blocks_delivery") for f in findings if isinstance(f, dict) and f.get("sprint") == sprint), f"sprint {sprint}: open blocking finding")
            ids = {i["id"] for i in selected}
            for item in selected:
                errors.require(all(item_map[d].get("state") in {"ready", "delivered"} for d in item.get("dependencies", []) if d in ids), f"{item['id']}: in-sprint dependency not ready")
                errors.require(all(item_map[d].get("state") in {"ready", "delivered", "recorded_delivered"} for d in item.get("dependencies", []) if d in item_map and d not in ids), f"{item['id']}: earlier dependency not ready")
                if item.get("state") == "delivered":
                    errors.require(sprint_row.get("state") == "delivered", f"{item['id']}: delivered item requires delivered sprint")
            if sprint_row.get("state") == "delivered":
                merge = sprint_row.get("merge_commit")
                errors.require(valid_commit(repo, merge) and ancestors(repo, tip, merge), f"sprint {sprint}: merge_commit must descend from tip")
    if mode in {"ready-ci", "ready-delivery"}:
        ready_check(target or 0, mode == "ready-delivery")
    for sprint, row in by_sprint.items():
        if row.get("state") == "delivered":
            ready_check(sprint, True)
        elif row["ci"]["status"] in {"running", "passed", "failed"}:
            ready_check(sprint, False)
            errors.require(row["ci"]["commit"] == row["tip_commit"],
                           f"sprint {sprint}: CI source must equal the composed tip")
            if row["ci"]["status"] in {"passed", "failed"}:
                errors.require(bool(row["ci"]["evidence"]), f"sprint {sprint}: CI result needs evidence")

    lines = [f"historical recorded source WPs: {sum(1 for i in item_map.values() if i.get('sprint') == 0)}; operational sprints delivered: {sum(1 for s in range(1, 4) if by_sprint.get(s, {}).get('state') == 'delivered')} / 3"]
    for sprint in range(1, 4):
        selected = [i for i in item_map.values() if i.get("sprint") == sprint]
        lines.append(f"sprint {sprint}: implementation {sum(i.get('implementation') == 'complete' for i in selected)}/{len(selected)}; delivered WPs {sum(i.get('state') == 'delivered' for i in selected)}")
    active = [f"{r.get('task')} ({r.get('owner')})" for r in activity if isinstance(r, dict) and r.get("state") == "active"]
    unresolved = [f"{f.get('id')} {f.get('wp')} ({f.get('owner')})" for f in findings if isinstance(f, dict) and f.get("state") in {"open", "active"}]
    lines.append("active tasks: " + (", ".join(active) if active else "none"))
    lines.append("unresolved findings: " + (", ".join(unresolved) if unresolved else "none"))
    next_actions = [f"{i['id']}: {i.get('next_action')}" for i in item_map.values() if i.get("sprint") == 1 and i.get("state") == "active"]
    lines.append("sprint 1 active next actions: " + ("; ".join(next_actions) if next_actions else "none"))
    report = "\n".join(lines)
    return errors.items, report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ledger", type=Path, default=None)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--report", action="store_true")
    parser.add_argument("--ready-for-ci", type=int, metavar="N")
    parser.add_argument("--ready-for-delivery", type=int, metavar="N")
    args = parser.parse_args()
    if args.check and (args.ready_for_ci is not None or args.ready_for_delivery is not None):
        parser.error("--check cannot be combined with a readiness mode")
    if args.ready_for_ci is not None and args.ready_for_delivery is not None:
        parser.error("--ready-for-ci and --ready-for-delivery are mutually exclusive")
    repo = Path(__file__).resolve().parents[2]
    path = args.ledger or repo / "docs/plan/delivery-ledger.json"
    mode = "ready-ci" if args.ready_for_ci is not None else "ready-delivery" if args.ready_for_delivery is not None else "check"
    target = args.ready_for_ci if args.ready_for_ci is not None else args.ready_for_delivery
    errors, report = validate(repo, path, mode, target)
    if args.report:
        print(report)
    if errors:
        print("delivery ledger: FAIL", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print("delivery ledger: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
