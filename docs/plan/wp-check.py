#!/usr/bin/env python3
"""wp-check: prove the WP layer is sound.

Blocks unless:
  * every live acceptance item is owned by exactly one WP (or is `judged` -> owner)
  * no WP owns zero items (every WP has structural ownership)
  * no WP owns more than 4 items (the sweet-spot ceiling)
  * every WP declares at least one invariant from the INV catalogue
  * no two PARALLEL WPs share an exclusive file scope

Item ids are parsed from the plan itself, so the two can never silently drift.
Usage: python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
"""
import re, sys
from collections import Counter

INV = {f"INV-{i}" for i in range(1, 9)}

# WP -> (items owned, invariants live, exclusive scope, wave)
WP = {
    # wave 0
    "T0-W1":  (["A0.1"],                     ["INV-5", "INV-6"], "docs/plan/union-ledger",        0),
    "T1-W1":  (["A1.4"],                     ["INV-5"],          "scripts/ops",                   0),
    "T2-W1a": (["A2.1"],                     ["INV-2"],          "build-cf-container-images.yml", 0),
    "T2-W2a": (["A2.3"],                     ["INV-2", "INV-5"], "scripts/ci/image-pin",          0),
    # wave 1 (parallel unless noted)
    "T3-W4":  (["A3.6", "A4.4", "A4.5"],     ["INV-1", "INV-3"], "crates/corelink-fabric-server", 1),
    "T4-W4":  (["A4.11", "A4.13"],           ["INV-3", "INV-4"], "crates/corelink-fabric-server", 1.5),  # serial after T3-W4
    "T6-W1":  (["A6.1", "A6.2", "A6.15"],    ["INV-7"],          "scripts+ci.yml",                1),
    "T6-W2":  (["A6.3"],                     ["INV-7"],          "moat-workflows+actions",        1),
    "T6-W3":  (["A6.4"],                     ["INV-1"],          "conformance.yml+sdk",           1),
    "T6-W8":  (["A6.8"],                     ["INV-7"],          "pg-suite.yml",                  1),
    "T6-W4":  (["A6.5", "A6.9", "A6.10"],    ["INV-8"],          "secret-scan+stress+canary",     1),
    "T5-W1":  (["A5.3"],                     ["INV-5"],          "docs/onboarding+actions-readme",1),
    "T5-W2":  (["A5.2", "A5.4", "A5.5"],     ["INV-2"],          "integrations+release.yml",      1),
    "T7-W1":  (["A7.2"],                     ["INV-5"],          "ROADMAP+CHANGELOG",             1),
    "T7-W2":  (["A7.1"],                     ["INV-5"],          "docs-sweep",                    1),
    "T7-W3":  (["A7.4", "A7.5"],             ["INV-5"],          "claim-artifact-lint",           1),
    "T9-W0":  (["A0.2"],                     ["INV-7"],          "devenv-coverage",               1),
    "T2-W3":  (["A2.6"],                     ["INV-2"],          "all-workflows(closer)",         1.9),
    # wave 2 (strictly serial on the worker monolith -> shared scope is EXPECTED)
    "T4-W1":  (["A4.1"],                     ["INV-4"],          "worker(serial)",                2),
    "T4-W2":  (["A4.2", "A4.3", "A4.6"],     ["INV-1", "INV-4"], "worker(serial)",                2),
    "T3-W3":  (["A3.5", "A3.11"],            ["INV-3"],          "worker(serial)",                2),
    "T3-W1":  (["A3.1", "A3.2", "A3.7"],     ["INV-1", "INV-8"], "worker(serial)",                2),
    "T3-W2":  (["A3.3", "A3.4", "A3.10", "A3.12"], ["INV-3"],    "worker(serial)",                2),
    "T8-W1":  (["A3.14", "A3.15", "A3.16"],  ["INV-3", "INV-8"], "worker(serial)",                2),
    "T8-W3":  (["A3.17", "A3.18"],           ["INV-3", "INV-4"], "worker(serial)",                2),
    "T8-W2":  (["A3.13"],                    ["INV-3", "INV-8"], "worker(serial)",                2),
    "T9-W1":  (["A3.8", "A4.8"],             ["INV-7"],          "worker(serial)",                2),
    # wave 3 (live proof)
    "T1-W2":  (["A1.1", "A1.2", "A1.3", "A1.5"], ["INV-5"],      "probe:control-plane",           3),
    "T1-W3":  (["A1.6", "A1.7"],             ["INV-5"],          "probe:resilience",              3),
    "T1-W4":  (["A1.8", "A1.9"],             ["INV-5"],          "probe:boot-rate+uptime",        3),
    "T2-W5":  (["A2.11", "A2.12", "A2.13"],  ["INV-5", "INV-7"], "runbook+compat",                1),
    "T4-W8":  (["A4.14", "A4.15"],           ["INV-4"],          "probe:billing-reconcile",       3),
    "T3-W8":  (["A3.19", "A3.20"],           ["INV-5"],          "probe:inventory+hitrate",       3),
    "T5-W5":  (["A5.10"],                    ["INV-5"],          "probe:stranger-adversarial",    3),
    "T6-W10": (["A6.16", "A6.17", "A6.18"],  ["INV-3", "INV-5"], "alerting-depth",                3),
    "T6-W11": (["A6.19"],                    ["INV-5"],          "runbook-execution",             3),
    "T7-W4b": (["A7.6"],                     ["INV-5"],          "artifact-freshness",            1),
    "T2-W2b": (["A2.4", "A2.5", "A2.7", "A2.10"], ["INV-2", "INV-5"], "probe:deploy",             3),
    "T2-W4":  (["A2.8", "A2.9"],             ["INV-2"],          "probe:image-ship",              3),
    "T3-W7":  (["A3.9"],                     ["INV-5"],          "probe:moat",                    3),
    "T4-W7":  (["A4.7", "A4.10", "A4.12"],   ["INV-1", "INV-4"], "probe:money",                   3),
    "T5-W4":  (["A5.6", "A5.8", "A5.9"],     ["INV-5"],          "probe:stranger",                3),
    "T6-W5":  (["A6.7"],                     ["INV-5"],          "probe:e2e",                     3),
    "T6-W6":  (["A6.6", "A6.13", "A6.14"],   ["INV-5"],          "probe:canary",                  3),
    "T6-W7":  (["A6.11"],                    ["INV-3"],          "probe:authz",                   3),
    "T6-W9":  (["A6.12"],                    ["INV-5"],          "probe:alert-rules",             3),
}

JUDGED_TO_OWNER = {"A4.9", "A5.1", "A7.3"}

doc = open(sys.argv[1]).read()
sec = doc[doc.index("## 3. The acceptance suite"):doc.index("## 4. Owner decisions")]
rows = re.findall(r"^\|\s*(?:\*\*)?(?:★)?(?:~~)?(A\d\.\d+)(?:~~)?(?:\*\*)?\s*\|\s*([^|]*?)\s*\|", sec, re.M)
items = {}
for i, k in rows:
    items.setdefault(i, k.strip())
withdrawn = {i for i, k in items.items() if k == "—"}
live = set(items) - withdrawn

owned = Counter(i for v, _, _, _ in WP.values() for i in v)
fail = []

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

# parallel-scope collision: only within the same integer wave, and wave 2 is serial by design
scopes = {}
for w, (_, _, sc, wave) in WP.items():
    if wave in (2,):          # serial chain — shared scope is the plan's explicit decision
        continue
    scopes.setdefault((wave, sc), []).append(w)
collide = {k: v for k, v in scopes.items() if len(v) > 1}
if collide:
    fail.append(f"PARALLEL scope collisions: {collide}")

print(f"suite rows {len(items)} · live {len(live)} · withdrawn {sorted(withdrawn)}")
print(f"WPs {len(WP)} · items owned {len(owned)} · judged->owner {sorted(JUDGED_TO_OWNER)}")
print(f"items per WP: min {min(len(v) for v,_,_,_ in WP.values())} max {max(len(v) for v,_,_,_ in WP.values())}")
for f in fail:
    print("  BLOCK:", f)
print("\nwp-check:", "BLOCKED" if fail else "PASS — every item owned once, structural WP ownership is complete")
sys.exit(1 if fail else 0)
