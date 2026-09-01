#!/usr/bin/env python3
"""wp-check: validate the WP layer's structural consistency.

Blocks unless:
  * every live acceptance item is owned by exactly one WP (or is `judged` -> owner)
  * no WP owns zero items (every WP has structural ownership)
  * no WP owns more than 4 items (the sweet-spot ceiling)
  * every WP declares at least one invariant from the INV catalogue
  * no two PARALLEL WPs share an exclusive file scope
  * the Markdown acceptance/ownership/scope tables match this catalogue exactly

This is a structural gate only. It does not assert that a WP is implemented,
falsifiable, green, or ready for production.
Usage: python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
"""

import re
import sys
from collections import Counter

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
        "mint-key arming/self-check · atomic spawn claim",
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


def exact_line_positions(text, heading):
    return [m.start() for m in re.finditer(rf"^{re.escape(heading)}$", text, re.M)]


def markdown_cells(line):
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def plain_markdown(value):
    value = value.replace("**", "").replace("`", "")
    return re.sub(r"\s+", " ", value).strip()


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


doc = open(sys.argv[1]).read()
fail = []

required_headings = [
    SUITE_HEADING,
    OWNER_HEADING,
    WAVES_HEADING,
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

rows = re.findall(
    r"^\|\s*(?:\*\*)?(?:★)?(?:~~)?(A\d\.\d+)(?:~~)?(?:\*\*)?\s*\|\s*([^|]*?)\s*\|",
    sec,
    re.M,
)
physical = Counter(i for i, _ in rows)
duplicate_rows = sorted(i for i, count in physical.items() if count > 1)
if duplicate_rows:
    fail.append(f"DUPLICATE physical acceptance rows: {duplicate_rows}")

items = {i: k.strip() for i, k in rows}
unknown_kinds = sorted((i, k) for i, k in items.items() if k not in ITEM_KINDS)
if unknown_kinds:
    fail.append(f"UNKNOWN acceptance kind labels: {unknown_kinds}")

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
