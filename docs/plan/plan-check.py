#!/usr/bin/env python3
"""plan-check: prove the coverage matrix is TOTAL and DISJOINT over all 247 findings."""

import collections
import re
import sys


EXPECTED_FINDING_COUNT = 247
SOURCE_ID_RE = re.compile(
    r"^(?:adopt|billing-money-path|ci-cd|conf|deploy|docs-truth|e2e|"
    r"fabric-core|fabricd|fabricd-deploy|gap|hist|live-probe|runner-core|"
    r"sc|sec|spawn-cf)-[0-9]{2}$"
)

B = {}


def b(name, ids):
    B[name] = ids.split()


b(
    "W0-unblock",
    """
ci-cd-02 deploy-01 spawn-cf-05 hist-03 gap-17 fabricd-deploy-10 live-probe-02 deploy-08
live-probe-01
e2e-01
hist-20
""",
)

b(
    "W1-parallel",
    """
sc-01 sc-07 fabricd-02 billing-money-path-10 billing-money-path-11 fabric-core-06
ci-cd-01 ci-cd-03 ci-cd-04 ci-cd-05 ci-cd-06 ci-cd-07 ci-cd-09 ci-cd-11 ci-cd-14
conf-01 conf-02 conf-03 sec-04 e2e-05
adopt-03 adopt-04 adopt-06 adopt-07 adopt-08 adopt-09 adopt-10 adopt-11 adopt-12 adopt-14
hist-02
ci-cd-12
""",
)

b(
    "W2-serial-worker",
    """
billing-money-path-03 hist-06 billing-money-path-05 billing-money-path-06 billing-money-path-08 adopt-16 hist-08
spawn-cf-01 spawn-cf-11 sc-02 sc-06 billing-money-path-12
 deploy-03  deploy-16 spawn-cf-02 spawn-cf-03 spawn-cf-04 spawn-cf-06 sec-05
deploy-02 deploy-04
""",
)

b(
    "W3-live-proof",
    """
 live-probe-05  e2e-02 e2e-04
fabricd-deploy-01 fabricd-deploy-12 fabricd-01 hist-04 hist-05 deploy-05 deploy-12 deploy-15 sec-08
live-probe-06
hist-12
fabricd-deploy-11
spawn-cf-15
deploy-14
fabric-core-16
billing-money-path-14
fabricd-09
""",
)

b(
    "W4-post-decision",
    """
runner-core-02 runner-core-05
fabric-core-02 fabric-core-03 fabric-core-07 fabric-core-08 fabric-core-09 fabric-core-10
fabric-core-11 fabric-core-15
fabricd-03 fabricd-07 fabricd-10
sc-04 sc-09 sc-10 spawn-cf-10 spawn-cf-14 fabricd-deploy-05
gap-08 gap-12 gap-14 gap-15 gap-16 gap-18 gap-19 gap-20 gap-24 gap-29
billing-money-path-04 billing-money-path-13 sec-03 sec-06 sec-07
e2e-03 e2e-07 conf-06 hist-09 hist-14 live-probe-04 live-probe-10
fabricd-deploy-04
fabric-core-05
""",
)

b(
    "DECISION",
    """
docs-truth-01 docs-truth-02 docs-truth-03 docs-truth-22
adopt-01 adopt-05 gap-06 gap-09 gap-13 fabric-core-01 fabricd-deploy-09
sec-01 sec-02 hist-01  billing-money-path-09
ci-cd-08
""",
)

b(
    "ARMING",
    """
gap-01 gap-02 gap-03  gap-07 gap-11 gap-25
runner-core-04 sc-05 spawn-cf-07 spawn-cf-08 spawn-cf-09 fabricd-04 fabricd-deploy-06
docs-truth-21 deploy-07 deploy-13 adopt-02 adopt-15 adopt-18
billing-money-path-01 hist-07 hist-10 hist-11  hist-13 live-probe-03 live-probe-08
""",
)

b(
    "RELAY",
    """
fabric-core-04 fabricd-05 conf-04 billing-money-path-07 adopt-17
gap-05
deploy-06
docs-truth-20
""",
)

b(
    "DOCS-sweep",
    """
runner-core-03 runner-core-08  fabric-core-12 fabricd-06 fabricd-08
sc-03 sc-11 spawn-cf-12 spawn-cf-13 conf-05
fabricd-deploy-02 fabricd-deploy-03 
 deploy-09 deploy-10 deploy-11 deploy-17 ci-cd-10 e2e-06
docs-truth-04 docs-truth-05 docs-truth-06 docs-truth-07 docs-truth-08 docs-truth-09
docs-truth-10 docs-truth-11 docs-truth-12 docs-truth-13 docs-truth-14 docs-truth-15
docs-truth-16 docs-truth-17 docs-truth-18 docs-truth-19 
gap-04 gap-10 gap-21 gap-22 gap-23 gap-30
 live-probe-07 billing-money-path-02 adopt-13 hist-15 
""",
)

b(
    "CLEAN-no-action",
    """
runner-core-06 runner-core-07 runner-core-09 fabric-core-13 fabric-core-14 
fabricd-00  sc-12  spawn-cf-16 fabricd-deploy-07 
  ci-cd-13 e2e-08 docs-truth-23 docs-truth-24
gap-26 gap-27 gap-28  hist-16 hist-17 hist-18 hist-19 live-probe-09
""",
)

b(
    "DEFER-needs-waiver",
    """
runner-core-01 sc-08 fabricd-deploy-08  ci-cd-15 billing-money-path-15


""",
)

source_path = sys.argv[1]
source_shape_errors = []
all_ids = []
with open(source_path, encoding="utf-8") as source:
    for line_no, raw_line in enumerate(source, 1):
        # The audit catalogue is deliberately a plain one-ID-per-line file:
        # do not let a set() hide duplicate physical rows or malformed rows.
        line = raw_line.rstrip("\n\r")
        if not line:
            source_shape_errors.append(f"line {line_no}: blank line (expected one id)")
            continue
        if line != line.strip():
            source_shape_errors.append(f"line {line_no}: surrounding whitespace")
        if not SOURCE_ID_RE.fullmatch(line.strip()):
            source_shape_errors.append(f"line {line_no}: invalid finding id {line!r}")
        all_ids.append(line.strip())

source_counts = collections.Counter(all_ids)
source_dupes = {i: c for i, c in source_counts.items() if c > 1}
assigned = collections.Counter()
for name, ids in B.items():
    for i in ids:
        assigned[i] += 1

known = set(all_ids)
dupes = {i: c for i, c in assigned.items() if c > 1}
assignment_shape_errors = sorted(i for i in assigned if not SOURCE_ID_RE.fullmatch(i))
unknown = sorted(set(assigned) - known)
orphans = sorted(known - set(assigned))

print(
    f"findings: {len(all_ids)} physical / {len(known)} unique   "
    f"assigned-unique: {len(set(assigned))}"
)
for name, ids in B.items():
    print(f"  {name:22s} {len(ids):3d}")
print(f"\nDUPLICATE (owned twice): {len(dupes)}")
for i in sorted(dupes):
    print("   ", i, "x", dupes[i], "in", [n for n, v in B.items() if i in v])
print(f"\nSOURCE DUPLICATE (physical rows): {len(source_dupes)}")
for i in sorted(source_dupes):
    print("   ", i, "x", source_dupes[i])
print(f"\nSOURCE SHAPE ERROR: {len(source_shape_errors)}")
for error in source_shape_errors:
    print("   ", error)
print(f"\nASSIGNMENT SHAPE ERROR: {len(assignment_shape_errors)}")
for i in assignment_shape_errors:
    print("   ", i)
print(f"\nUNKNOWN id (typo / not a finding): {len(unknown)}")
for i in unknown:
    print("   ", i)
print(f"\nORPHAN (no bucket): {len(orphans)}")
for i in orphans:
    print("   ", i)

ok = (
    len(all_ids) == EXPECTED_FINDING_COUNT
    and len(known) == EXPECTED_FINDING_COUNT
    and not source_dupes
    and not source_shape_errors
    and not assignment_shape_errors
    and not dupes
    and not unknown
    and not orphans
)
print("\nplan-check:", "PASS — total and disjoint" if ok else "BLOCKED")
sys.exit(0 if ok else 1)
