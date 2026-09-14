#!/usr/bin/env python3
"""Check execution ownership and candidate diffs; never invokes CI or tests."""
from __future__ import annotations

import argparse
import json
from pathlib import Path, PurePosixPath
import subprocess


def validate(plan: dict, ledger: dict) -> list[str]:
    errors: list[str] = []
    expected = {i["id"] for i in ledger["items"] if i["sprint"] > 0}
    packets = plan["packets"]
    ids = [p["id"] for p in packets]
    if len(ids) != len(set(ids)) or set(ids) != expected:
        errors.append("packet coverage must equal the 54 remaining canonical WPs exactly")
    owners: dict[str, str] = {}
    for owner, paths in [("I0", plan["control_work"]["I0"]["owns"])] + [
        (p["id"], p["owns"]) for p in packets
    ]:
        for path in paths:
            parsed = PurePosixPath(path)
            if parsed.is_absolute() or ".." in parsed.parts or str(parsed) != path or any(c in path for c in "*?["):
                errors.append(f"{owner}: write grant must be an exact relative file: {path}")
            if path in owners:
                errors.append(f"write conflict: {path}: {owners[path]} / {owner}")
            owners[path] = owner
    for path in owners:
        for parent in PurePosixPath(path).parents:
            if str(parent) in owners:
                errors.append(f"directory/file overlap: {parent} / {path}")
    for packet in packets:
        if not set(packet.get("allowed_writes", [])) <= set(packet["owns"]):
            errors.append(f"{packet['id']}: dispatch writes exceed ownership")
        if packet["state"] in {"ready", "active"}:
            for field in ("decisions_closed", "contract", "allowed_writes", "gate_commands"):
                if not packet.get(field):
                    errors.append(f"{packet['id']}: cannot dispatch without {field}")
    for wave in plan["waves"]:
        if len(wave["wps"]) > plan["policy"]["max_active_executors"]:
            errors.append(f"{wave['id']}: executor cap exceeded")
        if len(wave["wps"]) != len(set(wave["wps"])) or not set(wave["wps"]) <= set(ids):
            errors.append(f"{wave['id']}: invalid or repeated WP")
    return errors


def candidate_errors(plan: dict, repo: Path, wp: str, base: str, head: str) -> list[str]:
    packet = next(p for p in plan["packets"] if p["id"] == wp)
    if packet["state"] not in {"active", "returned", "verified", "integrated"}:
        return [f"{wp}: candidate was not dispatched"]
    if base != packet.get("dispatch_baseline"):
        return [f"{wp}: candidate base differs from the root-recorded dispatch baseline"]
    def git(*args: str) -> str:
        return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()
    if git("rev-parse", base + "^{commit}") != base or git("rev-parse", head + "^{commit}") != head:
        return ["candidate checks require exact 40-character commit SHAs"]
    if subprocess.call(["git", "-C", str(repo), "merge-base", "--is-ancestor", base, head]):
        return ["candidate does not descend from its dispatch baseline"]
    # Inspect every commit, including a forbidden edit reverted before return.
    changed = set(git("log", "--format=", "--name-only", base + ".." + head).splitlines()) - {""}
    allowed = set(packet.get("allowed_writes", []))
    return [f"{wp}: unauthorized changed path: {p}" for p in sorted(changed - allowed)]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, default=Path("docs/plan/execution/2026-09-06-disjoint-plan.json"))
    parser.add_argument("--candidate", nargs=3, metavar=("WP", "BASE", "HEAD"))
    args = parser.parse_args()
    repo = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    plan = json.loads(args.plan.read_text())
    errors = validate(plan, json.loads((repo / plan["ledger"]).read_text()))
    canonical = subprocess.run(["python3", str(repo / "scripts/dev/delivery-ledger.py"), "--check"], cwd=repo, capture_output=True, text=True)
    if canonical.returncode:
        errors.append("canonical 70-WP/sprint ledger invalid: " + canonical.stdout + canonical.stderr)
    for packet in plan["packets"]:
        if packet["state"] in {"ready", "active"} and not (repo / packet["contract"]).is_file():
            errors.append(f"{packet['id']}: frozen packet artifact missing")
        if packet["state"] in {"ready", "active"}:
            # This rework wave cannot clear product dependencies or dispatch new
            # feature/live work; it only preserves source already in the base.
            if packet.get("execution_kind") != "preserve_existing_source":
                errors.append(f"{packet['id']}: feature/live dispatch requires a new D0 readiness decision")
            source = packet.get("requires_source_ancestor", "")
            if subprocess.call(["git", "-C", str(repo), "merge-base", "--is-ancestor", source, plan["baseline"]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL):
                errors.append(f"{packet['id']}: required source is not composed at baseline")
    if args.candidate:
        errors += candidate_errors(plan, repo, *args.candidate)
    for error in errors:
        print("FAIL:", error)
    if not errors:
        print(f"PASS: {len(plan['packets'])} canonical WPs; exact file ownership; no write overlap")
    return int(bool(errors))


if __name__ == "__main__":
    raise SystemExit(main())
