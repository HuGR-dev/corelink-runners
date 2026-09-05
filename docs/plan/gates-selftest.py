#!/usr/bin/env python3
"""Negative tests for the three structural planning gates.

Each mutation recreates a false PASS found by a cold review.  The self-test
passes only when the unmodified inputs pass and every corrupted copy blocks.

The fixtures intentionally exercise the physical Markdown surfaces as well as
the catalogues: a gate must not pass merely because a malformed row is opaque,
or because a summary/header/capability section was changed while the global id
set still looks correct.

These are structural parser/registry checks only. They do not prevent a
coordinated checker-and-source tamper or arbitrary semantic text edits, and a
PASS is not production, evidence, freeze, or dispatch readiness.
"""

from __future__ import annotations

import hashlib
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


PLAN_DIR = Path(__file__).resolve().parent
REPO = PLAN_DIR.parent.parent
SOURCE = PLAN_DIR / "audit-2026-08-30-finding-ids.txt"
PLAN = PLAN_DIR / "2026-08-30-golive-remediation-plan.md"
TRIAGE = PLAN_DIR / "union-triage-remaining.md"
DAG = PLAN_DIR / "2026-09-01-reconciled-dispatch-dag.md"
STAGED = PLAN_DIR / "2026-09-01-round3-remediation-delta.md"
HANDOFF = REPO / "docs" / "handoff" / "2026-09-01-session-state-go-live-remediation.md"

CANONICAL_INPUTS = {
    path.resolve() for path in (SOURCE, PLAN, TRIAGE, DAG, STAGED, HANDOFF)
}
EXPECTED_MUTATION_INVENTORY = frozenset(
    {
        "ack-token-schema-drift.md",
        "actionlint-dynamic-runner-expression",
        "actionlint-extra-allowed-label",
        "actionlint-commented-dead-sha-binding",
        "actionlint-inline-runs-on-mapping",
        "actionlint-inline-escaped-runner",
        "actionlint-plan-structure-duplicate-job",
        "actionlint-plan-structure-duplicate-step",
        "actionlint-sha-heredoc-string-copy",
        "actionlint-suppress-all-config",
        "actionlint-unexpected-workflow",
        "actionlint-weakened-sha-binding",
        "ambiguous-canary-status.md",
        "activation-artifact-exception-weakened.md",
        "activation-expiry-accepted.md",
        "activation-fixture-isolation-weakened.md",
        "activation-permitted-delta-weakened.md",
        "activation-phase-credit-swapped.md",
        "activation-replay-accepted.md",
        "activation-verified-negative-unknown.md",
        "au-bucket-drift.md",
        "au-invalid-kind.md",
        "au-scope-overlap.md",
        "bad-ack-recovery-schema-delta.md",
        "bad-ack-recovery-schema-main.md",
        "bad-canary-activation-schema-delta.md",
        "bad-canary-activation-schema-main.md",
        "bad-cf-rate-schema-dag.md",
        "bad-cf-rate-schema-delta.md",
        "bad-cf-rate-schema-plan.md",
        "bad-cf-rate-schema-triage.md",
        "bad-page-ack-schema-delta.md",
        "bad-page-ack-schema-main.md",
        "bad-signer-rotation-manifest-schema-delta.md",
        "bad-signer-rotation-manifest-schema-main.md",
        "capability-corruption.md",
        "collapsed-activation-lane-dag.md",
        "collapsed-activation-lane-delta.md",
        "collapsed-activation-lane-main.md",
        "deleted-t4-w3-dependency-dag.md",
        "disabled-job-selftests.yml",
        "drifted-t6-w12-provider-path.md",
        "duplicate-a-row.md",
        "duplicate-source.txt",
        "early-success-selftests.yml",
        "excluded-required-scope-colon.md",
        "excluded-required-scope.md",
        "fenced-au-placement-row.md",
        "fenced-capability-heading.md",
        "fenced-dag-table-row.md",
        "fenced-main-acceptance-row.md",
        "fenced-staged-principal-registry.md",
        "forbidden-t6-w6-before-t6-w13.md",
        "handoff-ack-recovery-missing-binding.md",
        "handoff-page-ack-missing-binding.md",
        "handoff-stale-canary-sequence.md",
        "header-corruption.md",
        "html-comment-hidden-au-row.md",
        "html-comment-hidden-main-row.md",
        "ignored-step-selftests.yml",
        "inverse-t6-w12-t1-w6-order.md",
        "inverted-t6-w15-t6-w12-edge.md",
        "kind-drift.md",
        "legacy-au-alias.md",
        "manifest-fork-accepted.md",
        "manifest-rollback-accepted.md",
        "manifest-witness-discontinuity-accepted.md",
        "missing-ack-token-scope.md",
        "missing-au-serial-edge-dag.md",
        "missing-canary-activation-field.md",
        "missing-canary-flag-scope.md",
        "missing-d7-before-d3-consumer.md",
        "missing-evidence-prerequisite-dag.md",
        "missing-idle-no-wake-scope.md",
        "missing-interlock-race-scope.md",
        "missing-journal-reconciler-scope.md",
        "missing-o-cfcancel.md",
        "missing-o-canary-activate-predecessor.md",
        "missing-o-cfrate-predecessor.md",
        "missing-o-pg-rearm-predecessor.md",
        "missing-page-ack-binding.md",
        "missing-pg-fence-scope.md",
        "missing-r2-before-t4-w2.md",
        "missing-r6-before-t5-w1.md",
        "missing-t8w4a-image-predecessor.md",
        "missing-t8w4b-worker-serialization.md",
        "missing-recovery-manifest-digest.md",
        "forbidden-t6-w1-workflow-scope.md",
        "missing-sensitivity-receipt-isolation.md",
        "missing-signer-manifest-binding.md",
        "missing-signer-trust-tuple-field.md",
        "missing-t0-w1-from-t3-w17.md",
        "missing-t1-w6-from-t6-w14.md",
        "missing-t1-w6-monitor-tuple-interlock.md",
        "missing-t3-w17-containment-evidence-scope.md",
        "t3-w17-contract-drift.md",
        "t3-w17-reservation-api-drift.md",
        "t3-w17-reservation-completion-reopen.md",
        "t3-w17-reservation-identity-drift.md",
        "t3-w17-reservation-order-drift.md",
        "t3-w17-reservation-reclaim-drift.md",
        "t3-w17-admit-unexpired-held-503.md",
        "t3-w17-admit-expired-held-atomic.md",
        "t3-w17-stale-owner-zero-effects.md",
        "t3-w17-completion-observed-latch.md",
        "t3-w17-completion-unobserved-tombstone.md",
        "t3-w17-repo-job-normalizer.md",
        "t3-w17-normalizer-before-mutation.md",
        "t3-w17-orphan-identity-fail-closed.md",
        "missing-t3-w18-from-t1-w5.md",
        "missing-t5-w1-before-t5-w4.md",
        "missing-t6-w12-from-t1-w6.md",
        "missing-t6-w14-before-t6-w10.md",
        "missing-t6-w14-bind-only.md",
        "missing-t6-w14-preregistration.md",
        "missing-t6-w15-before-t3-w16.md",
        "missing-t6-w15-before-t6-w12.md",
        "missing-t6-w15-outbox-scope.md",
        "missing-t6-w15-recovery-scope.md",
        "missing-trusted-clock-scope.md",
        "missing-window-journal-scope.md",
        "nested-glob-file-overlap-dag.md",
        "no-wake-artifact-wrong-owner.md",
        "no-wake-target.test.ts-t6w14-missing.md",
        "ocfrate-closed-interval.md",
        "ocfrate-domain-weakened.md",
        "ocfrate-failure-formula-weakened.md",
        "ocfrate-line-formula-weakened.md",
        "ocfrate-missing-threshold-proof-dag.md",
        "ocfrate-missing-threshold-proof-delta.md",
        "ocfrate-missing-threshold-proof-main.md",
        "ocfrate-missing-threshold-proof-triage.md",
        "ocfrate-non-provider-source.md",
        "ocfrate-observed-cost-formula-weakened.md",
        "ocfrate-owner-signature-weakened.md",
        "ocfrate-policy-id-only.md",
        "ocfrate-threshold-witness-weakened.md",
        "opaque-extra-a-row.md",
        "outbox-periodic-head.test-missing.md",
        "outbox-quarantine.test-missing.md",
        "outbox-transition-head.test-missing.md",
        "ownership-drift.md",
        "owner-auth-cross-doc-drift.md",
        "owner-auth-replay-allowed.md",
        "owner-auth-schema-weakened.md",
        "pg-unset-enables.md",
        "phantom-dag-wp.md",
        "phase1-12-acks.md",
        "phase1-12-requests.md",
        "phase1-12-ticks.md",
        "phase1-before-phase2.md",
        "phase1-unrelated-artifact-ordering.md",
        "phase2-20-transactions.md",
        "phase2-no-contamination.md",
        "phase2-probe-zero.md",
        "producer-test-cycle.md",
        "ready-set-fence-missing.md",
        "ready-set-fence-unclosed.md",
        "ready-set-fence-wrong-info.md",
        "ready-set-row-outside-fence.md",
        "relocated-page-ack-schema.md",
        "renamed-au-heading.md",
        "renamed-heading.md",
        "reset-array-selftests.yml",
        "selftests-head-blob-toctou",
        "selftests-mode-toctou",
        "selftests-non-100755",
        "selftests-nonregular",
        "selftests-nonregular-toctou",
        "selftests-symlink",
        "selftests-symlink-toctou",
        "retrospective-journal.md",
        "r6-cross-doc-drift.md",
        "r6-cross-tenant-read-allowed.md",
        "r6-schema-weakened.md",
        "rules.ts-t6w14-missing.md",
        "source-au-swap.md",
        "summary-corruption.md",
        "t1w4-missing-t6w10-predecessor.md",
        "t1w4-same-batch-as-t6w10.md",
        "t6w10-no-implementation.md",
        "t6w14-flags-zero-zero.md",
        "t6w14-forbidden-arming-delta.md",
        "t6w14-zero-action.md",
        "t6w1-missing-script-scope.md",
        "types.ts-t6w14-missing.md",
        "unstable-canary-activation.md",
        "wave2-reordered.md",
        "weakened-canary-activation-drift.md",
        "weakened-cf-rate-budget.md",
        "wrong-a610-owner.md",
        "wrong-ack-recovery-schema.md",
        "wrong-au-owner.md",
        "wrong-cf-rate-artifact.md",
        "wrong-page-ack-schema.md",
    }
)
blocked_mutation_inventory: set[str] = set()
mutation_fixture_paths: dict[str, Path] = {}


def record_mutation(name: str) -> None:
    """Record one distinct corrupted fixture after its negative check blocks."""

    if not name:
        raise AssertionError("mutation inventory entries must be non-empty")
    blocked_mutation_inventory.add(name)


def record_mutated_paths(*paths: Path | None) -> None:
    for path in paths:
        if path is not None and path.resolve() not in CANONICAL_INPUTS:
            resolved = path.resolve()
            previous = mutation_fixture_paths.setdefault(path.name, resolved)
            if previous != resolved:
                raise AssertionError(
                    f"mutation fixture basename collision for {path.name!r}: "
                    f"{previous} versus {resolved}"
                )
            record_mutation(path.name)


def execute(
    checker: str,
    target: Path,
    *,
    dag: Path | None = None,
    plan: Path | None = None,
    delta: Path | None = None,
    handoff: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    command = [sys.executable, str(PLAN_DIR / checker), str(target)]
    if checker == "au-check.py":
        command.extend(["--plan", str(plan or PLAN)])
        command.extend(["--delta", str(delta or STAGED)])
        command.extend(["--handoff", str(handoff or HANDOFF)])
        # The dispatch DAG is the current canonical repair input.  Keep this
        # compatible with the pre-DAG snapshot while using it whenever the
        # coordinated input is present (the evolved AU gate requires the
        # explicit path).
        supports_dag = False
        selected_dag = dag or DAG
        if selected_dag.exists():
            help_result = subprocess.run(
                [sys.executable, str(PLAN_DIR / checker), "--help"],
                cwd=REPO,
                check=False,
                capture_output=True,
                text=True,
            )
            supports_dag = "--dag" in help_result.stdout
        if supports_dag:
            command.extend(["--dag", str(selected_dag)])
    return subprocess.run(
        command,
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )


def require(
    label: str,
    checker: str,
    target: Path,
    should_pass: bool,
    *,
    dag: Path | None = None,
    plan: Path | None = None,
    delta: Path | None = None,
    handoff: Path | None = None,
) -> None:
    result = execute(checker, target, dag=dag, plan=plan, delta=delta, handoff=handoff)
    passed = result.returncode == 0
    if passed != should_pass:
        stream = result.stdout + result.stderr
        expectation = "PASS" if should_pass else "BLOCK"
        raise AssertionError(
            f"{label}: expected {expectation}, rc={result.returncode}\n{stream}"
        )
    if not should_pass:
        record_mutated_paths(target, dag, plan, delta, handoff)
    print(f"PASS {label}: {'accepted baseline' if should_pass else 'blocked mutation'}")


def replace_once(document: str, old: str, new: str, label: str) -> str:
    count = document.count(old)
    if count != 1:
        raise AssertionError(f"{label}: expected one mutation target, found {count}")
    return document.replace(old, new, 1)


def replace_regex_once(
    document: str, pattern: str, replacement: str, label: str
) -> str:
    """Replace one regex target while proving the fixture is physical/non-vacuous."""

    matches = list(re.finditer(pattern, document, re.IGNORECASE | re.DOTALL))
    if len(matches) != 1:
        raise AssertionError(
            f"{label}: expected one regex mutation target, found {len(matches)}"
        )
    start, end = matches[0].span()
    mutated = document[:start] + replacement + document[end:]
    if mutated == document:
        raise AssertionError(f"{label}: regex replacement was vacuous")
    return mutated


def fence_once(document: str, line: str, label: str) -> str:
    """Hide one canonical Markdown line in a fenced code block."""

    return replace_once(
        document,
        line,
        f"```markdown\n{line}\n```",
        label,
    )


def swap_once(document: str, first: str, second: str, label: str) -> str:
    """Swap two adjacent physical rows, asserting the fixture is unambiguous."""

    pair = f"{first}\n{second}"
    count = document.count(pair)
    if count != 1:
        raise AssertionError(f"{label}: expected one adjacent row pair, found {count}")
    return document.replace(pair, f"{second}\n{first}", 1)


def table_cell(document: str, row_prefix: str, label: str) -> str:
    """Return the final cell of one declaration row for a scope mutation."""

    rows = [line for line in document.splitlines() if line.startswith(row_prefix)]
    if len(rows) != 1:
        raise AssertionError(
            f"{label}: expected one declaration row, found {len(rows)}"
        )
    cells = rows[0].strip().strip("|").split("|")
    if len(cells) < 2 or not cells[-1].strip():
        raise AssertionError(f"{label}: declaration has no scope cell")
    return cells[-1].strip()


def replace_in_row(
    document: str, row_prefix: str, old: str, new: str, label: str
) -> str:
    """Replace one token in one physical Markdown row."""

    rows = [line for line in document.splitlines() if line.startswith(row_prefix)]
    if len(rows) != 1:
        raise AssertionError(f"{label}: expected one row, found {len(rows)}")
    row = rows[0]
    if row.count(old) != 1:
        raise AssertionError(
            f"{label}: expected one token in row, found {row.count(old)}"
        )
    return document.replace(row, row.replace(old, new, 1), 1)


def require_mirrored_wp(
    label: str,
    target: Path,
    should_pass: bool,
    mirror_root: Path,
    *,
    overrides: dict[Path, Path] | None = None,
    workflow: Path | None = None,
    handoff: Path | None = None,
    contract_digest_bypass: bool = False,
) -> None:
    """Run wp-check against an isolated plan directory.

    wp-check loads the staged and AU registries beside its own source file.
    A mirror lets this self-test corrupt one supplemental registry without
    mutating the checked-out fixtures or making the checker configurable via
    an untrusted environment variable.
    """

    mirror_plan = mirror_root / "docs" / "plan"
    mirror_plan.mkdir(parents=True)
    for source in PLAN_DIR.iterdir():
        if source.is_file() and source.suffix in {".md", ".py", ".txt", ".json"}:
            shutil.copy2(source, mirror_plan / source.name)
    contracts = PLAN_DIR / "contracts"
    if contracts.is_dir():
        shutil.copytree(contracts, mirror_plan / "contracts")
    for destination, source in (overrides or {}).items():
        try:
            relative_destination = destination.relative_to(PLAN_DIR)
        except ValueError:
            relative_destination = Path(destination.name)
        mirrored_destination = mirror_plan / relative_destination
        mirrored_destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, mirrored_destination)
    if contract_digest_bypass:
        contract_override = (overrides or {}).get(PLAN_DIR / "contracts" / "T3-W17.md")
        if contract_override is None:
            raise AssertionError(
                "contract digest bypass requires an overridden T3-W17 contract"
            )
        checker_path = mirror_plan / "wp-check.py"
        checker_text = checker_path.read_text(encoding="utf-8")
        mutated_digest = hashlib.sha256(contract_override.read_bytes()).hexdigest()
        checker_text, replacements = re.subn(
            r'(T3_W17_CONTRACT_SHA256\s*=\s*\(\s*")[0-9a-f]+(")',
            rf"\g<1>{mutated_digest}\g<2>",
            checker_text,
            count=1,
            flags=re.DOTALL,
        )
        if replacements != 1:
            raise AssertionError(
                "contract digest bypass could not update mirrored checker digest"
            )
        checker_path.write_text(checker_text, encoding="utf-8")
    mirror_workflows = mirror_root / ".github" / "workflows"
    mirror_workflows.mkdir(parents=True)
    shutil.copy2(
        workflow or REPO / ".github" / "workflows" / "selftests.yml",
        mirror_workflows / "selftests.yml",
    )
    mirror_handoff = mirror_root / "docs" / "handoff"
    mirror_handoff.mkdir(parents=True)
    shutil.copy2(handoff or HANDOFF, mirror_handoff / HANDOFF.name)
    mirrored_target = mirror_plan / target.name
    shutil.copy2(target, mirrored_target)
    result = subprocess.run(
        [sys.executable, str(mirror_plan / "wp-check.py"), str(mirrored_target)],
        cwd=mirror_root,
        check=False,
        capture_output=True,
        text=True,
    )
    passed = result.returncode == 0
    if passed != should_pass:
        stream = result.stdout + result.stderr
        expectation = "PASS" if should_pass else "BLOCK"
        raise AssertionError(
            f"{label}: expected {expectation}, rc={result.returncode}\n{stream}"
        )
    if not should_pass:
        record_mutated_paths(
            target,
            *(overrides or {}).values(),
            workflow,
            handoff,
        )
    print(f"PASS {label}: {'accepted baseline' if should_pass else 'blocked mutation'}")
    # Each mirrored gate invocation is self-contained. Keeping every mirror
    # until the outer TemporaryDirectory exits made the full negative suite
    # consume several GiB and fail with ENOSPC before reaching the later
    # mutations. Release it immediately after its verdict; the mutation source
    # and inventory record live outside this mirror and remain available.
    shutil.rmtree(mirror_root)


def require_actionlint_config_is_not_authoritative(work: Path) -> None:
    """Prove a suppress-all repository config cannot hide CI diagnostics.

    actionlint intentionally honors an ignore-all config when explicitly
    given one.  The workflow therefore runs its authoritative lint with
    ``-config-file /dev/null``.  This fixture proves both halves: the hostile
    config would suppress the diagnostics, while the trusted invocation still
    reports the unexpected label and syntax error.
    """

    actionlint = shutil.which("actionlint")
    if actionlint is None:
        raise AssertionError(
            "actionlint is required for the config fail-closed self-test"
        )

    workflow = work / "unexpected-actionlint.yml"
    workflow.write_text(
        "name: synthetic\non: push\njobs:\n  bad:\n    runs-on: corelink-unexpected\n    steps: []\n",
        encoding="utf-8",
    )
    hostile_config = work / "suppress-all-actionlint.yaml"
    hostile_config.write_text(
        'paths:\n  "**/*.yml":\n    ignore: [".*"]\n',
        encoding="utf-8",
    )

    suppressed = subprocess.run(
        [actionlint, "-config-file", str(hostile_config), "-oneline", str(workflow)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    if suppressed.returncode != 0:
        raise AssertionError(
            "actionlint suppress-all fixture did not exercise configuration suppression:\n"
            + suppressed.stdout
            + suppressed.stderr
        )

    trusted = subprocess.run(
        [actionlint, "-config-file", "/dev/null", "-oneline", str(workflow)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = trusted.stdout + trusted.stderr
    if trusted.returncode == 0 or "corelink-unexpected" not in diagnostics:
        raise AssertionError(
            "actionlint trusted invocation failed to block an unexpected runner label:\n"
            + diagnostics
        )
    if "[syntax-check]" not in diagnostics:
        raise AssertionError(
            "actionlint trusted invocation failed to retain syntax diagnostics:\n"
            + diagnostics
        )
    record_mutation("actionlint-unexpected-workflow")
    record_mutation("actionlint-suppress-all-config")
    print("PASS actionlint suppress-all config cannot hide trusted diagnostics")


def require_actionlint_exact_baseline(work: Path) -> None:
    """Prove that the executable actionlint baseline rejects an extra label.

    The authoritative checker owns an exact (workflow,label) multiset.  Copy
    the workflow tree into an isolated fixture, add one otherwise-valid job
    using an already-allowed ``corelink`` label, and require the count mismatch
    to fail.  This catches the historical regex-only false PASS.
    """

    checker = PLAN_DIR / "actionlint-check.py"
    if not checker.exists():
        raise AssertionError(f"missing authoritative actionlint checker: {checker}")

    fixture_root = work / "actionlint-exact-baseline"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)

    workflow = workflow_root / "ci.yml"
    workflow.write_text(
        workflow.read_text(encoding="utf-8")
        + "\n  actionlint_baseline_mutation:\n"
        + "    runs-on: corelink\n"
        + "    steps:\n"
        + "      - run: true\n",
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "baseline mismatch" not in diagnostics:
        raise AssertionError(
            "actionlint exact-baseline checker accepted an extra allowed label:\n"
            + diagnostics
        )
    record_mutation("actionlint-extra-allowed-label")
    print("PASS actionlint exact baseline blocks an extra allowed-label occurrence")


def require_actionlint_dynamic_runner_rejected(work: Path) -> None:
    """Prove an expression-resolved unapproved runner cannot evade diagnostics."""

    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-dynamic-runner"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "corelink-stress.yml"
    source = workflow.read_text(encoding="utf-8")
    workflow.write_text(
        replace_once(
            source,
            "    runs-on: ubuntu-latest",
            "    runs-on: \"${{ 'corelink-unexpected' }}\"",
            "dynamic runs-on expression",
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "unapproved runs-on binding" not in diagnostics:
        raise AssertionError(
            "actionlint checker accepted an expression-resolved unapproved runner:\n"
            + diagnostics
        )
    record_mutation("actionlint-dynamic-runner-expression")
    print("PASS actionlint blocks an expression-resolved unapproved runner")


def require_actionlint_inline_runner_rejected(work: Path) -> None:
    """Prove an inline YAML job mapping cannot hide an unapproved runner."""

    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-inline-runner"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "ci.yml"
    workflow.write_text(
        workflow.read_text(encoding="utf-8")
        + "\n  inline_runner_mutation: {runs-on: corelink-unexpected, steps: []}\n",
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "unapproved runs-on binding" not in diagnostics:
        raise AssertionError(
            "actionlint checker accepted an inline unapproved runner mapping:\n"
            + diagnostics
        )
    record_mutation("actionlint-inline-runs-on-mapping")
    print("PASS actionlint blocks an inline unapproved runner mapping")


def require_actionlint_escaped_runner_rejected(work: Path) -> None:
    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-escaped-runner"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "ci.yml"
    workflow.write_text(
        workflow.read_text(encoding="utf-8")
        + '\n  escaped_runner_mutation: {"\\u0072uns-on": corelink-unexpected, steps: []}\n',
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "unapproved runs-on binding" not in diagnostics:
        raise AssertionError("actionlint checker accepted an escaped runs-on key:\n" + diagnostics)
    record_mutation("actionlint-inline-escaped-runner")
    print("PASS actionlint blocks an escaped inline runner key")


def require_actionlint_plan_structure_rejected(work: Path) -> None:
    checker = PLAN_DIR / "actionlint-check.py"
    mutations = (
        (
            "actionlint-plan-structure-duplicate-job",
            "\n  check:\n    name: duplicate\n",
        ),
        (
            "actionlint-plan-structure-duplicate-step",
            "      - name: Lint workflow syntax\n        run: true\n",
        ),
    )
    for mutation, addition in mutations:
        fixture_root = work / mutation
        workflow_root = fixture_root / ".github" / "workflows"
        shutil.copytree(REPO / ".github" / "workflows", workflow_root)
        workflow = workflow_root / "plan-integrity.yml"
        source = workflow.read_text(encoding="utf-8")
        if "duplicate-step" in mutation:
            source = replace_once(
                source,
                "      - name: Exercise structural planning gates\n",
                addition + "      - name: Exercise structural planning gates\n",
                mutation,
            )
        else:
            source += addition
        workflow.write_text(source, encoding="utf-8")
        result = subprocess.run(
            [sys.executable, str(checker), "--root", str(fixture_root)],
            cwd=REPO,
            check=False,
            capture_output=True,
            text=True,
        )
        diagnostics = result.stdout + result.stderr
        if result.returncode == 0 or "plan-integrity SHA binding mismatch" not in diagnostics:
            raise AssertionError(f"actionlint checker accepted {mutation}:\n{diagnostics}")
        record_mutation(mutation)
        print(f"PASS actionlint blocks {mutation}")


def require_actionlint_heredoc_copy_rejected(work: Path) -> None:
    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-heredoc-copy"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "plan-integrity.yml"
    source = workflow.read_text(encoding="utf-8")
    actual_sha = '          ACTUAL_SHA="$(git rev-parse --verify HEAD)"\n'
    if source.count(actual_sha) != 2:
        raise AssertionError("heredoc SHA copy: expected two capture commands")
    source = source.replace(
        actual_sha,
        "          cat <<'EOF'\n"
        '          ACTUAL_SHA="$(git rev-parse --verify HEAD)"\n'
        "          EOF\n",
        1,
    )
    source = source.replace(
        actual_sha,
        "          echo 'ACTUAL_SHA=\"$(git rev-parse --verify HEAD)\"'\n",
        1,
    )
    workflow.write_text(source, encoding="utf-8")
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "plan-integrity SHA binding mismatch" not in diagnostics:
        raise AssertionError("actionlint checker accepted heredoc/string SHA copies:\n" + diagnostics)
    record_mutation("actionlint-sha-heredoc-string-copy")
    print("PASS actionlint excludes heredoc/string SHA copies")


def require_actionlint_runner_lexing_controls(work: Path) -> None:
    checker_spec = importlib.util.spec_from_file_location(
        "actionlint_check", PLAN_DIR / "actionlint-check.py"
    )
    if checker_spec is None or checker_spec.loader is None:
        raise AssertionError("could not load actionlint checker for lexer controls")
    checker_module = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker_module)
    workflow = work / "runner-lexing-controls.yml"
    controls = (
        "name: controls\n"
        "jobs:\n"
        "  safe:\n"
        "    runs-on: abc#frag\n"
        "    steps: []\n"
        "  url:\n"
        "    runs-on: ubuntu-latest # https://example.test/runs-on:foo\n"
        "    steps: []\n"
    )
    workflow.write_text(controls, encoding="utf-8")
    errors = checker_module.validate_runs_on_bindings(workflow)
    if not any("runs-on value cannot be statically proven" in error for error in errors):
        raise AssertionError("runner checker accepted abc#frag as a static label")
    url_only = work / "runner-url-only.yml"
    url_only.write_text("name: url\nmeta: https://example.test/runs-on:foo\n", encoding="utf-8")
    if checker_module.validate_runs_on_bindings(url_only):
        raise AssertionError("runner checker treated URL scalar text as a runs-on key")
    print("PASS actionlint runner-key lexing controls")


def require_actionlint_commented_dead_sha_rejected(work: Path) -> None:
    """Prove comments/dead shell copies cannot satisfy SHA guard requirements."""

    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-commented-dead-sha"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "plan-integrity.yml"
    source = workflow.read_text(encoding="utf-8")
    source = source.replace(
        "          EXPECTED_SHA: ${{ github.sha }}",
        "          # EXPECTED_SHA: ${{ github.sha }}",
    )
    source = source.replace(
        "          set -euo pipefail\n",
        "          if false; then\n          set -euo pipefail\n",
        2,
    )
    source = source.replace("          esac\n", "          esac\n          fi\n", 1)
    source = source.replace(
        "          printf '### Plan integrity completion\\n\\n%s\\n' \"$RECORD\" >> \"$GITHUB_STEP_SUMMARY\"\n",
        "          printf '### Plan integrity completion\\n\\n%s\\n' \"$RECORD\" >> \"$GITHUB_STEP_SUMMARY\"\n          fi\n",
    )
    source = source.replace(
        "        if: ${{ success() }}",
        "        # if: ${{ success() }}",
    )
    workflow.write_text(source, encoding="utf-8")
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if result.returncode == 0 or "plan-integrity SHA binding mismatch" not in diagnostics:
        raise AssertionError(
            "actionlint checker accepted commented/dead SHA guards:\n" + diagnostics
        )
    record_mutation("actionlint-commented-dead-sha-binding")
    print("PASS actionlint blocks commented/dead SHA guard copies")


def require_actionlint_sha_binding_rejected(work: Path) -> None:
    """Prove the authoritative lint gate rejects weakened event-SHA binding."""

    checker = PLAN_DIR / "actionlint-check.py"
    fixture_root = work / "actionlint-sha-binding"
    workflow_root = fixture_root / ".github" / "workflows"
    shutil.copytree(REPO / ".github" / "workflows", workflow_root)
    workflow = workflow_root / "plan-integrity.yml"
    source = workflow.read_text(encoding="utf-8")
    workflow.write_text(
        replace_once(
            source,
            ' || "$SECOND_PARENT" != "$PR_HEAD_SHA"',
            "",
            "PR head SHA binding",
        ),
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(checker), "--root", str(fixture_root)],
        cwd=REPO,
        check=False,
        capture_output=True,
        text=True,
    )
    diagnostics = result.stdout + result.stderr
    if (
        result.returncode == 0
        or "plan-integrity SHA binding mismatch" not in diagnostics
    ):
        raise AssertionError(
            "actionlint checker accepted weakened PR head-SHA binding:\n" + diagnostics
        )
    record_mutation("actionlint-weakened-sha-binding")
    print("PASS actionlint blocks weakened plan-integrity SHA binding")


def require_tracked_selftest_guard_fixtures(work: Path) -> None:
    """Prove tracked selftests remain executable and immutable until launch.

    The workflow's first pass checks the index, while its discovery loop checks
    the same mode/type/blob immediately before each invocation. Keep both
    checks exercised: malformed index entries are rejected, and a first
    selftest cannot tamper with the next one before it runs.
    """

    workflow = REPO / ".github" / "workflows" / "selftests.yml"
    workflow_text = workflow.read_text(encoding="utf-8")
    start = workflow_text.index(
        "          set -e -u -o pipefail\n",
        workflow_text.index("- name: Validate tracked selftest files"),
    )
    discover_start = workflow_text.index(
        "          set -euo pipefail\n",
        workflow_text.index("- name: Discover and run every tracked selftest"),
    )
    validate_end = workflow_text.index(
        "\n      - name: Discover and run every tracked selftest", start
    )

    def script_from(start: int, end: int) -> str:
        return "\n".join(
            line[10:] if line.startswith("          ") else line
            for line in workflow_text[start:end].splitlines()
        ) + "\n"

    validate_script = script_from(start, validate_end)
    discover_script = script_from(discover_start, len(workflow_text))

    def git(root: Path, *args: str) -> str:
        result = subprocess.run(
            ["git", *args],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        )
        return result.stdout.strip()

    def base_fixture(name: str) -> tuple[Path, Path]:
        root = work / name
        scripts = root / "scripts"
        scripts.mkdir(parents=True)
        selftest = scripts / "fixture.selftest.sh"
        selftest.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
        selftest.chmod(0o755)
        git(root, "init", "-q")
        git(root, "config", "user.email", "fixture@example.test")
        git(root, "config", "user.name", "fixture")
        return root, selftest

    def invalid_fixture(name: str, kind: str) -> tuple[Path, Path]:
        root, selftest = base_fixture(name)
        if kind == "mode":
            selftest.chmod(0o644)
        elif kind == "symlink":
            target = selftest.parent / "target.sh"
            target.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
            selftest.unlink()
            selftest.symlink_to(target.name)
        else:
            child = root / "child-repo"
            child.mkdir()
            git(child, "init", "-q")
            git(child, "config", "user.email", "fixture@example.test")
            git(child, "config", "user.name", "fixture")
            (child / "README").write_text("fixture\n", encoding="utf-8")
            git(child, "add", "README")
            git(child, "commit", "-qm", "fixture")
            selftest.unlink()
            git(root, "update-index", "--add", "--cacheinfo", f"160000,{git(child, 'rev-parse', 'HEAD')},scripts/fixture.selftest.sh")
        if kind != "nonregular":
            git(root, "add", "scripts")
        git(root, "commit", "-qm", "fixture")
        return root, selftest

    def run_script(root: Path, script: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["/usr/local/bin/bash", "-c", script],
            cwd=root,
            env=os.environ.copy(),
            check=False,
            capture_output=True,
            text=True,
        )

    def toctou_fixture(name: str, replacement: str) -> Path:
        root = work / name
        scripts = root / "scripts"
        scripts.mkdir(parents=True)
        first = scripts / "a-first.selftest.sh"
        second = scripts / "b-second.selftest.sh"
        commands = {
            "content": "printf '#!/usr/bin/env bash\\nexit 7\\n' > scripts/b-second.selftest.sh",
            "mode": "chmod 644 scripts/b-second.selftest.sh",
            "symlink": "rm scripts/b-second.selftest.sh && ln -s missing-target.sh scripts/b-second.selftest.sh",
            "nonregular": "rm scripts/b-second.selftest.sh && mkdir scripts/b-second.selftest.sh",
        }
        first.write_text(
            "#!/usr/bin/env bash\n"
            f"{commands[replacement]}\n",
            encoding="utf-8",
        )
        second.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
        first.chmod(0o755)
        second.chmod(0o755)
        git(root, "init", "-q")
        git(root, "config", "user.email", "fixture@example.test")
        git(root, "config", "user.name", "fixture")
        git(root, "add", "scripts")
        git(root, "commit", "-qm", "fixture")
        return root

    valid_root, _ = base_fixture("selftests-valid")
    git(valid_root, "add", "scripts")
    git(valid_root, "commit", "-qm", "fixture")
    if run_script(valid_root, validate_script).returncode != 0:
        raise AssertionError("tracked selftest guard rejected its valid executable fixture")
    local_discover_script = discover_script.replace("/usr/bin/bash", "/usr/local/bin/bash")
    if run_script(valid_root, local_discover_script).returncode != 0:
        raise AssertionError("tracked selftest discovery rejected its valid executable fixture")
    print("PASS tracked selftest guard accepts a valid executable")

    for mutation, kind in (
        ("selftests-non-100755", "mode"),
        ("selftests-symlink", "symlink"),
        ("selftests-nonregular", "nonregular"),
    ):
        root, _ = invalid_fixture(mutation, kind)
        result = run_script(root, validate_script)
        if result.returncode == 0:
            raise AssertionError(f"tracked selftest guard accepted {mutation}")
        record_mutation(mutation)
        print(f"PASS tracked selftest guard blocks {mutation}")

    for mutation, replacement in (
        ("selftests-head-blob-toctou", "content"),
        ("selftests-mode-toctou", "mode"),
        ("selftests-symlink-toctou", "symlink"),
        ("selftests-nonregular-toctou", "nonregular"),
    ):
        root = toctou_fixture(mutation, replacement)
        if run_script(root, validate_script).returncode != 0:
            raise AssertionError(f"could not prepare discovery for {mutation}")
        if run_script(root, local_discover_script).returncode == 0:
            raise AssertionError(f"tracked selftest discovery accepted {mutation}")
        record_mutation(mutation)
        print(f"PASS tracked selftest discovery blocks {mutation}")


def _obsolete_tracked_selftest_guard_fixture(work: Path) -> None:
    """Retained only as a review anchor; use the inline guard above."""
    raise AssertionError("obsolete helper must not be called")


def _removed_guard_fixture_tail() -> None:
    return None


def main() -> int:
    require("plan baseline", "plan-check.py", SOURCE, True)
    require("WP baseline", "wp-check.py", PLAN, True)
    require("AU staging baseline", "au-check.py", TRIAGE, True)

    source = SOURCE.read_text(encoding="utf-8")
    plan = PLAN.read_text(encoding="utf-8")
    triage = TRIAGE.read_text(encoding="utf-8")
    staged = STAGED.read_text(encoding="utf-8")
    handoff = HANDOFF.read_text(encoding="utf-8")

    with tempfile.TemporaryDirectory(prefix="corelink-plan-gates-") as temp:
        work = Path(temp)

        require_actionlint_config_is_not_authoritative(work)
        require_actionlint_exact_baseline(work)
        require_actionlint_dynamic_runner_rejected(work)
        require_actionlint_inline_runner_rejected(work)
        require_actionlint_escaped_runner_rejected(work)
        require_actionlint_runner_lexing_controls(work)
        require_actionlint_plan_structure_rejected(work)
        require_actionlint_commented_dead_sha_rejected(work)
        require_actionlint_heredoc_copy_rejected(work)
        require_actionlint_sha_binding_rejected(work)
        require_tracked_selftest_guard_fixtures(work)

        duplicate_source = work / "duplicate-source.txt"
        first_id = source.splitlines()[0]
        duplicate_source.write_text(source + first_id + "\n", encoding="utf-8")
        require(
            "plan duplicate physical source row",
            "plan-check.py",
            duplicate_source,
            False,
        )

        duplicate_a = work / "duplicate-a-row.md"
        a1_line = next(
            line for line in plan.splitlines() if line.startswith("| A1.1 |")
        )
        duplicate_a.write_text(
            plan.replace(a1_line, f"{a1_line}\n{a1_line}", 1), encoding="utf-8"
        )
        require("WP duplicate physical A row", "wp-check.py", duplicate_a, False)

        kind_drift = work / "kind-drift.md"
        kind_drift.write_text(
            replace_once(
                plan,
                "| A1.1 | probe |",
                "| A1.1 | test |",
                "A kind drift",
            ),
            encoding="utf-8",
        )
        require("WP frozen A kind drift", "wp-check.py", kind_drift, False)

        opaque_extra_a = work / "opaque-extra-a-row.md"
        opaque_extra_a.write_text(
            replace_once(
                plan,
                "| A1.1 | probe | `GET /health` returns 200 |",
                "| A1.1 | probe | `GET /health` returns 200 |\n| A99.99 (opaque) | test | opaque extra acceptance row |",
                "opaque extra acceptance row",
            ),
            encoding="utf-8",
        )
        require("WP opaque extra acceptance row", "wp-check.py", opaque_extra_a, False)

        wave2_reorder = work / "wave2-reordered.md"
        wave2_row_1 = "| 1 | **T4-W1** | A4.1 | `index.ts` |"
        wave2_row_2 = "| 2 | **T4-W2** | A4.2 A4.3 A4.6 | `index.ts` + `lib.ts` |"
        wave2_reorder.write_text(
            swap_once(plan, wave2_row_1, wave2_row_2, "Wave-2 reorder"),
            encoding="utf-8",
        )
        require("WP Wave-2 serial reorder", "wp-check.py", wave2_reorder, False)

        ownership_drift = work / "ownership-drift.md"
        ownership_drift.write_text(
            replace_once(
                plan,
                "| **T1-W2** | A1.1 A1.2 A1.3 A1.5 |",
                "| **T1-W2** | A1.2 A1.3 A1.5 |",
                "WP ownership drift",
            ),
            encoding="utf-8",
        )
        require(
            "WP Markdown/catalog ownership drift", "wp-check.py", ownership_drift, False
        )

        renamed_heading = work / "renamed-heading.md"
        renamed_heading.write_text(
            replace_once(
                plan,
                "## 3. The acceptance suite (the completeness anchor)",
                "## 3. The acceptance suite (the completeness anchor) RENAMED",
                "WP exact heading",
            ),
            encoding="utf-8",
        )
        require("WP renamed canonical heading", "wp-check.py", renamed_heading, False)

        summary_corruption = work / "summary-corruption.md"
        summary_corruption.write_text(
            replace_once(
                plan,
                "**94 rows —",
                "**95 rows —",
                "summary count",
            ),
            encoding="utf-8",
        )
        require(
            "WP acceptance summary corruption", "wp-check.py", summary_corruption, False
        )

        header_corruption = work / "header-corruption.md"
        header_corruption.write_text(
            replace_once(
                plan,
                "| # | WP | owns | scope |",
                "| # | WP | owns | wrong-header |",
                "Wave-2 header",
            ),
            encoding="utf-8",
        )
        require("WP Wave-2 header corruption", "wp-check.py", header_corruption, False)

        capability_corruption = work / "capability-corruption.md"
        capability_corruption.write_text(
            replace_once(
                plan,
                "### C1 — control plane",
                "### C1 — corrupted capability",
                "capability heading",
            ),
            encoding="utf-8",
        )
        require(
            "WP capability partition corruption",
            "wp-check.py",
            capability_corruption,
            False,
        )

        hidden_main_row = work / "html-comment-hidden-main-row.md"
        main_row = next(
            line
            for line in plan.splitlines()
            if re.match(r"^\|\s+(?:\*\*)?A1\.1(?:\*\*)?\s+\|", line)
        )
        hidden_main_row.write_text(
            plan.replace(main_row, f"<!-- {main_row} -->", 1), encoding="utf-8"
        )
        require(
            "WP HTML-comment-hidden acceptance row",
            "wp-check.py",
            hidden_main_row,
            False,
        )

        fenced_main_row = work / "fenced-main-acceptance-row.md"
        fenced_main_row.write_text(
            fence_once(plan, main_row, "fenced principal acceptance row"),
            encoding="utf-8",
        )
        require(
            "WP fenced principal acceptance row",
            "wp-check.py",
            fenced_main_row,
            False,
        )

        fenced_capability = work / "fenced-capability-heading.md"
        capability_heading = "### C1 — control plane"
        fenced_capability.write_text(
            fence_once(plan, capability_heading, "fenced capability heading"),
            encoding="utf-8",
        )
        require(
            "WP fenced capability partition",
            "wp-check.py",
            fenced_capability,
            False,
        )

        fenced_staged_registry = work / "fenced-staged-principal-registry.md"
        staged_registry_row = next(
            line for line in staged.splitlines() if line.startswith("| new **T1-W5** |")
        )
        fenced_staged_registry.write_text(
            fence_once(staged, staged_registry_row, "fenced staged principal WP row"),
            encoding="utf-8",
        )
        require_mirrored_wp(
            "WP fenced staged principal registry row",
            PLAN,
            False,
            work / "staged-registry-mirror",
            overrides={STAGED: fenced_staged_registry},
        )

        deleted_t4w3_dependency = work / "deleted-t4-w3-dependency.md"
        if DAG.exists():
            deleted_t4w3_dag = work / "deleted-t4-w3-dependency-dag.md"
            deleted_t4w3_dag.write_text(
                replace_once(
                    DAG.read_text(encoding="utf-8"),
                    "| T3-W10 | W1 serial | T3-W4, T4-W4 |",
                    "| T3-W10 | W1 serial | T3-W4, T4-W3 |",
                    "deleted T4-W3 dependency",
                ),
                encoding="utf-8",
            )
            deleted_t4w3_dependency = deleted_t4w3_dag
            deleted_t4w3_target = TRIAGE
        else:
            deleted_t4w3_dependency.write_text(
                replace_in_row(
                    triage,
                    "| union-10 |",
                    "T3-W4 → T4-W4",
                    "T3-W4 → T4-W3",
                    "deleted T4-W3 dependency",
                ),
                encoding="utf-8",
            )
            deleted_t4w3_target = deleted_t4w3_dependency
        require(
            "AU deleted T4-W3 dependency",
            "au-check.py",
            deleted_t4w3_target,
            False,
            dag=deleted_t4w3_dependency if DAG.exists() else None,
        )

        source_au_swap = work / "source-au-swap.md"
        source_au_swap.write_text(
            replace_once(
                triage,
                "| union-06 |",
                "| AU4.16a |",
                "source to AU swap",
            ),
            encoding="utf-8",
        )
        require("AU source→AU swap", "au-check.py", source_au_swap, False)

        au_bucket_drift = work / "au-bucket-drift.md"
        au_bucket_drift.write_text(
            replace_once(
                triage,
                "| union-09 | M7 | MEDIUM | W1-parallel + W3-live-proof |",
                "| union-09 | M7 | MEDIUM | W2-serial-worker + W3-live-proof |",
                "AU bucket drift",
            ),
            encoding="utf-8",
        )
        require("AU bucket drift", "au-check.py", au_bucket_drift, False)

        hidden_au_row = work / "html-comment-hidden-au-row.md"
        au_row = next(
            line for line in triage.splitlines() if line.startswith("| union-06 |")
        )
        hidden_au_row.write_text(
            triage.replace(au_row, f"<!-- {au_row} -->", 1), encoding="utf-8"
        )
        require(
            "AU HTML-comment-hidden placement row",
            "au-check.py",
            hidden_au_row,
            False,
        )

        fenced_au_row = work / "fenced-au-placement-row.md"
        fenced_au_row.write_text(
            fence_once(triage, au_row, "fenced AU placement row"),
            encoding="utf-8",
        )
        require(
            "AU fenced placement row",
            "au-check.py",
            fenced_au_row,
            False,
        )

        au_invalid_kind = work / "au-invalid-kind.md"
        au_invalid_kind.write_text(
            replace_once(
                triage,
                "**AU3.19 — test:**",
                "**AU3.19 — invalid-kind:**",
                "AU invalid kind",
            ),
            encoding="utf-8",
        )
        require("AU invalid kind", "au-check.py", au_invalid_kind, False)

        legacy_alias = work / "legacy-au-alias.md"
        legacy_alias.write_text(triage.replace("T3-W14", "T3-W8"), encoding="utf-8")
        require("AU legacy WP alias collision", "au-check.py", legacy_alias, False)

        renamed_au_heading = work / "renamed-au-heading.md"
        renamed_au_heading.write_text(
            replace_once(
                triage,
                "## 2. Per-finding placement",
                "## 2. Per-finding placement RENAMED",
                "AU exact heading",
            ),
            encoding="utf-8",
        )
        require(
            "AU renamed canonical heading", "au-check.py", renamed_au_heading, False
        )

        missing_serial = work / "missing-au-serial-edge.md"
        if DAG.exists():
            missing_serial_dag = work / "missing-au-serial-edge-dag.md"
            missing_serial_dag.write_text(
                replace_once(
                    DAG.read_text(encoding="utf-8"),
                    "| T5-W3 | W1 serial | T5-W2 |",
                    "| T5-W3 | W1 serial | T7-W1 |",
                    "AU serial edge",
                ),
                encoding="utf-8",
            )
            missing_serial_target = TRIAGE
            missing_serial_dag_override = missing_serial_dag
        else:
            serial_edge = None
            for candidate in (
                "`T5-W2` → `T5-W3` → `T5-W6` is serial",
                "`T5-W2` → `T5-W3` is serial",
            ):
                if candidate in triage:
                    serial_edge = candidate
                    break
            if serial_edge is None:
                raise AssertionError(
                    "AU serial edge prose: no canonical serial declaration found"
                )
            without_edge = replace_once(
                triage,
                serial_edge,
                "`T5-W2` / `T5-W3` / `T5-W6` share scope",
                "AU serial edge prose",
            )
            without_edge = without_edge.replace(" (serial after T5-W2)", "", 1)
            missing_serial.write_text(without_edge, encoding="utf-8")
            missing_serial_target = missing_serial
            missing_serial_dag_override = None
        require(
            "AU missing shared-scope serial edge",
            "au-check.py",
            missing_serial_target,
            False,
            dag=missing_serial_dag_override,
        )

        scope_overlap = work / "au-scope-overlap.md"
        t8w4_scope = table_cell(triage, "| **T8-W4a**", "AU parallel scope overlap")
        t7w4_scope = table_cell(triage, "| **T7-W4**", "AU parallel scope overlap")
        scope_overlap.write_text(
            replace_once(
                triage,
                t8w4_scope,
                t7w4_scope,
                "AU parallel scope overlap",
            ),
            encoding="utf-8",
        )
        require("AU parallel scope overlap", "au-check.py", scope_overlap, False)

        if DAG.exists():
            dag_text = DAG.read_text(encoding="utf-8")

            phantom_dag = work / "phantom-dag-wp.md"
            phantom_anchor = (
                "| T0-W1 | W0 unblock | — | `docs/plan/union-catalog-ledger.md`; — | "
                "Luna / mechanical |"
            )
            phantom_dag.write_text(
                replace_once(
                    dag_text,
                    phantom_anchor,
                    "| T99-W1 | W1 parallel | — | `docs/plan/phantom.md`; — | Luna / test |\n"
                    + phantom_anchor,
                    "phantom DAG WP",
                ),
                encoding="utf-8",
            )
            require(
                "AU phantom DAG WP",
                "au-check.py",
                TRIAGE,
                False,
                dag=phantom_dag,
            )

            nested_scope = work / "nested-glob-file-overlap-dag.md"
            t7w2_line = next(
                line for line in dag_text.splitlines() if line.startswith("| T7-W2 |")
            )
            t7w2_scope = t7w2_line.split("|")[4].strip()
            docs_glob = "`docs/**`"
            docs_glob_start = t7w2_scope.index(docs_glob)
            docs_glob_end = t7w2_scope.index(";", docs_glob_start)
            broad_scope = (
                t7w2_scope[:docs_glob_start] + docs_glob + t7w2_scope[docs_glob_end:]
            )
            nested_scope.write_text(
                replace_once(
                    dag_text,
                    t7w2_line,
                    t7w2_line.replace(t7w2_scope, broad_scope, 1),
                    "nested glob/file overlap",
                ),
                encoding="utf-8",
            )
            require(
                "AU nested glob/file scope overlap",
                "au-check.py",
                TRIAGE,
                False,
                dag=nested_scope,
            )

            missing_evidence = work / "missing-evidence-prerequisite-dag.md"
            missing_evidence.write_text(
                replace_once(
                    dag_text,
                    "| T8-W7 | W3 live proof | T8-W4a, T2-W2a, T2-W2b, T2-W4, T1-W6, T7-W4b |",
                    "| T8-W7 | W3 live proof | T8-W4a, T2-W2a, T2-W2b, T2-W4, T1-W6 |",
                    "missing evidence gate/prerequisite",
                ),
                encoding="utf-8",
            )
            require(
                "AU missing evidence gate/prerequisite",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_evidence,
            )

            missing_jit_image = work / "missing-t8w4a-image-predecessor.md"
            missing_jit_image.write_text(
                replace_once(
                    dag_text,
                    "| T2-W2a | W0 unblock | T2-W1a, T8-W4a |",
                    "| T2-W2a | W0 unblock | T2-W1a |",
                    "missing T8-W4a image predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU missing JIT image predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_jit_image,
            )

            missing_worker_serial = work / "missing-t8w4b-worker-serialization.md"
            missing_worker_serial.write_text(
                replace_once(
                    dag_text,
                    "| T4-W1 | W2 worker | T3-W18, T8-W4b, D13 |",
                    "| T4-W1 | W2 worker | T3-W18, D13 |",
                    "missing T8-W4b worker serialization",
                ),
                encoding="utf-8",
            )
            require(
                "AU missing auth bridge worker serialization",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_worker_serial,
            )

            dag_row = next(
                line for line in dag_text.splitlines() if line.startswith("| T0-W1 |")
            )
            fenced_dag = work / "fenced-dag-table-row.md"
            fenced_dag.write_text(
                fence_once(dag_text, dag_row, "fenced DAG table row"),
                encoding="utf-8",
            )
            require(
                "AU fenced canonical DAG table row",
                "au-check.py",
                TRIAGE,
                False,
                dag=fenced_dag,
            )

            # Ready-set proofs are executable registry evidence, not free-form
            # prose.  Keep the negative fixtures here so a parser cannot accept
            # a BNN row merely because it appears somewhere in the document.
            ready_open = "```text\n"
            first_batch = next(
                line for line in dag_text.splitlines() if line.startswith("B00:")
            )

            outside_ready_set = work / "ready-set-row-outside-fence.md"
            outside_ready_set.write_text(
                replace_once(
                    dag_text,
                    ready_open + first_batch + "\n",
                    first_batch + "\n\n" + ready_open,
                    "ready-set row outside fence",
                ),
                encoding="utf-8",
            )
            require(
                "AU ready-set row outside canonical fence",
                "au-check.py",
                TRIAGE,
                False,
                dag=outside_ready_set,
            )
            require_mirrored_wp(
                "WP ready-set row outside canonical fence",
                PLAN,
                False,
                work / "ready-set-outside-wp-mirror",
                overrides={DAG: outside_ready_set},
            )

            missing_ready_fence = work / "ready-set-fence-missing.md"
            missing_ready_fence.write_text(
                replace_once(
                    dag_text,
                    ready_open,
                    "",
                    "missing ready-set opening fence",
                ),
                encoding="utf-8",
            )
            require(
                "AU ready-set opening fence missing",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_ready_fence,
            )

            wrong_ready_fence = work / "ready-set-fence-wrong-info.md"
            wrong_ready_fence.write_text(
                replace_once(
                    dag_text,
                    ready_open,
                    "```markdown\n",
                    "wrong ready-set fence info string",
                ),
                encoding="utf-8",
            )
            require(
                "AU ready-set fence has wrong info string",
                "au-check.py",
                TRIAGE,
                False,
                dag=wrong_ready_fence,
            )

            missing_ready_close = work / "ready-set-fence-unclosed.md"
            missing_ready_close.write_text(
                replace_once(
                    dag_text,
                    "\n```\n\nThe checker validates",
                    "\n\nThe checker validates",
                    "missing ready-set closing fence",
                ),
                encoding="utf-8",
            )
            require(
                "AU ready-set closing fence missing",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_ready_close,
            )

            t6w12_row = next(
                line for line in dag_text.splitlines() if line.startswith("| T6-W12 |")
            )
            missing_monitor_before_provider = work / "missing-t6-w15-before-t6-w12.md"
            missing_monitor_before_provider.write_text(
                replace_once(
                    dag_text,
                    t6w12_row,
                    t6w12_row.replace("T6-W15, ", "", 1),
                    "T6-W15 to T6-W12 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU missing T6-W15 predecessor before T6-W12",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_monitor_before_provider,
            )

            t6w15_row = next(
                line for line in dag_text.splitlines() if line.startswith("| T6-W15 |")
            )
            inverted_monitor_edge = work / "inverted-t6-w15-t6-w12-edge.md"
            inverted_monitor_edge_text = replace_once(
                dag_text,
                t6w12_row,
                t6w12_row.replace("T6-W15, ", "", 1),
                "inverted T6-W15/T6-W12 edge removal",
            )
            inverted_monitor_edge.write_text(
                replace_once(
                    inverted_monitor_edge_text,
                    t6w15_row,
                    t6w15_row.replace("T6-W4, ", "T6-W4, T6-W12, ", 1),
                    "inverted T6-W15/T6-W12 edge insertion",
                ),
                encoding="utf-8",
            )
            require(
                "AU inverted T6-W15/T6-W12 dependency",
                "au-check.py",
                TRIAGE,
                False,
                dag=inverted_monitor_edge,
            )

            wrong_a610_owner = work / "wrong-a610-owner.md"
            wrong_a610_owner.write_text(
                replace_once(
                    plan,
                    "| **T6-W15** | A6.10 |",
                    "| **T6-W15** | A6.9 |",
                    "A6.10 ownership",
                ),
                encoding="utf-8",
            )
            require(
                "WP A6.10 outside T6-W15 owner",
                "wp-check.py",
                wrong_a610_owner,
                False,
            )

            missing_outbox_scope = work / "missing-t6-w15-outbox-scope.md"
            missing_outbox_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/src/outbox.ts`; ",
                    "",
                    "T6-W15 outbox scope",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W15 missing outbox scope",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_outbox_scope,
            )

            missing_recovery_scope = work / "missing-t6-w15-recovery-scope.md"
            missing_recovery_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/test/outbox-recovery.test.ts`; ",
                    "",
                    "T6-W15 recovery scope",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W15 missing outbox recovery scope",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_recovery_scope,
            )

            # T6-W15's ordered outbox contract has three distinct safety
            # surfaces: immutable-transition HOL handling, periodic-observation
            # HOL handling, and fail-closed quarantine.  Each path is required
            # independently; dropping any one must not leave a false PASS.
            for path, label in (
                (
                    "deploy/cost-monitor/test/outbox-transition-head.test.ts",
                    "AU T6-W15 missing transition-head HOL scope",
                ),
                (
                    "deploy/cost-monitor/test/outbox-periodic-head.test.ts",
                    "AU T6-W15 missing periodic-head HOL scope",
                ),
                (
                    "deploy/cost-monitor/test/outbox-quarantine.test.ts",
                    "AU T6-W15 missing quarantine scope",
                ),
            ):
                missing_path = work / (Path(path).stem + "-missing.md")
                missing_path.write_text(
                    replace_once(
                        dag_text,
                        f"`{path}`; ",
                        "",
                        label,
                    ),
                    encoding="utf-8",
                )
                require(
                    label,
                    "au-check.py",
                    TRIAGE,
                    False,
                    dag=missing_path,
                )

            # Round 10 serializes durable PG re-arm before the provider/live
            # phase.  Keep the old edge as a separate mutation so a checker
            # that merely sees both vertices (or only checks reachability)
            # cannot accept the inverse ordering.
            inverse_t6w12_t1w6 = work / "inverse-t6-w12-t1-w6-order.md"
            inverse_t6w12_t1w6.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W12 |",
                    "T6-W15, ",
                    "T1-W6, T6-W15, ",
                    "inverse T6-W12 to T1-W6 ordering",
                ),
                encoding="utf-8",
            )
            require(
                "AU inverse/old T6-W12→T1-W6 ordering",
                "au-check.py",
                TRIAGE,
                False,
                dag=inverse_t6w12_t1w6,
            )

            missing_t6w12_from_t1w6 = work / "missing-t6-w12-from-t1-w6.md"
            missing_t6w12_from_t1w6.write_text(
                replace_in_row(
                    dag_text,
                    "| T1-W6 |",
                    "T6-W12, ",
                    "",
                    "T1-W6 T6-W12 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T1-W6 missing T6-W12 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_t6w12_from_t1w6,
            )

            missing_t1w6_from_t6w14 = work / "missing-t1-w6-from-t6-w14.md"
            missing_t1w6_from_t6w14.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W14 |",
                    "T1-W6, ",
                    "",
                    "T6-W14 T1-W6 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W14 missing T1-W6 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_t1w6_from_t6w14,
            )

            missing_t3w18_from_t1w5 = work / "missing-t3-w18-from-t1-w5.md"
            missing_t3w18_from_t1w5.write_text(
                replace_in_row(
                    dag_text,
                    "| T1-W5 |",
                    "T3-W18, ",
                    "",
                    "T1-W5 T3-W18 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T1-W5 missing T3-W18 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_t3w18_from_t1w5,
            )

            missing_cancellation_obstacle = work / "missing-o-cfcancel.md"
            missing_cancellation_obstacle.write_text(
                replace_once(
                    dag_text,
                    "| T3-W16 | W2 worker | T8-W2, T2-W2b, T6-W15, O-CFINVENTORY, O-CFCANCEL, T7-W4b |",
                    "| T3-W16 | W2 worker | T8-W2, T2-W2b, T6-W15, O-CFINVENTORY, T7-W4b |",
                    "T3-W16 O-CFCANCEL obstacle",
                ),
                encoding="utf-8",
            )
            require(
                "AU T3-W16 missing O-CFCANCEL obstacle",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_cancellation_obstacle,
            )

            missing_monitor_before_attempt = work / "missing-t6-w15-before-t3-w16.md"
            missing_monitor_before_attempt.write_text(
                replace_in_row(
                    dag_text,
                    "| T3-W16 |",
                    "T6-W15, ",
                    "",
                    "T3-W16 T6-W15 producer prerequisite",
                ),
                encoding="utf-8",
            )
            require(
                "AU T3-W16 missing T6-W15 producer prerequisite",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_monitor_before_attempt,
            )

            forbidden_canary_predecessor = work / "forbidden-t6-w6-before-t6-w13.md"
            forbidden_canary_predecessor.write_text(
                replace_once(
                    dag_text,
                    "| T6-W13 | W3 live proof | T6-W4, T6-W9, O-CANARY, T7-W4b |",
                    "| T6-W13 | W3 live proof | T6-W4, T6-W6, T6-W9, O-CANARY, T7-W4b |",
                    "forbidden T6-W6 to T6-W13 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU forbidden T6-W6 to T6-W13 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=forbidden_canary_predecessor,
            )

            missing_alerting_depth_predecessor = (
                work / "missing-t6-w14-before-t6-w10.md"
            )
            missing_alerting_depth_predecessor.write_text(
                replace_once(
                    dag_text,
                    "| T6-W10 | W3 live proof | T6-W6, T6-W9, T6-W12, T6-W14, T1-W6, O-CANARY-ACTIVATE, T7-W4b |",
                    "| T6-W10 | W3 live proof | T6-W6, T6-W9, T6-W12, T1-W6, O-CANARY-ACTIVATE, T7-W4b |",
                    "T6-W10 T6-W14 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W10 missing T6-W14 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_alerting_depth_predecessor,
            )

            missing_r2_interlock = work / "missing-r2-before-t4-w2.md"
            missing_r2_interlock.write_text(
                replace_once(
                    dag_text,
                    "| T4-W2 | W2 worker | T4-W1, R2 |",
                    "| T4-W2 | W2 worker | T4-W1 |",
                    "T4-W2 R2 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T4-W2 missing R2 interlock",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_r2_interlock,
            )

            missing_d7_interlock = work / "missing-d7-before-d3-consumer.md"
            missing_d7_interlock.write_text(
                replace_once(
                    dag_text,
                    "| T5-W4 | W3 live proof | D7, D3, D8, R3, T5-W1, T1-W6, T7-W4b |",
                    "| T5-W4 | W3 live proof | D3, D8, R3, T5-W1, T1-W6, T7-W4b |",
                    "D3 consumer D7 interlock",
                ),
                encoding="utf-8",
            )
            require(
                "AU D3 consumer missing D7 interlock",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_d7_interlock,
            )

            missing_stranger_predecessor = work / "missing-t5-w1-before-t5-w4.md"
            missing_stranger_predecessor.write_text(
                replace_once(
                    dag_text,
                    "| T5-W4 | W3 live proof | D7, D3, D8, R3, T5-W1, T1-W6, T7-W4b |",
                    "| T5-W4 | W3 live proof | D7, D3, D8, R3, T1-W6, T7-W4b |",
                    "T5-W4 T5-W1 predecessor",
                ),
                encoding="utf-8",
            )
            require(
                "AU T5-W4 missing T5-W1 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_stranger_predecessor,
            )

            drifted_monitor_scope = work / "drifted-t6-w12-provider-path.md"
            drifted_monitor_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/src/provider.ts`; ",
                    "`deploy/cost-monitor/src/provider-adapter.ts`; ",
                    "T6-W12 provider path scope",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W12 canonical provider path drift",
                "au-check.py",
                TRIAGE,
                False,
                dag=drifted_monitor_scope,
            )

            missing_monitor_tuple_interlock = (
                work / "missing-t1-w6-monitor-tuple-interlock.md"
            )
            missing_monitor_tuple_interlock.write_text(
                replace_once(
                    dag_text,
                    "`crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs`; ",
                    "",
                    "T1-W6 monitor tuple interlock scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T1-W6 missing monitor_tuple_interlock test",
                PLAN,
                False,
                work / "monitor-tuple-interlock-mirror",
                overrides={DAG: missing_monitor_tuple_interlock},
            )

            missing_sensitivity_receipt_isolation = (
                work / "missing-sensitivity-receipt-isolation.md"
            )
            missing_sensitivity_receipt_isolation.write_text(
                replace_once(
                    dag_text,
                    "external receipt verifier isolated from monitor application configuration",
                    "external receipt verifier",
                    "O-MONITORHOST receipt-verifier isolation",
                ),
                encoding="utf-8",
            )
            require(
                "AU missing O-MONITORHOST receipt-verifier isolation",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_sensitivity_receipt_isolation,
            )

            # The accepted registrations are stable before both suite runs.
            # A source-id substitution must still block; otherwise a producer
            # could mint a lane outside the sealed T6-W12 tuple.
            missing_t6w14_preregistration = work / "missing-t6-w14-preregistration.md"
            missing_t6w14_preregistration.write_text(
                replace_once(
                    dag_text,
                    "the exact future T6-W14 `canary-lifecycle` and\n"
                    "`canary-synthetic` source ids",
                    "future T6-W14 source ids",
                    "T6-W14 preregistered source ids",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W14 missing preregistered source ids",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_t6w14_preregistration,
            )

            # The consumer is also bind-only: removing that boundary must
            # block even when the stable accepted registrations remain present.
            missing_t6w14_bind_only = work / "missing-t6-w14-bind-only.md"
            missing_t6w14_bind_only.write_text(
                replace_once(
                    dag_text,
                    "T1-W6 and T6-W14 may\n"
                    "only bind their already-issued pairs; neither may mint, rotate, substitute, register or mutate\n"
                    "monitor acceptance or author an activation tuple.",
                    "T1-W6 may only bind its already-issued pair; T6-W14 may configure its lane.",
                    "T6-W14 bind-only boundary",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W14 missing preregistration/bind-only boundary",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_t6w14_bind_only,
            )

            # The ACK's ordered schema is a security boundary, not prose that
            # may be shortened while retaining a plausible signed response.
            signed_ack_schema = (
                "(ack_version,event_id,producer_seq,payload_digest,source,service,application,"
                "key_id,credential_epoch,monitor_rearm_tuple_digest,ingest_commit_id,"
                "committed_at,signer_key_id,signer_epoch,signature)"
            )
            ack_schema_drift = work / "ack-token-schema-drift.md"
            ack_schema_drift.write_text(
                replace_once(
                    dag_text,
                    signed_ack_schema,
                    signed_ack_schema.replace(",signer_epoch", ""),
                    "signed ACK canonical schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU signed ACK canonical schema drift",
                "au-check.py",
                TRIAGE,
                False,
                dag=ack_schema_drift,
            )

            # The canonical ACK implementation/test must remain in the
            # monitor's executable scope, rather than being implied by prose.
            missing_ack_scope = work / "missing-ack-token-scope.md"
            missing_ack_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/test/ack-token.test.ts`; ",
                    "",
                    "T6-W15 ACK-token test scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T6-W15 missing ACK-token scope",
                PLAN,
                False,
                work / "ack-token-scope-mirror",
                overrides={DAG: missing_ack_scope},
            )

            # A6.17 cannot be reconstructed from a summary after the
            # append-only journal implementation leaves the packet scope.
            missing_window_journal_scope = work / "missing-window-journal-scope.md"
            missing_window_journal_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/src/window_journal.ts`; ",
                    "",
                    "T6-W12 append-only journal scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T6-W12 missing append-only journal scope",
                PLAN,
                False,
                work / "window-journal-scope-mirror",
                overrides={DAG: missing_window_journal_scope},
            )

            missing_interlock_race_scope = work / "missing-interlock-race-scope.md"
            missing_interlock_race_scope.write_text(
                replace_once(
                    dag_text,
                    "`crates/corelink-fabric-server/tests/monitor_tuple_interlock_race.rs`; ",
                    "",
                    "T1-W6 interlock race scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T1-W6 missing interlock race scope",
                PLAN,
                False,
                work / "interlock-race-scope-mirror",
                overrides={DAG: missing_interlock_race_scope},
            )

            missing_signer_trust_field = work / "missing-signer-trust-tuple-field.md"
            missing_signer_trust_field.write_text(
                replace_once(
                    dag_text,
                    ",page_ack_signer_trust_revocation_digest,"
                    "ack_recovery_signer_trust_revocation_digest,",
                    ",ack_recovery_signer_trust_revocation_digest,",
                    "monitor-rearm signer-trust tuple field",
                ),
                encoding="utf-8",
            )
            require(
                "AU monitor tuple missing signer-trust field",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_signer_trust_field,
            )

            missing_canary_flag_scope = work / "missing-canary-flag-scope.md"
            missing_canary_flag_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cloudflare-canary/test/fabric-probe-flag-failclosed.test.ts`; ",
                    "",
                    "T6-W4 canary flag fail-closed scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP missing canary flag fail-closed scope",
                PLAN,
                False,
                work / "canary-flag-scope-mirror",
                overrides={DAG: missing_canary_flag_scope},
            )

            # T6-W1 owns only its three scripts. Workflow wiring belongs to
            # T2-W3, so reintroducing the former workflow atom is overlap.
            forbidden_t6w1_workflow_scope = work / "forbidden-t6-w1-workflow-scope.md"
            forbidden_t6w1_workflow_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W1 |",
                    "`scripts/pre-merge-gate-check.sh`",
                    "`scripts/pre-merge-gate-check.sh`; `.github/workflows/selftests.yml`",
                    "T6-W1 forbidden selftests workflow scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T6-W1 rejects workflow scope owned by T2-W3",
                PLAN,
                False,
                work / "forbidden-t6-w1-workflow-scope-mirror",
                overrides={DAG: forbidden_t6w1_workflow_scope},
            )

            # A listed atom is not effectively owned when the same row carves
            # it back out. Both DAG consumers must apply exclusions before
            # satisfying their required-path registries.
            excluded_required_scope = work / "excluded-required-scope.md"
            excluded_required_scope.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/src/window_journal.ts`; ",
                    "`deploy/cost-monitor/src/window_journal.ts` excluding "
                    "`deploy/cost-monitor/src/window_journal.ts`; ",
                    "T6-W12 excluded required scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP required atom excluded from effective scope",
                PLAN,
                False,
                work / "excluded-required-scope-mirror",
                overrides={DAG: excluded_required_scope},
            )
            require(
                "AU required atom excluded from effective scope",
                "au-check.py",
                TRIAGE,
                False,
                dag=excluded_required_scope,
            )

            excluded_required_scope_colon = work / "excluded-required-scope-colon.md"
            excluded_required_scope_colon.write_text(
                replace_once(
                    dag_text,
                    "`deploy/cost-monitor/src/window_journal.ts`; ",
                    "`deploy/cost-monitor/src/window_journal.ts`; excluding: "
                    "`deploy/cost-monitor/src/window_journal.ts`; ",
                    "T6-W12 colon-delimited excluded required scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP colon-delimited required atom exclusion",
                PLAN,
                False,
                work / "excluded-required-scope-colon-mirror",
                overrides={DAG: excluded_required_scope_colon},
            )
            require(
                "AU colon-delimited required atom exclusion",
                "au-check.py",
                TRIAGE,
                False,
                dag=excluded_required_scope_colon,
            )

            workflow_text = (REPO / ".github/workflows/selftests.yml").read_text(
                encoding="utf-8"
            )
            disabled_job_workflow = work / "disabled-job-selftests.yml"
            disabled_job_workflow.write_text(
                replace_once(
                    workflow_text,
                    "  selftests:\n",
                    "  selftests:\n    if: ${{ false }}\n",
                    "selftests job condition",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP selftests workflow rejects disabled job",
                PLAN,
                False,
                work / "disabled-job-selftests-mirror",
                workflow=disabled_job_workflow,
            )

            ignored_step_workflow = work / "ignored-step-selftests.yml"
            ignored_step_workflow.write_text(
                replace_once(
                    workflow_text,
                    "      - name: Discover and run every tracked selftest\n",
                    "      - name: Discover and run every tracked selftest\n"
                    "        continue-on-error: true\n",
                    "selftests step continue-on-error",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP selftests workflow rejects continue-on-error",
                PLAN,
                False,
                work / "ignored-step-selftests-mirror",
                workflow=ignored_step_workflow,
            )

            early_success_workflow = work / "early-success-selftests.yml"
            early_success_workflow.write_text(
                replace_once(
                    workflow_text,
                    "          set -euo pipefail\n",
                    "          set -euo pipefail\n          exit 0\n",
                    "selftests early successful exit",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP selftests workflow rejects early successful exit",
                PLAN,
                False,
                work / "early-success-selftests-mirror",
                workflow=early_success_workflow,
            )

            reset_array_workflow = work / "reset-array-selftests.yml"
            reset_array_workflow.write_text(
                replace_once(
                    workflow_text,
                    '          fi\n          for selftest in "${selftests[@]}"; do\n',
                    "          fi\n          selftests=()\n"
                    '          for selftest in "${selftests[@]}"; do\n',
                    "selftests post-discovery array reset",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP selftests workflow rejects post-discovery array reset",
                PLAN,
                False,
                work / "reset-array-selftests-mirror",
                workflow=reset_array_workflow,
            )

            missing_pg_fence_scope = work / "missing-pg-fence-scope.md"
            missing_pg_fence_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T1-W6 |",
                    "`crates/corelink-fabric/src/pg_monitor_fence.rs`; ",
                    "",
                    "T1-W6 PG transaction-fence scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T1-W6 missing PG transaction-fence scope",
                PLAN,
                False,
                work / "missing-pg-fence-scope-mirror",
                overrides={DAG: missing_pg_fence_scope},
            )

            wrong_page_ack_schema = work / "wrong-page-ack-schema.md"
            wrong_page_ack_schema.write_text(
                replace_once(
                    dag_text,
                    "page_ack_token=(page_ack_version,incident_id,",
                    "page_ack_token=(incident_id,",
                    "human page ACK exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU human page ACK schema drift",
                "au-check.py",
                TRIAGE,
                False,
                dag=wrong_page_ack_schema,
            )

            wrong_ack_recovery_schema = work / "wrong-ack-recovery-schema.md"
            wrong_ack_recovery_schema.write_text(
                replace_once(
                    dag_text,
                    "ACK_RECOVERY=(recovery_version,event_id,",
                    "ACK_RECOVERY=(event_id,",
                    "ACK_RECOVERY exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU ACK_RECOVERY schema drift",
                "au-check.py",
                TRIAGE,
                False,
                dag=wrong_ack_recovery_schema,
            )

            missing_page_ack_binding = work / "missing-page-ack-binding.md"
            missing_page_ack_binding.write_text(
                replace_once(
                    dag_text,
                    "on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,",
                    "on_call_schedule_digest,action,monitor_rearm_tuple_digest,",
                    "page ACK payload binding",
                ),
                encoding="utf-8",
            )
            require(
                "AU page ACK missing payload binding",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_page_ack_binding,
            )

            missing_signer_manifest_binding = (
                work / "missing-signer-manifest-binding.md"
            )
            missing_signer_manifest_binding.write_text(
                replace_once(
                    dag_text,
                    "overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,",
                    "overlap_expires_at,monitor_rearm_tuple_digest,",
                    "signer rotation recovery custody binding",
                ),
                encoding="utf-8",
            )
            require(
                "AU signer manifest missing recovery-custody binding",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_signer_manifest_binding,
            )

            missing_recovery_manifest_digest = (
                work / "missing-recovery-manifest-digest.md"
            )
            missing_recovery_manifest_digest.write_text(
                replace_once(
                    dag_text,
                    "revocation_record_digest,signer_rotation_manifest_digest,"
                    "signer_manifest_generation,signer_manifest_witness_root_digest,"
                    "current_monitor_rearm_tuple_digest,",
                    "revocation_record_digest,current_monitor_rearm_tuple_digest,",
                    "ACK_RECOVERY signer manifest digest",
                ),
                encoding="utf-8",
            )
            require(
                "AU ACK_RECOVERY missing signer-manifest digest",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_recovery_manifest_digest,
            )

            cross_document_schema_mutants = (
                (
                    "page ACK",
                    r"page_ack_token=\([^`\r\n]+\)",
                    "payload_digest",
                    "optional_payload_digest",
                ),
                (
                    "signer rotation manifest",
                    r"signer_rotation_manifest=\([^`\r\n]+\)",
                    "previous_manifest_digest",
                    "previous_manifest_hint",
                ),
                (
                    "ACK_RECOVERY",
                    r"ACK_RECOVERY=\([^`\r\n]+\)",
                    "signer_rotation_manifest_digest",
                    "signer_rotation_manifest_hint",
                ),
                (
                    "canary activation",
                    r"canary_activation_tuple=\([^`\r\n]+\)",
                    "producer_config_digest",
                    "producer_config_hint",
                ),
            )
            for family, pattern, old_field, new_field in cross_document_schema_mutants:
                schemas = re.findall(pattern, dag_text)
                if len(schemas) != 1:
                    raise AssertionError(
                        f"{family} cross-document fixture requires one DAG schema, "
                        f"got {len(schemas)}"
                    )
                schema = schemas[0]
                mutated_schema = schema.replace(old_field, new_field, 1)
                if mutated_schema == schema:
                    raise AssertionError(
                        f"{family} cross-document fixture did not mutate {old_field}"
                    )
                slug = family.lower().replace(" ", "-").replace("_", "-")

                bad_main_contract = work / f"bad-{slug}-schema-main.md"
                bad_main_contract.write_text(
                    replace_once(
                        plan,
                        schema,
                        mutated_schema,
                        f"main {family} exact schema",
                    ),
                    encoding="utf-8",
                )
                require(
                    f"WP {family} exact schema drift in main",
                    "wp-check.py",
                    bad_main_contract,
                    False,
                )
                require(
                    f"AU {family} exact schema drift in main",
                    "au-check.py",
                    TRIAGE,
                    False,
                    plan=bad_main_contract,
                )

                bad_delta_contract = work / f"bad-{slug}-schema-delta.md"
                bad_delta_contract.write_text(
                    replace_once(
                        staged,
                        schema,
                        mutated_schema,
                        f"delta {family} exact schema",
                    ),
                    encoding="utf-8",
                )
                require_mirrored_wp(
                    f"WP {family} exact schema drift in delta",
                    PLAN,
                    False,
                    work / f"bad-{slug}-delta-mirror",
                    overrides={STAGED: bad_delta_contract},
                )
                require(
                    f"AU {family} exact schema drift in delta",
                    "au-check.py",
                    TRIAGE,
                    False,
                    delta=bad_delta_contract,
                )

            t6w14_arming_delta = work / "t6w14-forbidden-arming-delta.md"
            t6w14_arming_delta.write_text(
                replace_once(
                    staged,
                    "T6-W14 remains bind-only/default-off and may neither seal "
                    "`canary_activation_tuple` nor change either flag. Only later "
                    "T6-W10 may",
                    "T6-W14 may set `FABRIC_PROBES_ENABLED=1` after its own proof. "
                    "Only later T6-W10 may",
                    "T6-W14 forbidden activation authority",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP delta rejects T6-W14 arming authority",
                PLAN,
                False,
                work / "t6w14-forbidden-arming-mirror",
                overrides={STAGED: t6w14_arming_delta},
            )
            require(
                "AU delta rejects T6-W14 arming authority",
                "au-check.py",
                TRIAGE,
                False,
                delta=t6w14_arming_delta,
            )

            # Cross-document canary identity schemas must retain two isolated
            # credential lanes; a singular/collapsed lane is not equivalent.
            activation_schema = re.findall(
                r"canary_activation_tuple=\([^`\r\n]+\)", dag_text
            )
            if len(activation_schema) != 1:
                raise AssertionError(
                    "dual-lane activation fixture requires one canonical DAG schema"
                )
            collapsed_activation = activation_schema[0].replace(
                "synthetic_source", "lifecycle_source", 1
            )
            for role, source_document in (
                ("main", plan),
                ("delta", staged),
                ("DAG", dag_text),
            ):
                bad_activation = work / f"collapsed-activation-lane-{role.lower()}.md"
                bad_activation.write_text(
                    replace_once(
                        source_document,
                        activation_schema[0],
                        collapsed_activation,
                        f"{role} collapsed activation lane",
                    ),
                    encoding="utf-8",
                )
                if role == "main":
                    require(
                        "WP main rejects collapsed activation lane",
                        "wp-check.py",
                        bad_activation,
                        False,
                    )
                    require(
                        "AU main rejects collapsed activation lane",
                        "au-check.py",
                        TRIAGE,
                        False,
                        plan=bad_activation,
                    )
                elif role == "delta":
                    require_mirrored_wp(
                        "WP delta rejects collapsed activation lane",
                        PLAN,
                        False,
                        work / "collapsed-activation-delta-mirror",
                        overrides={STAGED: bad_activation},
                    )
                    require(
                        "AU delta rejects collapsed activation lane",
                        "au-check.py",
                        TRIAGE,
                        False,
                        delta=bad_activation,
                    )
                else:
                    require_mirrored_wp(
                        "WP DAG rejects collapsed activation lane",
                        PLAN,
                        False,
                        work / "collapsed-activation-dag-mirror",
                        overrides={DAG: bad_activation},
                    )
                    require(
                        "AU DAG rejects collapsed activation lane",
                        "au-check.py",
                        TRIAGE,
                        False,
                        dag=bad_activation,
                    )

            # Each cardinality/order/ownership clause receives its own physical
            # corruption so a broad prose check cannot accidentally mask one.
            semantic_mutants = (
                (
                    "phase1-12-ticks",
                    dag_text,
                    "observes exactly 12\nconsecutive lifecycle ticks",
                    "observes exactly 11\nconsecutive lifecycle ticks",
                    "dag",
                ),
                (
                    "phase1-12-requests",
                    staged,
                    "exactly 12 passive outer-route\nrequests",
                    "exactly 11 passive outer-route\nrequests",
                    "delta",
                ),
                (
                    "phase1-12-acks",
                    plan,
                    "and runs exactly 12\nlifecycle ticks, 12 outer-route requests and 12 durably acknowledged lifecycle envelopes",
                    "and runs exactly 12\nlifecycle ticks, 12 outer-route requests and 11 durably acknowledged lifecycle envelopes",
                    "main",
                ),
                (
                    "phase2-20-transactions",
                    staged,
                    "then collects exactly 20 transactions",
                    "then collects exactly 19 transactions",
                    "delta",
                ),
                (
                    "phase1-before-phase2",
                    dag_text,
                    "Only after the Phase-1 artifact is sealed and immutable may\nPhase 2 begin and T6-W10",
                    "Before the Phase-1 artifact is sealed, Phase 2 may begin and T6-W10",
                    "dag",
                ),
                (
                    "phase2-no-contamination",
                    plan,
                    "those starts are excluded from and cannot amend or rerun the\nsealed A6.22 artifact",
                    "those starts may amend and rerun the\nsealed A6.22 artifact",
                    "main",
                ),
                (
                    "t6w14-flags-zero-zero",
                    plan,
                    "keeps both\n`FABRIC_PROBES_ENABLED` and `SYNTHETIC_SLOT_PROBES_ENABLED` exact `0`",
                    "keeps\n`FABRIC_PROBES_ENABLED` exact `1` and `SYNTHETIC_SLOT_PROBES_ENABLED` exact `0`",
                    "main",
                ),
                (
                    "t6w14-zero-action",
                    plan,
                    "proves zero outer-route\nrequests, lifecycle envelopes, container fetches, starts",
                    "permits one outer-route\nrequest, lifecycle envelope, container fetch and start",
                    "main",
                ),
                (
                    "t6w10-no-implementation",
                    staged,
                    "T6-W10 implements no driver, detector, credential or monitor\nroute",
                    "T6-W10 implements the driver, detector, credential and monitor\nroute",
                    "delta",
                ),
            )
            for slug, source_document, old, new, role in semantic_mutants:
                mutant = work / f"{slug}.md"
                mutated_document = replace_once(source_document, old, new, slug)
                if slug == "phase1-before-phase2":
                    mutated_document = replace_once(
                        mutated_document,
                        "Phase 2's may issue only after the immutable Phase-1\nroot.",
                        "Phase 2 may issue before any Phase-1 root.",
                        "Phase-2 owner-token ordering",
                    )
                mutant.write_text(
                    mutated_document,
                    encoding="utf-8",
                )
                if role == "main":
                    require(f"WP rejects {slug}", "wp-check.py", mutant, False)
                    require(
                        f"AU rejects {slug}", "au-check.py", TRIAGE, False, plan=mutant
                    )
                elif role == "delta":
                    require_mirrored_wp(
                        f"WP rejects {slug}",
                        PLAN,
                        False,
                        work / f"{slug}-mirror",
                        overrides={STAGED: mutant},
                    )
                    require(
                        f"AU rejects {slug}", "au-check.py", TRIAGE, False, delta=mutant
                    )
                else:
                    require_mirrored_wp(
                        f"WP rejects {slug}",
                        PLAN,
                        False,
                        work / f"{slug}-mirror",
                        overrides={DAG: mutant},
                    )
                    require(
                        f"AU rejects {slug}", "au-check.py", TRIAGE, False, dag=mutant
                    )

            # Round-13 security clauses get independent physical mutations.
            # Each fixture changes one authority, continuity, classification,
            # formula or isolation rule and must block without relying on a
            # companion schema corruption.
            dag_security_mutants = (
                (
                    "manifest-rollback-accepted",
                    "rollback, fork, equivocation, missing predecessor, reused generation,\n"
                    "witness-root discontinuity or issuer id/epoch regression is RED.",
                    "rollback is accepted; fork, equivocation, missing predecessor, reused generation,\n"
                    "witness-root discontinuity or issuer id/epoch regression is RED.",
                    False,
                ),
                (
                    "manifest-fork-accepted",
                    "rollback, fork, equivocation, missing predecessor, reused generation,\n"
                    "witness-root discontinuity or issuer id/epoch regression is RED.",
                    "rollback is RED; fork is accepted; equivocation, missing predecessor, reused generation,\n"
                    "witness-root discontinuity or issuer id/epoch regression is RED.",
                    False,
                ),
                (
                    "manifest-witness-discontinuity-accepted",
                    "rollback, fork, equivocation, missing predecessor, reused generation,\n"
                    "witness-root discontinuity or issuer id/epoch regression is RED.",
                    "rollback, fork, equivocation, missing predecessor and reused generation are RED;\n"
                    "witness-root discontinuity is accepted.",
                    False,
                ),
                (
                    "activation-replay-accepted",
                    "wrong signer role or replay is a verified violation and therefore `FAILED`; unavailable or\n"
                    "unverifiable activation evidence is `UNKNOWN`.",
                    "wrong signer role is `FAILED`; replay is accepted and unavailable activation evidence is `UNKNOWN`.",
                    True,
                ),
                (
                    "activation-expiry-accepted",
                    "expired tuple, revoked signer/authorization,",
                    "expired tuple is accepted; revoked signer/authorization,",
                    True,
                ),
                (
                    "activation-permitted-delta-weakened",
                    "`previous_activation_digest`, `synthetic_flag_value`, `activated_at`, `expires_at`,\n"
                    "`owner_authorization_digest` and `signature`; every other field, including both lane identities,",
                    "`previous_activation_digest`, `synthetic_flag_value`, `producer_image_digest`, `activated_at`,\n"
                    "`expires_at`, `owner_authorization_digest` and `signature`; selected other fields may drift,",
                    True,
                ),
                (
                    "activation-fixture-isolation-weakened",
                    "cryptographically separate from production lanes; fixtures cannot arm production timers, mutate\n"
                    "production signer/activation high-water or satisfy a production ACK.",
                    "shared with production lanes; fixtures may arm production timers and satisfy a production ACK.",
                    True,
                ),
                (
                    "activation-phase-credit-swapped",
                    "Phase 1 alone may credit A6.22 and Phase 2 alone may credit AU6.17.",
                    "Either phase may credit A6.22 or AU6.17.",
                    True,
                ),
                (
                    "activation-artifact-exception-weakened",
                    "implementation-artifact exception: it proves bind/default-off\n"
                    "behavior only, never activation, live A6.22/AU6.17 credit or mutation authority.",
                    "implementation artifact: it proves activation and live A6.22/AU6.17 credit.",
                    True,
                ),
                (
                    "activation-verified-negative-unknown",
                    "returns both flags to exact-`0`, emits fail-visible `FAILED`,",
                    "returns both flags to exact-`0`, emits fail-visible `UNKNOWN`,",
                    False,
                ),
                (
                    "owner-auth-replay-allowed",
                    "Ready-set membership, credentials, a green test or an expired/replayed token authorizes no\n"
                    "mutation.",
                    "Ready-set membership or an expired/replayed token authorizes the mutation.",
                    True,
                ),
                (
                    "ocfrate-owner-signature-weakened",
                    "`canonical_payload_digest` commits every preceding\n"
                    "field under the O-CFRATE domain tag, and the owner signature verifies those bytes under the\n"
                    "independently verified Billing-Administrator role/key/epoch.",
                    "`canonical_payload_digest` omits fields and an unverified named owner may sign it.",
                    True,
                ),
                (
                    "ocfrate-threshold-witness-weakened",
                    "Before observation the independent\n"
                    "witness appends it to the named log and signs the increasing sequence, previous/root digests and\n"
                    "witness time; local/mutable timestamps are invalid.",
                    "After observation the owner may write a mutable local threshold timestamp.",
                    True,
                ),
                (
                    "ocfrate-line-formula-weakened",
                    "the exact provider/account/plan/period/SKU/unit/currency/quantity/amount/rate bounds and proves the\n"
                    "line formula.",
                    "selected provider fields but does not prove the line formula.",
                    True,
                ),
                (
                    "ocfrate-observed-cost-formula-weakened",
                    "`observed_cost` is recomputed from the complete provider quantities, verified effective rate and\n"
                    "the declared credits/discounts/tax treatment and must remain at or below `cost_budget`.",
                    "`observed_cost` may be copied from a local estimate and may exceed `cost_budget`.",
                    True,
                ),
                (
                    "ocfrate-domain-weakened",
                    "All identifiers/digests/signatures/formulas/units\n"
                    "are nonempty; counts are non-negative integers; quantity/rate/threshold/money fields are finite\n"
                    "canonical non-negative decimals; the interval is nonempty; quantity, denominator and served count\n"
                    "are positive; and at least one billable quantity is positive.",
                    "Blank identifiers, negative counts, NaN money and zero quantity are accepted.",
                    True,
                ),
                (
                    "ocfrate-failure-formula-weakened",
                    "`failed_attempt_count + retry_count + idle_wakeup_count`; its denominator formula is\n"
                    "`attempt_count + retry_count + idle_wakeup_count`.",
                    "`failed_attempt_count`; its denominator formula is `attempt_count`.",
                    True,
                ),
                (
                    "r6-cross-tenant-read-allowed",
                    "unverified role or any allowed cross-tenant read leaves R6 unresolved and T5-W1 blocked.",
                    "unverified role or an allowed cross-tenant read resolves R6 and unblocks T5-W1.",
                    True,
                ),
            )
            for slug, old, new, check_wp in dag_security_mutants:
                mutant = work / f"{slug}.md"
                mutant.write_text(
                    replace_once(dag_text, old, new, slug), encoding="utf-8"
                )
                if check_wp:
                    require_mirrored_wp(
                        f"WP rejects {slug}",
                        PLAN,
                        False,
                        work / f"{slug}-mirror",
                        overrides={DAG: mutant},
                    )
                require(
                    f"AU rejects {slug}",
                    "au-check.py",
                    TRIAGE,
                    False,
                    dag=mutant,
                )

            owner_auth_schema = re.findall(
                r"OWNER_ACTION_AUTHORIZATION=\([^`\r\n]+\)", dag_text
            )
            if len(owner_auth_schema) != 1:
                raise AssertionError(
                    "owner authorization fixture requires one DAG schema"
                )
            owner_auth_weakened = work / "owner-auth-schema-weakened.md"
            owner_auth_weakened.write_text(
                replace_once(
                    dag_text,
                    owner_auth_schema[0],
                    owner_auth_schema[0].replace("nonce,", "reusable_hint,", 1),
                    "owner authorization nonce",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects weakened owner authorization schema",
                PLAN,
                False,
                work / "owner-auth-schema-weakened-mirror",
                overrides={DAG: owner_auth_weakened},
            )
            require(
                "AU rejects weakened owner authorization schema",
                "au-check.py",
                TRIAGE,
                False,
                dag=owner_auth_weakened,
            )

            r6_schema = re.findall(r"R6_RELAY=\([^`\r\n]+\)", dag_text)
            if len(r6_schema) != 1:
                raise AssertionError("R6 fixture requires one DAG schema")
            r6_schema_weakened = work / "r6-schema-weakened.md"
            r6_schema_weakened.write_text(
                replace_once(
                    dag_text,
                    r6_schema[0],
                    r6_schema[0].replace(
                        "role_authority_digest,", "self_asserted_role,", 1
                    ),
                    "R6 role authority",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects weakened R6 schema",
                PLAN,
                False,
                work / "r6-schema-weakened-mirror",
                overrides={DAG: r6_schema_weakened},
            )
            require(
                "AU rejects weakened R6 schema",
                "au-check.py",
                TRIAGE,
                False,
                dag=r6_schema_weakened,
            )

            predecessor_mutants = (
                ("missing-o-pg-rearm-predecessor", "| T1-W6 |", "O-PG-REARM, "),
                (
                    "missing-o-canary-activate-predecessor",
                    "| T6-W10 |",
                    "O-CANARY-ACTIVATE, ",
                ),
                ("missing-o-cfrate-predecessor", "| T7-W5 |", "O-CFRATE, "),
                ("missing-r6-before-t5-w1", "| T5-W1 |", "R6"),
            )
            for slug, row_marker, predecessor in predecessor_mutants:
                mutant = work / f"{slug}.md"
                mutant.write_text(
                    replace_in_row(
                        dag_text,
                        row_marker,
                        predecessor,
                        "—" if predecessor == "R6" else "",
                        slug,
                    ),
                    encoding="utf-8",
                )
                require_mirrored_wp(
                    f"WP rejects {slug}",
                    PLAN,
                    False,
                    work / f"{slug}-mirror",
                    overrides={DAG: mutant},
                )
                require(
                    f"AU rejects {slug}",
                    "au-check.py",
                    TRIAGE,
                    False,
                    dag=mutant,
                )

            # T3-W17 is the post-freeze containment implementation that
            # consumes the reconciled immutable ledger; its T0-W1 predecessor
            # cannot be dropped for a bare em-dash root.
            missing_t0w1_from_t3w17 = work / "missing-t0-w1-from-t3-w17.md"
            missing_t0w1_from_t3w17.write_text(
                replace_in_row(
                    dag_text,
                    "| T3-W17 |",
                    "T0-W1",
                    "—",
                    "T3-W17 T0-W1 predecessor",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 missing T0-W1 predecessor",
                PLAN,
                False,
                work / "missing-t0-w1-from-t3-w17-mirror",
                overrides={DAG: missing_t0w1_from_t3w17},
            )

            # T3-W17's staged containment proof is an exact DAG artifact; the
            # evidence scope cannot be dropped while the row still passes.
            missing_t3w17_evidence_scope = (
                work / "missing-t3-w17-containment-evidence-scope.md"
            )
            missing_t3w17_evidence_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T3-W17 |",
                    "; `docs/plan/evidence/T3-W17-containment-test.json`",
                    "",
                    "T3-W17 exact test evidence scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 missing exact test evidence scope",
                PLAN,
                False,
                work / "missing-t3-w17-containment-evidence-scope-mirror",
                overrides={DAG: missing_t3w17_evidence_scope},
            )

            # The individually frozen implementation contract must not be
            # editable without an explicit checker digest update and review.
            contract_path = PLAN_DIR / "contracts" / "T3-W17.md"
            contract_drift = work / "t3-w17-contract-drift.md"
            contract_drift.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "DRAIN_LEASE_TTL_MS = 120_000",
                    "DRAIN_LEASE_TTL_MS = 60_000",
                    "T3-W17 frozen contract lease drift",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 frozen contract digest drift",
                PLAN,
                False,
                work / "t3-w17-contract-drift-mirror",
                overrides={contract_path: contract_drift},
            )

            # The admission reservation closes a bidirectional TOCTOU: moving
            # its check after release is an unsafe ordering that must not be a
            # false PASS, even though the rest of the contract is unchanged.
            reservation_order_drift = work / "t3-w17-reservation-order-drift.md"
            reservation_order_drift.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "before listing-derived claim release",
                    "after listing-derived claim release",
                    "T3-W17 reservation-before-release ordering",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 reservation ordering drift",
                PLAN,
                False,
                work / "t3-w17-reservation-order-drift-mirror",
                overrides={contract_path: reservation_order_drift},
                contract_digest_bypass=True,
            )

            # Removing the DO-state reclaim fence would permit a time-based
            # second effect eligibility. Keep this mutation physically
            # distinct from the generic lease-drift fixture.
            reservation_reclaim_drift = work / "t3-w17-reservation-reclaim-drift.md"
            reservation_reclaim_drift.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "only because the deciding\n  DO transaction still observes that exact reservation state as `HELD`.",
                    "because a timer says the reservation is old.",
                    "T3-W17 reservation reclaim fence",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 reservation reclaim fence drift",
                PLAN,
                False,
                work / "t3-w17-reservation-reclaim-drift-mirror",
                overrides={contract_path: reservation_reclaim_drift},
                contract_digest_bypass=True,
            )

            reservation_identity_drift = work / "t3-w17-reservation-identity-drift.md"
            reservation_identity_drift.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "The redrive `effect_id` is exactly",
                    "The redrive `effect_id` is derived from",
                    "T3-W17 exact redrive identity",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 exact identity/effect-id drift",
                PLAN,
                False,
                work / "t3-w17-reservation-identity-drift-mirror",
                overrides={contract_path: reservation_identity_drift},
                contract_digest_bypass=True,
            )

            reservation_api_drift = work / "t3-w17-reservation-api-drift.md"
            reservation_api_drift.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "Reservation APIs\n  never inspect or mutate a backlog head",
                    "Reservation APIs\n  may inspect or mutate a backlog head",
                    "T3-W17 reservation API ownership",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 reservation API/head isolation drift",
                PLAN,
                False,
                work / "t3-w17-reservation-api-drift-mirror",
                overrides={contract_path: reservation_api_drift},
                contract_digest_bypass=True,
            )

            reservation_completion_reopen = work / "t3-w17-reservation-completion-reopen.md"
            reservation_completion_reopen.write_text(
                replace_once(
                    contract_path.read_text(encoding="utf-8"),
                    "It can never reopen or downgrade a completed reservation",
                    "It may reopen or downgrade a completed reservation",
                    "T3-W17 completion no-reopen fence",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 completion reopen drift",
                PLAN,
                False,
                work / "t3-w17-reservation-completion-reopen-mirror",
                overrides={contract_path: reservation_completion_reopen},
                contract_digest_bypass=True,
            )

            # Admission must distinguish an unexpired holder from an expired
            # holder, and the expired branch must append in the fencing tx.
            contract_text = contract_path.read_text(encoding="utf-8")
            admission_unexpired = work / "t3-w17-admit-unexpired-held-503.md"
            admission_unexpired.write_text(
                replace_once(
                    contract_text,
                    "An exact `HELD` reservation with\n  `expires_ms > now` returns typed `authority-busy` without writing an event,\n  and the route returns 503 so GitHub retries.",
                    "An exact `HELD` reservation with\n  `expires_ms > now` proceeds without writing an event,\n  and the route returns 202.",
                    "T3-W17 unexpired HELD 503 invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 unexpired HELD returns 503",
                PLAN,
                False,
                work / "t3-w17-admit-unexpired-held-503-mirror",
                overrides={contract_path: admission_unexpired},
                contract_digest_bypass=True,
            )

            admission_expired = work / "t3-w17-admit-expired-held-atomic.md"
            admission_expired.write_text(
                replace_once(
                    contract_text,
                    "An exact `HELD` reservation with\n  `expires_ms <= now` is atomically fenced and removed, and the contained event\n  is appended in that same transaction; this does not depend on a scheduled\n  tick.",
                    "An exact `HELD` reservation with\n  `expires_ms <= now` is removed, and the contained event\n  is appended by a later scheduled tick.",
                    "T3-W17 expired HELD atomic append invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 expired HELD append is atomic",
                PLAN,
                False,
                work / "t3-w17-admit-expired-held-atomic-mirror",
                overrides={contract_path: admission_expired},
                contract_digest_bypass=True,
            )

            stale_owner = work / "t3-w17-stale-owner-zero-effects.md"
            stale_owner.write_text(
                replace_once(
                    contract_text,
                    "Any stale owner tuple observed after that commit is a typed no-op and\n  touches zero KV or effect state.",
                    "Any stale owner tuple observed after that commit may continue and\n  touch effect state.",
                    "T3-W17 stale owner zero-effects invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 stale owner has zero effects",
                PLAN,
                False,
                work / "t3-w17-stale-owner-zero-effects-mirror",
                overrides={contract_path: stale_owner},
                contract_digest_bypass=True,
            )

            completion_latch = work / "t3-w17-completion-observed-latch.md"
            completion_latch.write_text(
                replace_once(
                    contract_text,
                    "With\n  `completion_observed = true`, `completeRedrive` transitions the eligible\n  record to terminal completion and removes it in that same transaction.",
                    "With\n  `completion_observed = true`, `completeRedrive` transitions the eligible\n  record to terminal completion but leaves the tombstone.",
                    "T3-W17 completion observed latch invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 completion latch removes atomically",
                PLAN,
                False,
                work / "t3-w17-completion-observed-latch-mirror",
                overrides={contract_path: completion_latch},
                contract_digest_bypass=True,
            )

            completion_tombstone = work / "t3-w17-completion-unobserved-tombstone.md"
            completion_tombstone.write_text(
                replace_once(
                    contract_text,
                    "With\n  `completion_observed = false`, it changes `EFFECT_ELIGIBLE -> COMPLETED` but\n  leaves the tombstone for verified completion cleanup.",
                    "With\n  `completion_observed = false`, it removes the reservation immediately.",
                    "T3-W17 unobserved completion tombstone invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 unobserved completion retains tombstone",
                PLAN,
                False,
                work / "t3-w17-completion-unobserved-tombstone-mirror",
                overrides={contract_path: completion_tombstone},
                contract_digest_bypass=True,
            )

            normalizer = work / "t3-w17-repo-job-normalizer.md"
            normalizer.write_text(
                replace_once(
                    contract_text,
                    "One canonical fail-closed `normalizeRepoJob(repo, job_id)` is the only\n  normalizer for reservation identity.",
                    "Multiple caller-provided normalizers may be used for reservation identity.",
                    "T3-W17 canonical repo/job normalizer invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 canonical repo/job normalizer",
                PLAN,
                False,
                work / "t3-w17-repo-job-normalizer-mirror",
                overrides={contract_path: normalizer},
                contract_digest_bypass=True,
            )

            normalizer_order = work / "t3-w17-normalizer-before-mutation.md"
            normalizer_order.write_text(
                replace_once(
                    contract_text,
                    "`admitQueued`,\n  `reserveRedriveCandidate`, `completeRedrive`, and\n  `clearCompletedRedrive` call this normalizer before any reservation lookup,\n  KV read/write, or mutation;",
                    "These APIs may perform a reservation lookup or mutation before calling\n  the normalizer;",
                    "T3-W17 normalizer-before-mutation invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 normalizer precedes reservation mutation",
                PLAN,
                False,
                work / "t3-w17-normalizer-before-mutation-mirror",
                overrides={contract_path: normalizer_order},
                contract_digest_bypass=True,
            )

            orphan_identity = work / "t3-w17-orphan-identity-fail-closed.md"
            orphan_identity.write_text(
                replace_once(
                    contract_text,
                    "before\n  `reserveRedriveCandidate` or any mutation, an orphan record's `repo`,\n  managed `labels`, and `installationId` are validated together",
                    "after\n  `reserveRedriveCandidate` or a mutation, an orphan record's `repo`,\n  managed `labels`, and `installationId` may be validated independently",
                    "T3-W17 orphan identity fail-closed invariant",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T3-W17 orphan identity validation is fail-closed",
                PLAN,
                False,
                work / "t3-w17-orphan-identity-fail-closed-mirror",
                overrides={contract_path: orphan_identity},
                contract_digest_bypass=True,
            )

            t6w1_missing_scope = work / "t6w1-missing-script-scope.md"
            t6w1_missing_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W1 |",
                    "`scripts/pre-merge-gate-check.selftest.sh`; ",
                    "",
                    "T6-W1 exact three-script scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects missing T6-W1 script scope",
                PLAN,
                False,
                work / "t6w1-missing-script-scope-mirror",
                overrides={DAG: t6w1_missing_scope},
            )

            phase1_unrelated_artifact = work / "phase1-unrelated-artifact-ordering.md"
            phase1_unrelated_artifact.write_text(
                replace_once(
                    plan,
                    "different authorization issued only after the immutable Phase-1\n"
                    "artifact exists.",
                    "different authorization issued after any unrelated\n"
                    "artifact exists.",
                    "explicit Phase-1 artifact ordering",
                ),
                encoding="utf-8",
            )
            require(
                "WP rejects unrelated artifact as Phase-2 predecessor",
                "wp-check.py",
                phase1_unrelated_artifact,
                False,
            )
            require(
                "AU rejects unrelated artifact as Phase-2 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                plan=phase1_unrelated_artifact,
            )

            phase2_probe_zero = work / "phase2-probe-zero.md"
            phase2_probe_zero.write_text(
                replace_once(
                    dag_text,
                    "Phase 2 begin and T6-W10 seal the next tuple with probe exact `1` "
                    "and synthetic exact `1`.",
                    "Phase 2 begin and T6-W10 seal the next tuple with probe exact `0` "
                    "and synthetic exact `1`.",
                    "Phase-2 exact 1/1 flag vector",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects Phase-2 probe=0 despite 20 transactions",
                PLAN,
                False,
                work / "phase2-probe-zero-mirror",
                overrides={DAG: phase2_probe_zero},
            )
            require(
                "AU rejects Phase-2 probe=0 despite 20 transactions",
                "au-check.py",
                TRIAGE,
                False,
                dag=phase2_probe_zero,
            )

            ocfrate_policy_id_only = work / "ocfrate-policy-id-only.md"
            ocfrate_policy_id_only.write_text(
                replace_once(
                    dag_text,
                    "`threshold_policy_digest` binds every threshold and formula.",
                    "`threshold_policy_digest` identifies a generic policy version.",
                    "O-CFRATE policy-to-threshold/formula binding",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects generic O-CFRATE policy id",
                PLAN,
                False,
                work / "ocfrate-policy-id-only-mirror",
                overrides={DAG: ocfrate_policy_id_only},
            )
            require(
                "AU rejects generic O-CFRATE policy id",
                "au-check.py",
                TRIAGE,
                False,
                dag=ocfrate_policy_id_only,
            )

            plan_owner_schemas = re.findall(
                r"OWNER_ACTION_AUTHORIZATION=\([^`\r\n]+\)", plan
            )
            if len(plan_owner_schemas) != 1:
                raise AssertionError(
                    "cross-document owner fixture requires one main-plan schema"
                )
            owner_cross_doc = work / "owner-auth-cross-doc-drift.md"
            owner_cross_doc.write_text(
                replace_once(
                    plan,
                    plan_owner_schemas[0],
                    plan_owner_schemas[0].replace("nonce,", "reusable_hint,", 1),
                    "main-plan owner authorization schema",
                ),
                encoding="utf-8",
            )
            require(
                "WP rejects cross-document owner authorization drift",
                "wp-check.py",
                owner_cross_doc,
                False,
            )
            require(
                "AU rejects cross-document owner authorization drift",
                "au-check.py",
                TRIAGE,
                False,
                plan=owner_cross_doc,
            )

            plan_r6_schemas = re.findall(r"R6_RELAY=\([^`\r\n]+\)", plan)
            if len(plan_r6_schemas) != 1:
                raise AssertionError(
                    "cross-document R6 fixture requires one main schema"
                )
            r6_cross_doc = work / "r6-cross-doc-drift.md"
            r6_cross_doc.write_text(
                replace_once(
                    plan,
                    plan_r6_schemas[0],
                    plan_r6_schemas[0].replace(
                        "owner_key_epoch,", "owner_claimed_epoch,", 1
                    ),
                    "main-plan R6 owner epoch",
                ),
                encoding="utf-8",
            )
            require(
                "WP rejects cross-document R6 drift",
                "wp-check.py",
                r6_cross_doc,
                False,
            )
            require(
                "AU rejects cross-document R6 drift",
                "au-check.py",
                TRIAGE,
                False,
                plan=r6_cross_doc,
            )

            no_t6w10_predecessor = work / "t1w4-missing-t6w10-predecessor.md"
            no_t6w10_predecessor.write_text(
                replace_in_row(
                    dag_text, "| T1-W4 |", ", T6-W10", "", "T1-W4 T6-W10 predecessor"
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects T1-W4 without T6-W10 predecessor",
                PLAN,
                False,
                work / "t1w4-predecessor-mirror",
                overrides={DAG: no_t6w10_predecessor},
            )
            require(
                "AU rejects T1-W4 without T6-W10 predecessor",
                "au-check.py",
                TRIAGE,
                False,
                dag=no_t6w10_predecessor,
            )

            same_batch = work / "t1w4-same-batch-as-t6w10.md"
            same_batch.write_text(
                replace_regex_once(
                    dag_text,
                    r"B(\d{2}): T6-W10\nB\d{2}: T1-W4",
                    r"B\1: T6-W10 T1-W4",
                    "T6-W10/T1-W4 batch serialization",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects same-batch T6-W10/T1-W4",
                PLAN,
                False,
                work / "same-batch-mirror",
                overrides={DAG: same_batch},
            )
            require(
                "AU rejects same-batch T6-W10/T1-W4",
                "au-check.py",
                TRIAGE,
                False,
                dag=same_batch,
            )

            wrong_artifact_owner = work / "no-wake-artifact-wrong-owner.md"
            wrong_artifact_text = replace_in_row(
                dag_text,
                "| T6-W10 |",
                "`docs/plan/evidence/T6-W14-canary-no-wake.json`; ",
                "",
                "remove no-wake artifact from T6-W10",
            )
            wrong_artifact_text = replace_in_row(
                wrong_artifact_text,
                "| T6-W14 |",
                " | Sol / live-risk |",
                "; `docs/plan/evidence/T6-W14-canary-no-wake.json` | Sol / live-risk |",
                "assign no-wake artifact to T6-W14",
            )
            wrong_artifact_owner.write_text(wrong_artifact_text, encoding="utf-8")
            require_mirrored_wp(
                "WP rejects T6-W14 artifact ownership",
                PLAN,
                False,
                work / "wrong-artifact-owner-mirror",
                overrides={DAG: wrong_artifact_owner},
            )
            require(
                "AU rejects T6-W14 artifact ownership",
                "au-check.py",
                TRIAGE,
                False,
                dag=wrong_artifact_owner,
            )

            handoff_page_schema = re.findall(r"page_ack_token=\([^`]+\)", handoff)
            handoff_recovery_schema = re.findall(r"ACK_RECOVERY=\([^`]+\)", handoff)
            for family, schemas, field in (
                ("page-ack", handoff_page_schema, "payload_digest,"),
                (
                    "ack-recovery",
                    handoff_recovery_schema,
                    "signer_rotation_manifest_digest,",
                ),
            ):
                if len(schemas) != 1:
                    raise AssertionError(
                        f"handoff {family} fixture requires one schema"
                    )
                bad_handoff = work / f"handoff-{family}-missing-binding.md"
                bad_handoff.write_text(
                    replace_once(
                        handoff,
                        schemas[0],
                        schemas[0].replace(field, "", 1),
                        f"handoff {family}",
                    ),
                    encoding="utf-8",
                )
                require_mirrored_wp(
                    f"WP rejects handoff {family} drift",
                    PLAN,
                    False,
                    work / f"handoff-{family}-mirror",
                    handoff=bad_handoff,
                )
                require(
                    f"AU rejects handoff {family} drift",
                    "au-check.py",
                    TRIAGE,
                    False,
                    handoff=bad_handoff,
                )

            relocated_schema = work / "relocated-page-ack-schema.md"
            page_schema = re.findall(r"page_ack_token=\([^`\r\n]+\)", plan)
            if len(page_schema) != 1:
                raise AssertionError(
                    "schema relocation fixture requires one main page ACK schema"
                )
            relocated_schema.write_text(
                plan.replace(f"`{page_schema[0]}`;", "", 1)
                + f"\n\n> Non-normative note: `{page_schema[0]}`\n",
                encoding="utf-8",
            )
            require(
                "WP rejects schema relocated outside canonical section",
                "wp-check.py",
                relocated_schema,
                False,
            )
            require(
                "AU rejects schema relocated outside canonical section",
                "au-check.py",
                TRIAGE,
                False,
                plan=relocated_schema,
            )

            for role, source_document in (
                ("main", plan),
                ("delta", staged),
                ("dag", dag_text),
                ("triage", triage),
            ):
                schemas = re.findall(
                    r"O_CFRATE_EVIDENCE=\([^`\r\n]+\)", source_document
                )
                if len(schemas) != 1:
                    raise AssertionError(
                        f"{role} threshold fixture requires one O-CFRATE schema"
                    )
                weakened = schemas[0].replace("threshold_policy_digest,", "", 1)
                mutant = work / f"ocfrate-missing-threshold-proof-{role}.md"
                mutant.write_text(
                    replace_once(
                        source_document, schemas[0], weakened, f"{role} threshold proof"
                    ),
                    encoding="utf-8",
                )
                if role == "main":
                    require(
                        "WP rejects main O-CFRATE threshold proof drift",
                        "wp-check.py",
                        mutant,
                        False,
                    )
                    require(
                        "AU rejects main O-CFRATE threshold proof drift",
                        "au-check.py",
                        TRIAGE,
                        False,
                        plan=mutant,
                    )
                elif role == "delta":
                    require_mirrored_wp(
                        "WP rejects delta O-CFRATE threshold proof drift",
                        PLAN,
                        False,
                        work / "ocfrate-threshold-delta-mirror",
                        overrides={STAGED: mutant},
                    )
                    require(
                        "AU rejects delta O-CFRATE threshold proof drift",
                        "au-check.py",
                        TRIAGE,
                        False,
                        delta=mutant,
                    )
                elif role == "dag":
                    require_mirrored_wp(
                        "WP rejects DAG O-CFRATE threshold proof drift",
                        PLAN,
                        False,
                        work / "ocfrate-threshold-dag-mirror",
                        overrides={DAG: mutant},
                    )
                    require(
                        "AU rejects DAG O-CFRATE threshold proof drift",
                        "au-check.py",
                        TRIAGE,
                        False,
                        dag=mutant,
                    )
                else:
                    require_mirrored_wp(
                        "WP rejects triage O-CFRATE threshold proof drift",
                        PLAN,
                        False,
                        work / "ocfrate-threshold-triage-mirror",
                        overrides={TRIAGE: mutant},
                    )
                    require(
                        "AU rejects triage O-CFRATE threshold proof drift",
                        "au-check.py",
                        mutant,
                        False,
                    )

            closed_interval = work / "ocfrate-closed-interval.md"
            closed_interval.write_text(
                replace_once(
                    triage,
                    "half-open interval",
                    "closed interval",
                    "O-CFRATE half-open interval",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects O-CFRATE closed interval",
                PLAN,
                False,
                work / "ocfrate-closed-mirror",
                overrides={TRIAGE: closed_interval},
            )
            require(
                "AU rejects O-CFRATE closed interval",
                "au-check.py",
                closed_interval,
                False,
            )

            non_provider_source = work / "ocfrate-non-provider-source.md"
            non_provider_source.write_text(
                replace_once(
                    staged,
                    "provider-issued invoice or\nusage export",
                    "locally transcribed dashboard summary",
                    "O-CFRATE provider-issued source",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects non-provider O-CFRATE source",
                PLAN,
                False,
                work / "ocfrate-source-mirror",
                overrides={STAGED: non_provider_source},
            )
            require(
                "AU rejects non-provider O-CFRATE source",
                "au-check.py",
                TRIAGE,
                False,
                delta=non_provider_source,
            )

            stale_handoff_sequence = work / "handoff-stale-canary-sequence.md"
            stale_handoff_sequence.write_text(
                replace_once(
                    handoff,
                    "`T6-W13 → T6-W14 → T6-W10`",
                    "`T6-W13 → T6-W14`",
                    "handoff canary evidence sequence",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP rejects stale handoff re-enable owner",
                PLAN,
                False,
                work / "handoff-sequence-mirror",
                handoff=stale_handoff_sequence,
            )
            require(
                "AU rejects stale handoff re-enable owner",
                "au-check.py",
                TRIAGE,
                False,
                handoff=stale_handoff_sequence,
            )

            missing_journal_reconciler_scope = (
                work / "missing-journal-reconciler-scope.md"
            )
            missing_journal_reconciler_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W12 |",
                    "`deploy/cost-monitor/src/journal_reconciler.ts`; ",
                    "",
                    "T6-W12 provider-reconciliation scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T6-W12 missing journal reconciler scope",
                PLAN,
                False,
                work / "missing-journal-reconciler-scope-mirror",
                overrides={DAG: missing_journal_reconciler_scope},
            )

            missing_clock_scope = work / "missing-trusted-clock-scope.md"
            missing_clock_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T6-W12 |",
                    "`deploy/cost-monitor/src/clock.ts`; ",
                    "",
                    "T6-W12 trusted-clock scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T6-W12 missing trusted-clock scope",
                PLAN,
                False,
                work / "missing-trusted-clock-scope-mirror",
                overrides={DAG: missing_clock_scope},
            )

            unstable_activation = work / "unstable-canary-activation.md"
            unstable_activation.write_text(
                replace_once(
                    dag_text,
                    "All four registry entries and credential authorizations are accepted and byte-stable",
                    "All four registry entries and credential authorizations remain inactive and mutable",
                    "stable canary registration",
                ),
                encoding="utf-8",
            )
            require(
                "AU unstable canary activation registry",
                "au-check.py",
                TRIAGE,
                False,
                dag=unstable_activation,
            )

            missing_activation_field = work / "missing-canary-activation-field.md"
            missing_activation_field.write_text(
                replace_once(
                    dag_text,
                    "producer_image_digest,producer_config_digest,probe_flag_name,",
                    "producer_image_digest,probe_flag_name,",
                    "canary activation producer-config binding",
                ),
                encoding="utf-8",
            )
            require(
                "AU canary activation tuple missing config digest",
                "au-check.py",
                TRIAGE,
                False,
                dag=missing_activation_field,
            )

            weakened_activation_drift = work / "weakened-canary-activation-drift.md"
            weakened_activation_drift.write_text(
                replace_once(
                    dag_text,
                    "verified field/digest drift,\nobserved post-check config/runtime/key/flag change,",
                    "selected field drift,",
                    "canary activation drift fail-closed matrix",
                ),
                encoding="utf-8",
            )
            require(
                "AU canary activation drift weakening",
                "au-check.py",
                TRIAGE,
                False,
                dag=weakened_activation_drift,
            )

            ambiguous_canary_status = work / "ambiguous-canary-status.md"
            ambiguous_canary_status.write_text(
                replace_once(
                    dag_text,
                    "`SKIPPED`, `FAILED`, `UNKNOWN` or `SERVED`",
                    "`SKIPPED`, `FAILED`, `OK` or `SERVED`",
                    "canary fail-visible status vocabulary",
                ),
                encoding="utf-8",
            )
            require(
                "AU canary ambiguous status vocabulary",
                "au-check.py",
                TRIAGE,
                False,
                dag=ambiguous_canary_status,
            )

            missing_idle_no_wake_scope = work / "missing-idle-no-wake-scope.md"
            missing_idle_no_wake_scope.write_text(
                replace_in_row(
                    dag_text,
                    "| T1-W6 |",
                    "`deploy/cloudflare-fabricd/test/idle-no-wake.test.ts`; ",
                    "",
                    "T1-W6 idle no-wake scope",
                ),
                encoding="utf-8",
            )
            require_mirrored_wp(
                "WP T1-W6 missing idle no-wake scope",
                PLAN,
                False,
                work / "missing-idle-no-wake-scope-mirror",
                overrides={DAG: missing_idle_no_wake_scope},
            )

            for path, label in (
                (
                    "deploy/cloudflare-canary/test/no-wake-target.test.ts",
                    "AU T6-W14 missing no-wake-target scope",
                ),
                (
                    "deploy/cloudflare-canary/src/rules.ts",
                    "AU T6-W14 missing rules scope",
                ),
                (
                    "deploy/cloudflare-canary/src/types.ts",
                    "AU T6-W14 missing types scope",
                ),
            ):
                missing_t6w14_scope = work / (Path(path).name + "-t6w14-missing.md")
                missing_t6w14_scope.write_text(
                    replace_in_row(
                        dag_text,
                        "| T6-W14 |",
                        f"`{path}`; ",
                        "",
                        label,
                    ),
                    encoding="utf-8",
                )
                require(
                    label,
                    "au-check.py",
                    TRIAGE,
                    False,
                    dag=missing_t6w14_scope,
                )

            pg_unset_enables = work / "pg-unset-enables.md"
            pg_unset_enables.write_text(
                replace_once(
                    dag_text,
                    "exact `FABRIC_PG_DISABLED=0` is the\nonly value",
                    "unset `FABRIC_PG_DISABLED` is the\nonly value",
                    "PG exact-zero enablement",
                ),
                encoding="utf-8",
            )
            require(
                "AU PG flag unset enablement",
                "au-check.py",
                TRIAGE,
                False,
                dag=pg_unset_enables,
            )

            wrong_cf_rate_artifact = work / "wrong-cf-rate-artifact.md"
            wrong_cf_rate_artifact.write_text(
                replace_once(
                    dag_text,
                    "docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json",
                    "docs/plan/evidence/O-CFRATE-estimate.json",
                    "O-CFRATE canonical artifact",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE artifact substitution",
                "au-check.py",
                TRIAGE,
                False,
                dag=wrong_cf_rate_artifact,
            )

            cf_rate_schemas = re.findall(r"O_CFRATE_EVIDENCE=\([^`\r\n]+\)", dag_text)
            if len(cf_rate_schemas) != 1:
                raise AssertionError(
                    "O-CFRATE fixture requires one canonical DAG schema, got "
                    f"{len(cf_rate_schemas)}"
                )
            cf_rate_schema = cf_rate_schemas[0]
            weakened_cf_rate_schema = cf_rate_schema.replace(
                ",cost_budget,", ",optional_cost_budget,", 1
            )
            if weakened_cf_rate_schema == cf_rate_schema:
                raise AssertionError(
                    "O-CFRATE schema fixture did not mutate cost_budget"
                )

            bad_cf_rate_triage = work / "bad-cf-rate-schema-triage.md"
            bad_cf_rate_triage.write_text(
                replace_once(
                    triage,
                    cf_rate_schema,
                    weakened_cf_rate_schema,
                    "triage O-CFRATE exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE exact schema drift in triage",
                "au-check.py",
                bad_cf_rate_triage,
                False,
            )

            bad_cf_rate_plan = work / "bad-cf-rate-schema-plan.md"
            bad_cf_rate_plan.write_text(
                replace_once(
                    plan,
                    cf_rate_schema,
                    weakened_cf_rate_schema,
                    "plan O-CFRATE exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE exact schema drift in plan",
                "au-check.py",
                TRIAGE,
                False,
                plan=bad_cf_rate_plan,
            )
            require(
                "WP O-CFRATE exact schema drift in plan",
                "wp-check.py",
                bad_cf_rate_plan,
                False,
            )

            bad_cf_rate_delta = work / "bad-cf-rate-schema-delta.md"
            bad_cf_rate_delta.write_text(
                replace_once(
                    staged,
                    cf_rate_schema,
                    weakened_cf_rate_schema,
                    "delta O-CFRATE exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE exact schema drift in delta",
                "au-check.py",
                TRIAGE,
                False,
                delta=bad_cf_rate_delta,
            )
            require_mirrored_wp(
                "WP O-CFRATE exact schema drift in delta",
                PLAN,
                False,
                work / "bad-cf-rate-delta-mirror",
                overrides={STAGED: bad_cf_rate_delta},
            )

            bad_cf_rate_dag = work / "bad-cf-rate-schema-dag.md"
            bad_cf_rate_dag.write_text(
                replace_once(
                    dag_text,
                    cf_rate_schema,
                    weakened_cf_rate_schema,
                    "DAG O-CFRATE exact schema",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE exact schema drift in DAG",
                "au-check.py",
                TRIAGE,
                False,
                dag=bad_cf_rate_dag,
            )
            require_mirrored_wp(
                "WP O-CFRATE exact schema drift in DAG",
                PLAN,
                False,
                work / "bad-cf-rate-dag-mirror",
                overrides={DAG: bad_cf_rate_dag},
            )

            weakened_cf_rate_budget = work / "weakened-cf-rate-budget.md"
            weakened_cf_rate_budget.write_text(
                replace_once(
                    dag_text,
                    "must remain at or below `cost_budget`",
                    "may exceed `cost_budget`",
                    "O-CFRATE total-cost budget",
                ),
                encoding="utf-8",
            )
            require(
                "AU O-CFRATE weakened cost budget",
                "au-check.py",
                TRIAGE,
                False,
                dag=weakened_cf_rate_budget,
            )

            producer_test_cycle = work / "producer-test-cycle.md"
            producer_test_cycle.write_text(
                replace_once(
                    dag_text,
                    "| T6-W12 | W1 pre-rearm live gate | T6-W15, T6-W9, T3-W16,",
                    "| T6-W12 | W1 pre-rearm live gate | T6-W15, T6-W9, T3-W16, T1-W6,",
                    "T6-W12 producer-test dependency cycle",
                ),
                encoding="utf-8",
            )
            require(
                "AU T6-W12 producer-test dependency cycle",
                "au-check.py",
                TRIAGE,
                False,
                dag=producer_test_cycle,
            )

            retrospective_journal = work / "retrospective-journal.md"
            retrospective_text = replace_once(
                dag_text,
                "The journal is write-ahead, not a retrospective audit.",
                "The journal may be reconstructed retrospectively.",
                "journal write-ahead boundary",
            )
            retrospective_text = replace_once(
                retrospective_text,
                "Before sending a page, applying a sensitivity\n"
                "control or returning a rearm attestation, T6-W12 durably appends the exact immutable intent",
                "After all external effects, T6-W12 may reconstruct an intent",
                "journal intent-before-effect boundary",
            )
            retrospective_journal.write_text(
                retrospective_text,
                encoding="utf-8",
            )
            require(
                "AU retrospective journal substitution",
                "au-check.py",
                TRIAGE,
                False,
                dag=retrospective_journal,
            )

        wrong_au_owner = work / "wrong-au-owner.md"
        union23 = next(
            line for line in triage.splitlines() if line.startswith("| union-23 |")
        )
        wrong_union23 = union23.replace("**T5-W1**", "**T7-W4**", 1)
        wrong_au_owner.write_text(
            triage.replace(union23, wrong_union23, 1), encoding="utf-8"
        )
        require(
            "AU7.10 outside canonical file owner", "au-check.py", wrong_au_owner, False
        )

    missing_mutations = EXPECTED_MUTATION_INVENTORY - blocked_mutation_inventory
    unexpected_mutations = blocked_mutation_inventory - EXPECTED_MUTATION_INVENTORY
    if missing_mutations or unexpected_mutations:
        raise AssertionError(
            "mutation inventory mismatch: "
            f"missing={sorted(missing_mutations)}, "
            f"unexpected={sorted(unexpected_mutations)}"
        )
    mutation_count = len(blocked_mutation_inventory)
    print(
        "\nplan gate self-test: PASS — baselines accepted and "
        f"{mutation_count} corruptions blocked"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
