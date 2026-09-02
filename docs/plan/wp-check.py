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
        "only `scripts/orphan-box-check.selftest.sh`, `scripts/pre-merge-gate-check.selftest.sh` and `scripts/pre-merge-gate-check.sh`; it specifies exhaustive discovery but owns no T7-W4 selftest and no workflow file",
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
        ["A6.5", "A6.9"],
        ["INV-8"],
        "new `secret-scan.yml`, `corelink-stress.yml`, `deploy/cloudflare-canary/**` (not its README); default-off canary-tick producer plus durable capacity-1 ordered outbox/config/head-preserving credential migration and ≤60 s total enqueue→ACK/terminal crash/concurrency tests only, no A6.10 detector or live credit",
        1,
    ),
    "T6-W15": (
        ["A6.10"],
        ["INV-5"],
        "base-only `deploy/cost-monitor/` paths enumerated exactly in the canonical DAG; explicitly excludes `deploy/cost-monitor/README.md` and every T6-W12 provider/correlator/live-proof path. Its mandatory base suite includes `deploy/cost-monitor/test/outbox-transition-head.test.ts`, `deploy/cost-monitor/test/outbox-periodic-head.test.ts` and `deploy/cost-monitor/test/outbox-quarantine.test.ts`; while PG stays disabled T6-W12 must rerun the unchanged complete suite on its candidate before cutover and on the active final `monitor_rearm_tuple` after cutover",
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
    "A6.10": "test+probe",
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
HANDOFF_FILENAME = "2026-09-01-session-state-go-live-remediation.md"
DAG_SCHEMA_MARKER = (
    "**Date:** 2026-09-01 · **Schema:** `dispatch-dag/v1` · "
    "**Status: NOT FROZEN · NOT DISPATCHABLE · quiet count 0**"
)
DAG_TABLE_HEADING = "## Canonical node table"
DAG_BATCH_HEADING = "## Deterministic ready sets and proof"
DAG_EXPECTED_VERTEX_COUNT = 69
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
    "O-CFCANCEL",
    "O-CFRATE",
    "O-PG-REARM",
    "O-CANARY-ACTIVATE",
    "O-MONITORHOST",
}
DAG_PHASES = {
    "W0 unblock",
    "W0 containment (post-freeze)",
    "W1 parallel",
    "W1 serial",
    "W1 pre-rearm live gate",
    "W1 closer",
    "W2 worker",
    "W2 separate lane",
    "W3 live proof",
    "W4 post-decision",
    "W1 serial test+probe",
    "W2 worker test+probe",
}
MONITOR_REARM_TUPLE = "monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)"
O_CFRATE_EVIDENCE_SCHEMA = "O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)"
PAGE_ACK_SCHEMA = "page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)"
SIGNER_ROTATION_MANIFEST = "signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)"
ACK_RECOVERY_SCHEMA = "ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)"
CANARY_ACTIVATION_TUPLE = "canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)"
OWNER_ACTION_AUTHORIZATION = "OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)"
R6_RELAY_SCHEMA = "R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)"

# T6-W12 must execute T6-W15's complete base suite but may not acquire its test
# files. Freeze the version/cutover evidence contract in visible DAG prose so
# a dependency-only repair cannot silently restore the unsafe historical
# monitor version after PG is rearmed.
REQUIRED_DAG_SEMANTIC_CLAUSES = {
    "O-CFRATE exact evidence schema": O_CFRATE_EVIDENCE_SCHEMA,
    "T6-W12 final-version base regression": (
        "With `FABRIC_PG_DISABLED=1`, T6-W12 must first execute every unchanged "
        "T6-W15 test against its candidate image before cutover. A candidate PASS "
        "permits an atomic cutover; T6-W12 must then rerun the entire unchanged "
        "T6-W15 suite against the active final deployed tuple before PG rearm. The "
        "existing `docs/plan/evidence/T6-W12-independent-monitor.json` records both "
        "complete executions and their candidate/final image digest, config digest, "
        "credential epochs, expected-source registry digest and T6-W15 test-tree "
        "digest/results, plus the previous active version and atomic cutover/rollback "
        "outcome. A candidate failure forbids cutover; any post-cutover failure rolls "
        "back, keeps PG disabled and forbids T6-W12 completion. Only a post-cutover "
        "PASS against the active final tuple completes T6-W12. This is an execution "
        "obligation only: T6-W15 retains exclusive ownership of every base test file, "
        "and T6-W12 may not copy, weaken or rewrite the suite."
    ),
    "T6-W12 A6.17 sensitivity-control ownership": (
        "T6-W12 also owns the A6.17 sensitivity-window implementation. It runs "
        "from an O-MONITORHOST scheduler and credential distinct from the monitor "
        "application and validates delivery through an external receipt verifier "
        "isolated from monitor application configuration, so disabling or "
        "desensitizing the monitored detector/delivery path cannot green the "
        "seven-day window. T6-W10 owns only the evidence-only consumption of those "
        "seven-day sensitivity results."
    ),
    "T6-W12 future T1-W6 source registration": (
        "All four registry entries and credential authorizations are accepted and "
        "byte-stable before both complete T6-W15-suite executions and are inputs to "
        "the sealed tuple; T6-W14 never changes an accepted/active bit at bind time."
    ),
    "T3-W16 capacity-one external-ACK action gate": (
        "`T3-W16` directly waits for T6-W15 so its attempt/binding producer can use "
        "only the deployed external monitor. That lane has durable capacity one: it "
        "may hold at most one nonterminal head, and the total interval from durable "
        "enqueue to the external monitor's committed ACK or typed terminal is at most "
        "60 seconds. The exact head must be externally ACKed before container start "
        "or before any next immutable lifecycle/cost action. If the head is not "
        "terminal within 60 seconds, the packet is hard RED and the start/action "
        "fails closed."
    ),
    "other producer capacity and rotation gates": (
        "`T1-W6` applies the same already-required action gate to its two non-"
        "interchangeable write-only producer scopes (`fabric-server` and "
        "`fabricd-proxy`), each with its own durable capacity-one ordered outbox, key "
        "id and credential epoch; neither may reuse any read-only O-CFINVENTORY "
        "principal. T1-W6's provider inactivity proof therefore names O-CFINVENTORY "
        "directly and uses only the rearm-probe principal. T6-W14's periodic lifecycle "
        "and synthetic lanes are independently capacity one and cannot enqueue a "
        "successor or perform the successor's immutable action until the current head "
        "receives its external ACK or typed terminal. Credential rotation cannot reset "
        "the original enqueue clock or bypass a head: the producer must either drain "
        "that exact head under its original credential or perform a signed epoch "
        "migration that preserves its exact bytes, source/event/sequence identity, "
        "original timestamps and original deadline. Migration never restarts the "
        "60-second bound; exceeding it is hard RED and all dependent actions remain "
        "fail closed."
    ),
    "A6.17 canonical window tuple": (
        "The seven-day evidence is bound to exactly `A6.17_window_tuple=(monitor_"
        "rearm_tuple_digest,sensitivity_scheduler_deployed_runtime_digest,sensitivity_"
        "scheduler_config_digest,sensitivity_scheduler_key_id_credential_epoch_digest,"
        "receipt_verifier_deployed_runtime_digest,receipt_verifier_config_digest,on_"
        "call_escalation_schedule_digest)`. Any constituent "
        "drift during the window invalidates all elapsed time and restarts a full "
        "seven-day window. Drift of `monitor_rearm_tuple_digest` additionally invokes "
        "the broader PG-disable/reproof rule above; drift confined to the other six "
        "A6.17 fields invalidates only the A6.17 window and does not by itself "
        "invalidate the PG-rearm proof."
    ),
    "canonical monitor rearm tuple": (
        f"The final post-cutover PASS also seals the exact `{MONITOR_REARM_TUPLE}` "
        "and its digest."
    ),
    "nonce-bound effective-tuple health attestation": (
        "T6-W12's `deploy/cost-monitor/src/rearm_attestation.ts` derives the current "
        "effective `monitor_rearm_tuple` from the running deployment. For each fresh "
        "caller nonce it returns a signature over that nonce, the exact effective "
        "tuple digest and separately timestamped provider-poll and delivery-route "
        "health. The challenge response age is at most 10 seconds, and provider-poll "
        "and delivery health observations are at most 60 seconds old. The last five "
        "tuple fields separately commit the exact `(signer_key_id,signer_epoch)` registry, "
        "trust-anchor digests and revocation state for rearm, ingest-ACK, page-ACK, "
        "recovery and manifest-issuer roles. Cross-role trust is forbidden; a signature "
        "outside its exact role digest is invalid, and changing any role input is tuple "
        "drift. T1-W6's "
        "`crates/corelink-fabric-server/src/monitor_"
        "interlock.rs` obtains a new response before every readiness answer, PG-backed "
        "mutation, and PG/exporter socket/init/pool use, then verifies the bound tuple "
        "digest, nonce echo, signature and each applicable freshness rule. A cached "
        "success, nonce replay or earlier valid response is never reusable."
    ),
    "linearizable generation-fenced monitor interlock": (
        "Every readiness, mutation and socket/init path first obtains a generation-"
        "scoped coordinator permit. Every PG transaction additionally holds the same "
        "generation's shared transaction-scoped PostgreSQL advisory fence and "
        "validates the durable fence-row generation inside that transaction; an "
        "application-side check alone is never authority."
    ),
    "durable monitor tuple interlock and race tests": (
        "Any tuple mismatch, unavailable attestation, stale provider-poll or delivery "
        "health, bad signature or nonce failure first atomically arms a durable non-PG "
        "disable latch. Once latched, T1-W6 closes and discards every PG pool/socket, "
        "returns typed 503 for readiness and mutation, and permits zero further PG/"
        "exporter socket or mutation actions, including after restart. A planned `monitor_"
        "rearm_tuple` change must enter `CLOSING` and complete the permit/action/socket "
        "drain and pool discard before the change begins. `crates/corelink-fabric-"
        "server/tests/monitor_tuple_interlock."
        "rs` independently mutates all eleven tuple fields and injects unavailable, "
        "missing, stale, bad-signature, wrong-nonce, replayed and cached attestations; "
        "`crates/corelink-fabric-server/tests/monitor_tuple_interlock_race.rs` pauses "
        "each action immediately before and during durable commit, proves `CLOSING` "
        "admits no new generation, and proves every old permit/action/socket is "
        "cancelled, rolled back or closed before `LATCHED`, with zero post-latch side "
        "effects before and after restart. `deploy/cost-monitor/test/rearm-tuple-"
        "attestation.test.ts` "
        "proves the signed effective tuple, the two correctly bounded health classes "
        "and refusal of a wrong-but-valid signer, stale signer epoch and revoked "
        "signer under the seventh field. Every negative case is green only when the "
        "latch is durable, pools/"
        "sockets are gone, typed 503 is returned and zero action occurs. The latch may "
        "clear only after complete T6-W12 candidate/cutover/active-final reproof, exact "
        "T1-W6 rebinding to the new tuple and T1-W6's atomic consumption of a fresh, "
        "unexpired, one-shot `O-PG-REARM` authorization bound to the final tuple/scans/"
        "poll and exact flag transition; none alone restores PG."
    ),
    "sensitivity receipt excluded from PG interlock": (
        "Sensitivity receipt/health is excluded from the rearm attestation and PG "
        "latch: a missing or overdue sensitivity receipt alerts and restarts only the "
        "A6.17 window unless an independent tuple, provider-poll or core delivery "
        "failure separately triggers the interlock. The sensitivity receipt is overdue "
        "only relative to its configured cadence of at most six hours, never the rearm "
        "attestation's 60-second observation bound."
    ),
    "O-MONITORHOST independent sensitivity capabilities": (
        "It does the same for a sensitivity scheduler and an external receipt verifier "
        "that are distinct from the monitor application and from each other: the "
        "artifact names their separate accounts, credential ids, version/config "
        "digests, durable state, delivery-read permissions, control cadence of at most "
        "six hours and independent failure/configuration/control domains. It also "
        "names an append-only/WORM journal capability outside Cloudflare with atomic "
        "append, read-after-write verification, immutable record ids, at least eight "
        "days of retention and export/read permissions for the external receipt "
        "verifier; a mutable monitor database row or object overwrite is not that "
        "capability. These are capability-only properties; O-MONITORHOST does not "
        "claim application behavior, a deployed control, a receipt or a delivery or "
        "journal result."
    ),
    "authenticated exact durable ACK": (
        "Every producer consumes the same signed ACK token emitted by T6-W15 only "
        "after the matching ingest CAS commits. The token contains exactly these "
        "ordered fields and no implicit substitutes: `(ack_version,event_id,producer_"
        "seq,payload_digest,source,service,application,key_id,credential_epoch,monitor_"
        "rearm_tuple_digest,ingest_commit_id,committed_at,signer_key_id,signer_epoch,"
        "signature)`. `signature` authenticates the preceding fourteen fields in that "
        "order. A byte-identical duplicate returns the byte-identical stable token; an "
        "arbitrary HTTP 2xx, unsigned body or newly minted duplicate response is not an "
        "ACK. Before any gated next action, the producer verifies the signature and "
        "frozen fields against its durable head and rejects a wrong ACK version, old "
        "or wrong event, payload digest, sequence, source, service/application, key id "
        "or credential epoch, monitor-tuple digest, ingest commit or commit time. It "
        "also rejects a stale, revoked or wrong-but-currently-valid signer under "
        "`ingest_ack_signer_trust_revocation_digest`. Every rejection preserves the "
        "head and original 60-second deadline, "
        "performs zero gated action and fails closed. In both candidate and active-"
        "final passes, T6-W12's monitor-side fixtures submit isolated exact "
        "authenticated envelopes under every accepted lane and prove only ingest/CAS/"
        "stable-token behavior; they never execute, activate or claim a producer "
        "fixture. T6-W4, T3-W16, T1-W6 and T6-W14 each own their producer-side refusal/"
        "recovery suite, and T1-W6/T6-W14 must pass it after bind but before their "
        "first gated socket, fetch, start, acquire, spawn, release or successor "
        "emission. This split removes any producer-test dependency from T6-W12 back to "
        "consumers that follow it."
    ),
    "append-only exhaustive A6.17 journal": (
        "T6-W12 owns and deploys an append-only/WORM journal retained for at least "
        "eight days. It records every page, page acknowledgement, sensitivity control, "
        "rearm attestation and ingest ACK without sampling or mutable replacement. Each sealed "
        "window manifest binds the exact `A6.17_window_tuple` and records the inclusive "
        "start, exclusive end, exhaustive ordered record ids, record count, initial "
        "and terminal hash-chain roots and the storage-provider retention/immutability "
        "receipts. Rewrite, omitted first/ middle/last record, sequence/time gap and "
        "mixed-window/mixed-tuple substitutions all invalidate the window and restart "
        "seven days at zero. T6-W12 runs the complete journal mutant matrix against "
        "both the candidate and active-final deployments. T6-W10 consumes only the "
        "sealed manifest root and its retention/immutability receipts; no copied "
        "journal, summary counter, selected receipt set or evidence-time reconstruction "
        "can substitute for that exhaustive root."
    ),
    "canary exact-one fail-closed flags": (
        "The canary performs its outer-route fetch only when `FABRIC_PROBES_ENABLED` "
        "is the exact string `1`; exact `0` is the valid contained state and performs "
        "zero fabric fetches while the independent tick lane still emits its scheduled "
        "tick and authenticated containment/config state."
    ),
    "PG fence exact state machine": (
        "Arming is exactly `OPEN -> FENCING -> CLOSING -> LATCHED`: the coordinator "
        "first blocks new permits and publishes `FENCING`, then obtains the exclusive "
        "transaction-scoped fence lock, waits for every earlier shared transaction to "
        "commit or roll back, atomically advances the durable PG generation/latch and "
        "commits, and only then publishes `CLOSING`, closes/discards pools and reaches "
        "`LATCHED`."
    ),
    "write-ahead provider reconciliation": (
        "The journal is write-ahead, not a retrospective audit. Before sending a page, "
        "applying a sensitivity control or returning a rearm attestation, T6-W12 "
        "durably appends the exact immutable intent with its deterministic operation id "
        "and previous hash root; after the external effect it appends the exact provider "
        "result/receipt."
    ),
    "trusted monotonic evidence time": (
        "All window, freshness, ACK and receipt times pass through `deploy/cost-monitor/"
        "src/clock.ts`, which persists a monotonic high-water value and verifies the "
        "monitor's durable ingest-commit checkpoint, provider-authenticated monotonic "
        "watermark/`as_of` and immutable delivery/control receipt against the named "
        "O-MONITORHOST trusted time/checkpoint capability."
    ),
    "human page ACK exact schema": (
        f"An on-call page is acknowledged only by the exact signed `{PAGE_ACK_SCHEMA}`; "
        "`signature` authenticates the preceding fifteen fields in that order."
    ),
    "signer rotation manifest exact schema": (
        f"Signer rotation is authorized only by the exact signed "
        f"`{SIGNER_ROTATION_MANIFEST}`; `signature` authenticates the preceding "
        "nineteen fields in that order under the role-exclusive manifest-issuer trust "
        "and revocation set committed by the monitor tuple."
    ),
    "signer rotation ACK recovery exact schema": (
        f"`{ACK_RECOVERY_SCHEMA}`; `signature` authenticates the preceding twenty fields "
        "in that order."
    ),
    "canary activation exact schema": (
        f"`{CANARY_ACTIVATION_TUPLE}`. `signature` authenticates the preceding "
        "twenty-seven fields in that order under the role-exclusive canary-activation "
        "signer trust/revocation set. Those fields and their canonical digest are the "
        "sole activation authority."
    ),
    "canary activation successor authority": (
        "Acceptance atomically persists `activation_generation` and tuple digest as a "
        "monotonic high-water; `previous_activation_digest` must equal the accepted "
        "predecessor, `activated_at` must be trusted and fresh, `expires_at` must be "
        "later and unexpired, and `revocation_state_digest` must prove the signer and "
        "authorization remain current."
    ),
    "canary activation FAILED versus UNKNOWN": (
        "Reused/regressed generation, missing/wrong predecessor, fork/equivocation, "
        "stale/future activation, expired tuple, revoked signer/authorization, wrong "
        "signer role or replay is a verified violation and therefore `FAILED`; "
        "unavailable or unverifiable activation evidence is `UNKNOWN`."
    ),
    "canary phase credit and fixture isolation": (
        "Phase 1 alone may credit A6.22 and Phase 2 alone may credit AU6.17. Fixture "
        "keys, identities, sequence/outbox namespaces, manifest/verifier stores and "
        "activation high-water stores are cryptographically separate from production "
        "lanes; fixtures cannot arm production timers, mutate production signer/"
        "activation high-water or satisfy a production ACK."
    ),
    "canary implementation artifact exception": (
        "T6-W14's named deterministic default-off artifact is the explicit "
        "implementation-artifact exception: it proves bind/default-off behavior only, "
        "never activation, live A6.22/AU6.17 credit or mutation authority."
    ),
    "canary fail-visible state machine": (
        "Every probe result records exactly one of `SKIPPED`, `FAILED`, `UNKNOWN` or "
        "`SERVED`, plus reason, deployed version, `monitor_rearm_tuple` digest and "
        "trusted `observed_at`: valid exact-`0` is `SKIPPED`, an authoritative exact-`1` "
        "response alone may be `SERVED`, an observed negative is `FAILED`, and missing, "
        "invalid or unverifiable evidence is `UNKNOWN`."
    ),
    "PG exact-zero and idle no-wake": (
        "The fabricd containment switch is independently fail-closed: exact `FABRIC_PG_"
        "DISABLED=0` is the only value that permits a PG/container path."
    ),
    "O-CFRATE read-only owner evidence": (
        "Resolving the token is read-only evidence and authorizes no provider mutation, "
        "Cloudflare re-enable, proof credit or dispatch."
    ),
    "one-shot owner mutation authority": (
        f"`{OWNER_ACTION_AUTHORIZATION}`. The signature covers the preceding fourteen "
        "fields; independent role/key verification, strict `not_before <= consumed_at "
        "< expires_at`, and atomic one-time append-only consumption precede the bound "
        "mutation."
    ),
    "one-shot owner replay refusal": (
        "Ready-set membership, credentials, a green test or an expired/replayed token "
        "authorizes no mutation."
    ),
    "R6 exact relay authority": (
        f"`{R6_RELAY_SCHEMA}`. The signature covers the preceding twenty fields; "
        "tenants differ, the key is identical and both directions are exactly 20/20 "
        "refusals."
    ),
    "R6 fail-closed provenance": (
        "Runners/plan self-attestation, mutable sibling evidence, unverified role or "
        "any allowed cross-tenant read leaves R6 unresolved and T5-W1 blocked."
    ),
    "O-CFRATE formula and owner binding": (
        "`invoice_line_payload_digest` binds the exact provider/account/plan/period/SKU/"
        "unit/currency/quantity/amount/rate bounds and proves the line formula. "
        "`cost_quantity_reconciliation_digest` binds all usage lines, billable "
        "quantities, observed cost, manifest and receipt/cursor roots. "
        "`canonical_payload_digest` commits every preceding field under the O-CFRATE "
        "domain tag, and the owner signature verifies those bytes under the "
        "independently verified Billing-Administrator role/key/epoch."
    ),
    "O-CFRATE observed cost recomputation": (
        "`observed_cost` is recomputed from the complete provider quantities, verified "
        "effective rate and the declared credits/discounts/tax treatment and must remain "
        "at or below `cost_budget`."
    ),
    "O-CFRATE finite positive domain": (
        "All identifiers/digests/signatures/formulas/units are nonempty; counts are non-"
        "negative integers; quantity/rate/threshold/money fields are finite canonical "
        "non-negative decimals; the interval is nonempty; quantity, denominator and "
        "served count are positive; and at least one billable quantity is positive."
    ),
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

# A3.30 is one test+probe item with two mandatory phase owners. Keep the
# routing explicit: learning the kind from the aggregate item would
# incorrectly classify both WPs as probe owners.
STAGED_SPLIT_ACCEPTANCE_OWNERS = {
    "A3.30": "test owner: new T3-W17; live-probe owner: new T3-W18 (1 item total)",
    "A6.20": "base owner: principal T6-W15; pre-rearm final-monitor/provider owner: new T6-W12 (1 item total)",
}
STAGED_SPLIT_PHASE_KINDS = {
    ("T3-W17", "A3.30"): ("test", "a3.30 repo/test half"),
    ("T3-W18", "A3.30"): ("probe", "a3.30 live-probe half"),
    ("T6-W15", "A6.20"): ("test", "a6.10 + a6.20 base half"),
    ("T6-W12", "A6.20"): (
        "probe",
        "a6.20 pre-rearm final-monitor/provider half",
    ),
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

# These are structural dispatch contracts, not claims that the corresponding
# implementation or production evidence is semantically sufficient.
REQUIRED_DAG_DIRECT_PREDECESSORS = {
    # A deploy cannot resume while the containment proof or the fleet-busy
    # credential/force-deploy owner action is still outstanding.
    "T2-W2b": {"T3-W18", "O-FLEETBUSY", "T8-W4"},
    # The JIT-config secret-surface repair must precede pinning, deployment,
    # and image shipment; a fan-in at the final probe cannot prove that the
    # deployed digest actually contains the repaired entrypoint.
    "T2-W2a": {"T8-W4"},
    "T2-W4": {"T8-W4"},
    # The independent implementation must exist before the Cloudflare canary
    # is credited with delivering synthesized conditions end to end.
    "T6-W6": {"T6-W9", "T6-W12"},
    # Live billing reconciliation proofs cannot begin in parallel with the
    # worker billing implementation whose behavior they are meant to prove.
    "T4-W7": {"T4-W2", "R2"},
    "T4-W8": {"T4-W2"},
    # The first Cloudflare live-risk branch must remain behind the completed
    # containment re-drive proof, not merely behind a prose convention.
    "T6-W4": {"T3-W18"},
    # The external detector/base is live before durable-PG recovery, and the
    # provider-backed live phase may run only after both foundations exist.
    "T6-W15": {"T6-W4", "O-MONITORHOST", "T7-W4b"},
    # T6-W9 freezes the rule semantics; T6-W12 then implements those rules in
    # the independent monitor and re-proves the complete base against the
    # final candidate before PG may re-arm. T6-W15 remains a transitive base
    # predecessor through T6-W12 rather than a substitute for that final seal.
    "T1-W6": {"T6-W12", "O-CFINVENTORY", "O-PG-REARM"},
    "T6-W12": {
        "T6-W15",
        "T6-W9",
        "T3-W16",
        "O-CFINVENTORY",
    },
    "T6-W13": {"T6-W4", "T6-W9", "O-CANARY", "T7-W4b"},
    "T6-W14": {"T1-W6", "T6-W12", "T6-W13"},
    "T6-W10": {
        "T6-W6",
        "T6-W9",
        "T6-W12",
        "T6-W14",
        "T1-W6",
        "O-CANARY-ACTIVATE",
        "T7-W4b",
    },
    # External owner relations are not vertices and therefore must remain
    # explicit on the exact packets that consume them.
    "T4-W2": {"R2"},
    "T5-W4": {"T5-W1"},
    "T5-W1": {"R6"},
    "T7-W5": {"O-CFRATE"},
    "T3-W16": {"T6-W15", "O-CFINVENTORY", "O-CFCANCEL"},
    "T1-W5": {"T3-W18"},
    "T8-W7": {"T8-W4", "T2-W2a", "T2-W2b", "T2-W4"},
}

FORBIDDEN_DAG_DIRECT_PREDECESSORS = {
    # W13 is the immediate key lane. Making it wait for W6 would create a
    # cycle now that the live W6 proof correctly waits for external T6-W12.
    "T6-W13": {"T6-W6"},
    # The provider/final-version phase must finish before T1-W6. Retaining the
    # old reverse edge would recreate the Round-9 unsafe-rearm cycle.
    "T6-W12": {"T1-W6"},
}

REQUIRED_DAG_SCOPE_ATOMS = {
    # An evidence-only T6-W9 row cannot reserve or dispatch rule/channel work.
    "T6-W9": {
        "deploy/cloudflare-canary/src/rules.ts",
        "deploy/cloudflare-canary/test/rules.test.ts",
    },
    # The metrics-key repair and later probe re-enable both change executable
    # canary configuration, so the shared atom must be explicit and serialized.
    "T6-W13": {"deploy/cloudflare-canary/wrangler.jsonc"},
    "T6-W14": {
        "deploy/cloudflare-canary/wrangler.jsonc",
        "deploy/cloudflare-canary/test/lifecycle-monitor-envelope.test.ts",
        "deploy/cloudflare-canary/test/lifecycle-sampler-outbox.test.ts",
        "deploy/cloudflare-canary/src/synthetic_slot.ts",
        "deploy/cloudflare-canary/src/synthetic_outbox.ts",
        "deploy/cloudflare-canary/test/synthetic-slot-lifecycle.test.ts",
        "deploy/cloudflare-canary/test/synthetic-slot-outbox.test.ts",
        "deploy/cloudflare-canary/test/synthetic-slot-default-off.test.ts",
        "deploy/cloudflare-canary/test/synthetic-slot-credential-isolation.test.ts",
        "deploy/cloudflare-canary/test/synthetic-slot-correlation.test.ts",
    },
    "T6-W4": {
        "deploy/cloudflare-canary/src/config.ts",
        "deploy/cloudflare-canary/src/tick_outbox.ts",
        "deploy/cloudflare-canary/wrangler.jsonc",
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
        "crates/corelink-fabric-server/src/monitor_transaction_fence.rs",
        "crates/corelink-fabric-server/tests/monitor_outbox.rs",
        "crates/corelink-fabric-server/tests/monitor_transaction_fence.rs",
        "crates/corelink-fabric-server/tests/monitor_ack_recovery.rs",
        "deploy/cloudflare-fabricd/src/monitor_outbox.ts",
        "deploy/cloudflare-fabricd/test/monitor-outbox.test.ts",
        "deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts",
        "deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts",
        "deploy/cloudflare-fabricd/test/idle-no-wake.test.ts",
        "deploy/cloudflare-fabricd/wrangler.jsonc",
    },
    "T3-W16": {
        "deploy/cloudflare/test/attempt-monitor-outbox.test.ts",
        "deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts",
    },
    "T6-W12": {
        "deploy/cost-monitor/src/index.ts",
        "deploy/cost-monitor/src/scheduler.ts",
        "deploy/cost-monitor/src/state.ts",
        "deploy/cost-monitor/src/incidents.ts",
        "deploy/cost-monitor/src/types.ts",
        "deploy/cost-monitor/src/capability_rules.ts",
        "deploy/cost-monitor/src/synthetic_ingest.ts",
        "deploy/cost-monitor/src/journal_reconciler.ts",
        "deploy/cost-monitor/src/clock.ts",
        "deploy/cost-monitor/config.schema.json",
        "deploy/cost-monitor/migrations/0002-provider-cursors.json",
        "deploy/cost-monitor/test/c1-c5-rules.test.ts",
        "deploy/cost-monitor/test/c1-c5-synthetic-ingest.test.ts",
        "deploy/cost-monitor/test/acked-incident-update.test.ts",
        "deploy/cost-monitor/test/provider-stale-frozen.test.ts",
        "deploy/cost-monitor/test/window-journal-writeahead.test.ts",
        "deploy/cost-monitor/test/window-journal-reconcile.test.ts",
        "deploy/cost-monitor/test/window-journal-fork.test.ts",
        "deploy/cost-monitor/test/clock-freshness.test.ts",
    },
}

# These safety-critical rows use exact path registries rather than only minimum
# subsets, preventing broad scopes or evidence-only substitutions.
# T6-W12 deliberately reopens only the named base seams after T6-W15, then adds
# provider, C1-C5 and synthetic-ingest integration without taking base delivery
# or credential-isolation ownership.
EXACT_DAG_SCOPE_ATOMS = {
    "T6-W1": {
        "scripts/orphan-box-check.selftest.sh",
        "scripts/pre-merge-gate-check.selftest.sh",
        "scripts/pre-merge-gate-check.sh",
    },
    "T6-W4": {
        ".github/workflows/secret-scan.yml",
        ".github/workflows/corelink-stress.yml",
        "deploy/cloudflare-canary/src/index.ts",
        "deploy/cloudflare-canary/src/config.ts",
        "deploy/cloudflare-canary/src/types.ts",
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
    },
    "T1-W6": {
        "crates/corelink-fabric/src/pg_ledger.rs",
        "crates/corelink-fabric/src/billing_sink.rs",
        "crates/corelink-fabric/src/pg_monitor_fence.rs",
        "crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs",
        "crates/corelink-fabric-server/src/main.rs",
        "crates/corelink-fabric-server/src/server.rs",
        "crates/corelink-fabric-server/src/billing_export.rs",
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
        "deploy/cloudflare-fabricd/src/index.ts",
        "deploy/cloudflare-fabricd/src/monitor_outbox.ts",
        "deploy/cloudflare-fabricd/test/resilience.test.ts",
        "deploy/cloudflare-fabricd/test/monitor-outbox.test.ts",
        "deploy/cloudflare-fabricd/test/monitor-ack.test.ts",
        "deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts",
        "deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts",
        "deploy/cloudflare-fabricd/test/idle-no-wake.test.ts",
        "deploy/cloudflare-fabricd/wrangler.jsonc",
    },
    "T3-W16": {
        "deploy/cloudflare/src/index.ts",
        "deploy/cloudflare/src/lib.ts",
        "deploy/cloudflare/test/attempt-handle-reconcile.test.ts",
        "deploy/cloudflare/test/attempt-monitor-outbox.test.ts",
        "deploy/cloudflare/test/attempt-monitor-ack.test.ts",
        "deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts",
        "deploy/cloudflare/test/inventory-crosscheck.test.ts",
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
        "deploy/cost-monitor/test/provider-unavailable.test.ts",
        "deploy/cost-monitor/test/cursor-crash.test.ts",
        "deploy/cost-monitor/test/incident-boundary.test.ts",
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
        "deploy/cost-monitor/test/acked-incident-update.test.ts",
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
}

EXACT_DAG_ARTIFACTS = {
    "T6-W4": {"docs/plan/evidence/T6-W4-stress-host.json"},
    "T6-W15": {"docs/plan/evidence/T6-W15-monitor-base.json"},
    "T1-W6": {"docs/plan/evidence/T1-W6-pg-durable-live.json"},
    "T6-W12": {"docs/plan/evidence/T6-W12-independent-monitor.json"},
    "T6-W10": {
        "docs/plan/evidence/T6-W10-alerting-depth.json",
        "docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json",
        "docs/plan/evidence/T6-W14-canary-no-wake.json",
    },
    "T6-W14": set(),
}

SELFTEST_WORKFLOW = ".github/workflows/selftests.yml"
SELFTEST_WORKFLOW_WIRING = {
    "pull-request trigger": ("pull_request:", 1),
    "push trigger": ("push:", 1),
    "scripts path filters": ("- 'scripts/**'", 2),
    "self-workflow path filters": ("- '.github/workflows/selftests.yml'", 2),
    "tracked selftest discovery": (
        "git ls-files -z -- ':(glob)scripts/**/*.selftest.sh'",
        1,
    ),
    "non-vacuous empty-suite refusal": ("if (( ${#selftests[@]} == 0 )); then", 1),
    "fail-fast shell mode": ("set -euo pipefail", 1),
    "per-file execution": ('bash "${selftest}"', 1),
}

SELFTEST_WORKFLOW_FALSE_PASS_PATTERNS = {
    "conditional job/step": re.compile(r"(?m)^\s+if\s*:"),
    "continue-on-error": re.compile(r"(?m)^\s+continue-on-error\s*:"),
    "successful shell short-circuit": re.compile(
        r"(?m)^\s*(?:exit|return)(?:\s+0)?\s*(?:#.*)?$"
    ),
    "shell error suppression": re.compile(
        r"(?m)(?:^\s*set\s+\+e(?:\s|$)|\|\|\s*(?:true|:)(?:\s|$))"
    ),
    "backgrounded selftest": re.compile(r'(?m)^\s*bash\s+"\$\{selftest\}"\s*&\s*$'),
}

SELFTEST_WORKFLOW_STEP_NAME = "      - name: Discover and run every tracked selftest"
SELFTEST_WORKFLOW_RUN_BODY = [
    "set -euo pipefail",
    "# The glob magic makes **/ include selftests directly below scripts/",
    "# as well as in nested directories.",
    "mapfile -d '' selftests < <(git ls-files -z -- ':(glob)scripts/**/*.selftest.sh')",
    "if (( ${#selftests[@]} == 0 )); then",
    "  echo 'No tracked scripts/**/*.selftest.sh files found.' >&2",
    "  exit 1",
    "fi",
    'for selftest in "${selftests[@]}"; do',
    '  echo "==> bash ${selftest}"',
    '  bash "${selftest}"',
    "done",
]


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


def rendered_markdown(text, label, *, mask_fences=True):
    """Mask non-rendered HTML comments and fenced code without moving offsets.

    Canonical headings and tables must be visible Markdown. Backtick and tilde
    fences (including longer fences and language/info strings) can contain
    examples, but those examples cannot satisfy or duplicate a registry. HTML
    comment markers inside a fence are code, and fence markers inside an HTML
    comment are comments, so the two states are scanned together.
    """

    def mask_span(buffer, start, stop):
        for index in range(start, stop):
            if buffer[index] not in "\r\n":
                buffer[index] = " "

    rendered = list(text)
    errors = []
    in_comment = False
    fence_character = None
    fence_length = 0
    offset = 0

    for line in text.splitlines(keepends=True):
        line_body = line.rstrip("\r\n")

        if fence_character is not None:
            closing = re.fullmatch(
                rf" {{0,3}}{re.escape(fence_character)}{{{fence_length},}}[ \t]*",
                line_body,
            )
            if mask_fences:
                mask_span(rendered, offset, offset + len(line))
            if closing:
                fence_character = None
                fence_length = 0
            offset += len(line)
            continue

        # CommonMark fenced blocks may be indented by at most three spaces.
        # A fence is not recognized while a multi-line HTML comment is open.
        if not in_comment:
            opening = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line_body)
            if opening and not (
                opening.group(1).startswith("`") and "`" in opening.group(2)
            ):
                fence_character = opening.group(1)[0]
                fence_length = len(opening.group(1))
                if mask_fences:
                    mask_span(rendered, offset, offset + len(line))
                offset += len(line)
                continue

        position = 0
        while position < len(line):
            if in_comment:
                end = line.find("-->", position)
                if end < 0:
                    mask_span(rendered, offset + position, offset + len(line))
                    position = len(line)
                else:
                    mask_span(rendered, offset + position, offset + end + 3)
                    in_comment = False
                    position = end + 3
            else:
                start = line.find("<!--", position)
                if start < 0:
                    break
                end = line.find("-->", start + 4)
                if end < 0:
                    mask_span(rendered, offset + start, offset + len(line))
                    in_comment = True
                    position = len(line)
                else:
                    mask_span(rendered, offset + start, offset + end + 3)
                    position = end + 3
        offset += len(line)

    if fence_character is not None:
        errors.append(
            f"{label} contains an unterminated Markdown {fence_character * fence_length} fence"
        )
    if in_comment:
        errors.append(f"{label} contains an unterminated Markdown HTML comment")
    return "".join(rendered), errors


def parse_dag_ready_sets(visible_text, fence_visible_text):
    """Return ready-set rows from the one canonical fenced text block.

    ``visible_text`` has fenced code masked, while ``fence_visible_text`` keeps
    fences and their contents visible but still masks HTML comments.  The two
    strings retain identical line boundaries, so the visible heading can
    safely delimit the section without accepting a heading forged in code.
    """
    errors = []
    visible_lines = visible_text.splitlines()
    fence_visible_lines = fence_visible_text.splitlines()
    heading_lines = [
        index for index, line in enumerate(visible_lines) if line == DAG_BATCH_HEADING
    ]
    if len(heading_lines) != 1:
        return [], [], errors

    section_start = heading_lines[0] + 1
    section_stop = next(
        (
            index
            for index in range(section_start, len(visible_lines))
            if re.fullmatch(r"## .+", visible_lines[index])
        ),
        len(visible_lines),
    )

    blocks = []
    fence_character = None
    fence_length = 0
    opening_line = None
    opening_text = None
    for line_number in range(section_start, section_stop):
        line = fence_visible_lines[line_number]
        if fence_character is not None:
            closing = re.fullmatch(
                rf" {{0,3}}{re.escape(fence_character)}{{{fence_length},}}[ \t]*",
                line,
            )
            if closing:
                blocks.append((opening_line, line_number, opening_text, line))
                fence_character = None
                fence_length = 0
                opening_line = None
                opening_text = None
            continue

        opening = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if opening and not (
            opening.group(1).startswith("`") and "`" in opening.group(2)
        ):
            fence_character = opening.group(1)[0]
            fence_length = len(opening.group(1))
            opening_line = line_number
            opening_text = line

    if fence_character is not None:
        errors.append("ready-set section has an unterminated fenced block")
    if len(blocks) != 1:
        errors.append(
            "ready-set section fenced-block count mismatch: "
            f"expected 1, got {len(blocks)}"
        )

    canonical_bounds = None
    if len(blocks) == 1:
        block_open, block_close, opening_text, closing_text = blocks[0]
        if opening_text != "```text":
            errors.append(
                "ready-set block must open with exact canonical fence '```text'"
            )
        if closing_text != "```":
            errors.append("ready-set block must close with exact canonical fence '```'")
        if opening_text == "```text" and closing_text == "```":
            canonical_bounds = (block_open, block_close)

    ready_row_re = re.compile(r"\s*B\d{2}:")
    outside_rows = []
    for line_number, line in enumerate(fence_visible_lines):
        if not ready_row_re.match(line):
            continue
        if canonical_bounds is None or not (
            canonical_bounds[0] < line_number < canonical_bounds[1]
        ):
            outside_rows.append(line_number + 1)
    if outside_rows:
        errors.append(
            "ready-set BNN rows exist outside the canonical fenced block "
            f"at lines {outside_rows}"
        )

    rendered_batches = []
    rendered_ordinals = []
    if canonical_bounds is None:
        return rendered_ordinals, rendered_batches, errors

    batch_row_re = re.compile(r"B(\d{2}): (T\d+-W\d+[a-z]?(?: T\d+-W\d+[a-z]?)*)")
    for line_number in range(canonical_bounds[0] + 1, canonical_bounds[1]):
        line = fence_visible_lines[line_number]
        match = batch_row_re.fullmatch(line)
        if not match:
            errors.append(
                "ready-set canonical block contains a malformed or ambiguous row "
                f"at line {line_number + 1}: {line!r}"
            )
            continue
        rendered_ordinals.append(match.group(1))
        rendered_batches.append(match.group(2).split())

    if not rendered_batches:
        errors.append("ready-set canonical fenced block contains no BNN rows")
    return rendered_ordinals, rendered_batches, errors


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
        carveout = re.search(
            r"(?:^|\s+)(?:excluding|explicitly excludes|minus)\s*:?\s+",
            segment,
            re.I,
        )
        if carveout and code_fragments:
            has_inline_base = bool(plain_markdown(segment[: carveout.start()]))
            if has_inline_base:
                base = normalize_path_atom(fragments[0])
                raw_exclusions = fragments[1:]
            elif atoms:
                base = atoms[-1]
                raw_exclusions = fragments
            else:
                base = ""
                raw_exclusions = fragments
            if path_like(base):
                if has_inline_base:
                    atoms.append(base)
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
                r"(?:^|\s+)(?:excluding|explicitly excludes|minus)\s*:?\s+",
                fragment,
                maxsplit=1,
                flags=re.I,
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
            item_id = ids[0]
            staged_item_kinds[item_id] = kind
            expected_owner = STAGED_SPLIT_ACCEPTANCE_OWNERS.get(item_id)
            if expected_owner is not None:
                owner_cell = plain_markdown(row[2])
                if owner_cell != expected_owner:
                    errors.append(
                        f"staged acceptance {item_id} split-owner routing mismatch: "
                        f"expected {expected_owner!r}, got {owner_cell!r}"
                    )

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
        seen_split_phases = set()
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
                split_key = (owner, item_id)
                if item_id in STAGED_SPLIT_ACCEPTANCE_OWNERS:
                    split_phase = STAGED_SPLIT_PHASE_KINDS.get(split_key)
                    if split_phase is None:
                        errors.append(
                            f"staged split item {item_id} has unexpected phase owner {owner}"
                        )
                        continue
                    kind, expected_owns = split_phase
                    if split_key in seen_split_phases:
                        errors.append(
                            f"staged split phase is physically repeated: {split_key}"
                        )
                    seen_split_phases.add(split_key)
                    if owns != expected_owns:
                        errors.append(
                            f"staged split phase {owner}/{item_id} routing mismatch: "
                            f"expected {expected_owns!r}, got {owns!r}"
                        )
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
        missing_split_phases = sorted(set(STAGED_SPLIT_PHASE_KINDS) - seen_split_phases)
        if missing_split_phases:
            errors.append(
                f"staged split phases missing from packet registry: {missing_split_phases}"
            )

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


def validate_selftest_workflow(repository_root):
    """Require T6-W1's non-vacuous script-selftest CI wiring to exist."""
    errors = []
    workflow_path = repository_root / SELFTEST_WORKFLOW
    try:
        workflow = workflow_path.read_text(encoding="utf-8")
    except OSError as exc:
        return [f"selftest workflow {SELFTEST_WORKFLOW!r} is unavailable: {exc}"]

    for label, (required_text, expected_count) in SELFTEST_WORKFLOW_WIRING.items():
        count = workflow.count(required_text)
        if count != expected_count:
            errors.append(
                f"selftest workflow wiring {label!r} count mismatch: "
                f"expected {expected_count}, got {count}"
            )
    for label, pattern in SELFTEST_WORKFLOW_FALSE_PASS_PATTERNS.items():
        if pattern.search(workflow):
            errors.append(
                f"selftest workflow contains forbidden false-pass control {label!r}"
            )

    lines = workflow.splitlines()
    step_positions = [
        index for index, line in enumerate(lines) if line == SELFTEST_WORKFLOW_STEP_NAME
    ]
    if len(step_positions) != 1:
        errors.append(
            "selftest workflow must contain exactly one canonical discovery/execution step"
        )
    else:
        step_index = step_positions[0]
        expected_prelude = ["        shell: bash", "        run: |"]
        actual_prelude = lines[step_index + 1 : step_index + 3]
        body: list[str] = []
        cursor = step_index + 3
        while cursor < len(lines):
            line = lines[cursor]
            if line and len(line) - len(line.lstrip(" ")) <= 8:
                break
            if line.startswith("          "):
                body.append(line[10:])
            else:
                body.append(line)
            cursor += 1
        if actual_prelude != expected_prelude or body != SELFTEST_WORKFLOW_RUN_BODY:
            errors.append(
                "selftest workflow discovery/execution body differs from the canonical "
                "fail-fast dataflow"
            )

    execution_lines = re.findall(r'(?m)^\s*bash\s+"\$\{selftest\}"\s*$', workflow)
    if len(execution_lines) != 1:
        errors.append(
            "selftest workflow must execute the tracked selftest on one exact "
            f"fail-fast line, got {len(execution_lines)}"
        )
    return errors


def normalized_contract_text(document):
    """Collapse presentation-only Markdown whitespace for semantic clauses."""

    return re.sub(r"\s+", " ", plain_markdown(document)).strip()


def contract_section_between(document, start, end):
    """Return one ordered contract section, or empty on missing/ambiguous markers."""

    if document.count(start) != 1 or document.count(end) != 1:
        return ""
    start_index = document.index(start)
    end_index = document.index(end, start_index + len(start))
    return document[start_index:end_index] if start_index < end_index else ""


def canonical_schema_section(document, label):
    """Return the normative section for a schema, never a trailing note/example."""

    if "# Go-Live Remediation Plan" in document:
        if label == "O-CFRATE evidence":
            return contract_section_between(
                document, "### Wave 4", "## 6. Owner arming"
            )
        if label == "R6 relay":
            return contract_section_between(
                document, "## 7. Cross-repo relays", "## 8. Gates and the done-gate"
            )
        return contract_section_between(
            document, "## 1. The live picture", "## 2. Scope"
        )
    if "# Round-3 remediation delta" in document:
        if label == "canary activation":
            return contract_section_between(document, "## 2. Decisions", "### 3.1")
        return contract_section_between(
            document, "## 2. Decisions", "## 3. Acceptance proposals"
        )
    if "# Reconciled dispatch DAG" in document:
        return contract_section_between(
            document, "## Registry contract", "## Canonical node table"
        )
    if "# Union catalog" in document and label in {
        "O-CFRATE evidence",
        "owner action authorization",
        "R6 relay",
    }:
        return document[: document.index("# Union catalog")]
    return ""


def validate_canary_split_contract(document, label):
    """Freeze the default-off implementation/evidence-only two-phase split."""

    canonical = canonical_schema_section(document, "canary activation")
    normalized = normalized_contract_text(canonical)
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
            r"(?:only after the phase[- ]?1 artifact is sealed and immutable may "
            r"phase[- ]?2|phase[- ]?2.{0,160}(?:only after|issued only after).{0,80}"
            r"(?:the )?immutable phase[- ]?1 (?:artifact|root))"
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
        "Phase 2 exact permitted delta preserves all other fields": (
            r"(?:phase[- ]?2.{0,120}(?:only permitted|only).{0,80}(?:field )?changes|"
            r"only phase[- ]?2 field changes|exact permitted phase[- ]?2 delta is)"
            r".{0,80}activation_phase.{0,80}"
            r"activation_generation.{0,80}previous_activation_digest.{0,80}"
            r"synthetic_flag_value.{0,80}activated_at.{0,80}expires_at.{0,80}"
            r"owner_authorization_digest.{0,80}signature.{0,180}"
            r"(?:every other field|every identity|all identity).{0,240}"
            r"(?:byte-identical|remains)"
        ),
        "Phase 2 requires probe exact 1 and synthetic exact 1": (
            r"(?:phase[- ]?2.{0,900}(?:probe exact 1.{0,100}synthetic exact 1|"
            r"successor.{0,160}synthetic.{0,80}(?:exact )?1.{0,900}"
            r"(?:only phase[- ]?2 field changes|exact permitted phase[- ]?2 delta).{0,700}"
            r"(?:probe flag value|probe-value).{0,160}(?:byte-identical|remains))|"
            r"only later T6-W10.{0,600}with probe exact 1.{0,120}synthetic exact 0"
            r".{0,700}only after.{0,300}T6-W10 seal/arm synthetic exact 1)"
        ),
    }
    return [
        f"{label} canary split omits {requirement}"
        for requirement, pattern in requirements.items()
        if re.search(pattern, normalized, re.IGNORECASE) is None
    ]


def validate_cf_rate_cross_document(document, label):
    """Require provider-issued, predeclared and half-open O-CFRATE evidence."""

    normalized = normalized_contract_text(document)
    prose = normalized.replace(plain_markdown(O_CFRATE_EVIDENCE_SCHEMA), "")
    requirements = {
        "threshold declaration precedes observation": (
            r"threshold_declared_at.{0,140}(?:<|before).{0,80}budget_interval_start"
        ),
        "threshold policy has an independent append-only witness": (
            r"threshold_policy_digest.{0,300}(?:before observation.{0,120})?"
            r"independent(?:ly)? witness(?:ed)?.{0,120}(?:append|named log).{0,180}"
            r"(?:sequence|previous/root digests).{0,180}"
            r"(?:signature|signs|witness(?:ed)?(?:_at| time| key))"
        ),
        "threshold policy digest binds thresholds and formulas to witness": (
            r"threshold_policy_digest.{0,80}binds.{0,120}"
            r"(?:threshold|all three|every).{0,120}formula.{0,360}witness"
        ),
        "provider-issued invoice/usage source": (
            r"provider-issued (?:invoice|usage export|invoice or usage export)"
        ),
        "half-open budget interval": (
            r"(?:half-open|inclusive-start/exclusive-end|"
            r"\[budget_interval_start,budget_interval_end\))"
        ),
        "failure numerator formula is recomputable": (
            r"(?:failure_rate_numerator_formula|failure_rate_numerator|failure-rate "
            r"numerator formula).{0,80}failed_attempt_count.{0,80}"
            r"retry_count.{0,80}idle_wakeup_count"
        ),
        "failure denominator formula is recomputable": (
            r"(?:failure_rate_denominator_formula|failure_rate_denominator|denominator "
            r"formula).{0,80}attempt_count.{0,80}"
            r"retry_count.{0,80}idle_wakeup_count"
        ),
        "invoice line digest binds its arithmetic": (
            r"invoice_line_payload_digest.{0,500}(?:line_amount.{0,80}quantity.{0,80}"
            r"effective_rate|line formula|effective-rate bounds)"
        ),
        "cost reconciliation digest binds observed cost": (
            r"cost_quantity_reconciliation_digest.{0,260}(?:usage|invoice) lines?.{0,180}"
            r"(?:observed_cost|observed cost)"
        ),
        "canonical payload has verified owner authority": (
            r"canonical_payload_digest.{0,220}(?:every preceding|complete).{0,160}"
            r"owner[_ ]signature.{0,220}(?:independently verified|role authority)"
        ),
        "numeric domain is finite and nonnegative": (
            r"(?:quantities|quantity/rate).{0,100}(?:finite canonical )?non-negative "
            r"decimals"
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


def validate_owner_and_r6_cross_document(document, label):
    """Freeze owner-mutation and tenant-isolation authority in every source."""

    normalized = normalized_contract_text(document)
    requirements = {
        "owner authorization signature coverage": (
            r"signature (?:authenticates|covers) the preceding fourteen "
            r"(?:ordered )?fields"
        ),
        "owner role and key verification": (
            r"(?:owner key/epoch and role must verify|role/key verifies independently|"
            r"role/key validation|independent role/key verification)"
        ),
        "owner authorization one-shot consumption": (
            r"(?:atomically consumed once into an append-only|atomic.{0,60}"
            r"append-only one-time consumption|atomic one-time append-only consumption)"
        ),
        "R6 role-exclusive owner": (
            r"corelink-server.{0,80}CAS tenant-isolation owner.{0,80}Security/Storage role"
        ),
        "R6 signature coverage": (
            r"signature (?:authenticates|covers) the preceding twenty fields"
        ),
        "R6 distinct-tenant bidirectional refusal proof": (
            r"(?:tenants (?:are )?distinct|distinct tenants|tenants differ).{0,100}"
            r"(?:memoize key.{0,40}(?:byte-)?identical|identical memoize key|"
            r"the key is identical).{0,140}"
            r"(?:20/20.{0,80}(?:both directions|each direction|in both directions)|"
            r"both.{0,80}20/20)"
        ),
    }
    return [
        f"{label} owner/R6 contract omits {requirement}"
        for requirement, pattern in requirements.items()
        if re.search(pattern, normalized, re.IGNORECASE) is None
    ]


def validate_cross_document_contracts(
    plan_path, delta_path, dag_path, triage_path, handoff_path
):
    """Require byte-exact safety schemas in every normative document that owns them."""

    errors = []
    contracts = {
        "O-CFRATE evidence": (
            O_CFRATE_EVIDENCE_SCHEMA,
            (plan_path, delta_path, dag_path, triage_path),
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
        "owner action authorization": (
            OWNER_ACTION_AUTHORIZATION,
            (plan_path, delta_path, dag_path, triage_path),
        ),
        "R6 relay": (
            R6_RELAY_SCHEMA,
            (plan_path, delta_path, dag_path, triage_path),
        ),
    }
    visible_documents = {}
    for document_path in {path for _, paths in contracts.values() for path in paths}:
        try:
            visible, visibility_errors = rendered_markdown(
                document_path.read_text(encoding="utf-8"), document_path.name
            )
        except OSError as exc:
            errors.append(f"contract document {document_path} is unavailable: {exc}")
            continue
        visible_documents[document_path] = visible
        errors.extend(
            f"contract document {document_path.name}: {error}"
            for error in visibility_errors
        )
    for label, (literal, document_paths) in contracts.items():
        for document_path in document_paths:
            visible = visible_documents.get(document_path, "")
            section_count = canonical_schema_section(visible, label).count(literal)
            total_count = visible.count(literal)
            if section_count != 1 or total_count != 1:
                errors.append(
                    f"{label} exact schema must occur once in its canonical section "
                    f"and once visibly in {document_path.name}, got "
                    f"section={section_count}, total={total_count}"
                )

    for role, document_path in (
        ("main plan", plan_path),
        ("round-3 delta", delta_path),
        ("canonical DAG", dag_path),
    ):
        if document_path in visible_documents:
            errors.extend(
                validate_canary_split_contract(visible_documents[document_path], role)
            )
    for document_path in (plan_path, delta_path, dag_path, triage_path):
        if document_path in visible_documents:
            errors.extend(
                validate_cf_rate_cross_document(
                    visible_documents[document_path], document_path.name
                )
            )
            errors.extend(
                validate_owner_and_r6_cross_document(
                    visible_documents[document_path], document_path.name
                )
            )

    try:
        handoff_visible, handoff_visibility_errors = rendered_markdown(
            handoff_path.read_text(encoding="utf-8"), handoff_path.name
        )
    except OSError as exc:
        errors.append(f"contract document {handoff_path} is unavailable: {exc}")
    else:
        errors.extend(
            f"contract document {handoff_path.name}: {error}"
            for error in handoff_visibility_errors
        )
        handoff_schema_section = contract_section_between(
            handoff_visible, "## Compaction checkpoint", "## 1. Production containment"
        )
        compact_handoff = re.sub(r"\s+", "", handoff_visible)
        compact_handoff_section = re.sub(r"\s+", "", handoff_schema_section)
        for label, literal in (
            ("page ACK", PAGE_ACK_SCHEMA),
            ("ACK_RECOVERY", ACK_RECOVERY_SCHEMA),
        ):
            compact_literal = re.sub(r"\s+", "", literal)
            section_count = compact_handoff_section.count(compact_literal)
            total_count = compact_handoff.count(compact_literal)
            if section_count != 1 or total_count != 1:
                errors.append(
                    f"{label} exact schema must occur once in the handoff checkpoint "
                    f"and once visibly, got section={section_count}, total={total_count}"
                )
        normalized_handoff = normalized_contract_text(handoff_visible)
        handoff_flow_section = contract_section_between(
            handoff_visible,
            "## 5. Provenance and future DAG",
            "## 6. Rules and operational traps",
        )
        exact_canary_chain = "`T6-W13 → T6-W14 → T6-W10`"
        if (
            handoff_flow_section.count(exact_canary_chain) != 1
            or handoff_visible.count(exact_canary_chain) != 1
        ):
            errors.append(
                "handoff must contain the exact T6-W13 -> T6-W14 -> T6-W10 "
                "canary chain once in its provenance section"
            )
        if re.search(
            r"later no-wake re-enable is.{0,40}T6-W13.{0,20}T6-W14",
            normalized_handoff,
            re.IGNORECASE,
        ):
            errors.append(
                "handoff falsely assigns no-wake re-enable to T6-W14 instead of "
                "T6-W10's evidence-only phase"
            )
        if (
            re.search(
                r"T6-W13.{0,100}T6-W14.{0,100}T6-W10",
                normalized_handoff,
                re.IGNORECASE,
            )
            is None
        ):
            errors.append(
                "handoff must preserve the T6-W13 -> T6-W14 -> T6-W10 canary chain"
            )

    try:
        delta_visible, _ = rendered_markdown(
            delta_path.read_text(encoding="utf-8"), delta_path.name
        )
    except OSError as exc:
        errors.append(f"cannot validate A6.22 activation authority: {exc}")
        return errors
    rows = [
        line for line in delta_visible.splitlines() if line.startswith("| **A6.22** |")
    ]
    if len(rows) != 1:
        errors.append(
            "round-3 delta must contain exactly one visible A6.22 acceptance row, "
            f"got {len(rows)}"
        )
        return errors
    row = plain_markdown(rows[0])
    required = {
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
    errors.extend(
        f"A6.22 activation authority omits required {label}"
        for label, pattern in required.items()
        if re.search(pattern, row, re.IGNORECASE) is None
    )
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


def validate_dispatch_dag(path):
    """Validate the optional schema-v1 DAG as executable registry evidence."""
    dag_fail = []
    raw_text = path.read_text(encoding="utf-8")
    text, visibility_errors = rendered_markdown(raw_text, path.name)
    # Ready sets are deliberately rendered inside a fenced text block. Keep
    # that block available to the dedicated BNN parser while still masking
    # HTML comments and tracking fence boundaries. Generic headings/tables use
    # ``text`` above and therefore cannot be learned from fenced examples.
    batch_text, _ = rendered_markdown(raw_text, path.name, mask_fences=False)
    dag_fail.extend(f"DAG {error}" for error in visibility_errors)
    staged_nodes, au_nodes, registry_errors = load_supplemental_registries(path.parent)
    dag_fail.extend(f"DAG {error}" for error in registry_errors)
    probe_nodes, probe_registry_errors = load_probe_node_registry(path.parent)
    dag_fail.extend(f"DAG {error}" for error in probe_registry_errors)
    expected_nodes = set(WP) | staged_nodes | au_nodes
    if len(expected_nodes) != DAG_EXPECTED_VERTEX_COUNT:
        dag_fail.append(
            "DAG frozen registry count mismatch: "
            f"expected {DAG_EXPECTED_VERTEX_COUNT}, got {len(expected_nodes)}"
        )
    dag_fail.extend(
        f"DAG {error}"
        for error in validate_selftest_workflow(path.parent.parent.parent)
    )

    if len(exact_line_positions(text, DAG_SCHEMA_MARKER)) != 1:
        dag_fail.append(f"DAG exact schema marker mismatch in {path.name}")
    for heading in (DAG_TABLE_HEADING, DAG_BATCH_HEADING):
        count = len(exact_line_positions(text, heading))
        if count != 1:
            dag_fail.append(
                f"DAG EXACT HEADING count for {heading!r}: expected 1, got {count}"
            )

    normalized_dag = plain_markdown(text)
    for label, clause in REQUIRED_DAG_SEMANTIC_CLAUSES.items():
        count = normalized_dag.count(plain_markdown(clause))
        if count != 1:
            dag_fail.append(
                f"DAG semantic clause {label!r} count mismatch: expected 1, got {count}"
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
        if phase == "W3 live proof" and not node_artifacts and node != "T6-W14":
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

    for node, required in REQUIRED_DAG_DIRECT_PREDECESSORS.items():
        if node not in nodes:
            continue
        missing = sorted(required - set(nodes[node]["predecessors"]))
        if missing:
            dag_fail.append(
                f"DAG {node} is missing required exact hard predecessors {missing}"
            )

    for node, forbidden in FORBIDDEN_DAG_DIRECT_PREDECESSORS.items():
        if node not in nodes:
            continue
        present = sorted(forbidden & set(nodes[node]["predecessors"]))
        if present:
            dag_fail.append(
                f"DAG {node} has forbidden exact hard predecessors {present}"
            )

    # D7 -> D3 is an owner-action relation outside the WP graph.  Requiring D7
    # on every physical D3 consumer prevents the scheduler from treating a
    # satisfied publication token as proof that the leaked key was rotated.
    for node, record in nodes.items():
        if "D3" in record["predecessors"] and "D7" not in record["predecessors"]:
            dag_fail.append(f"DAG D3 consumer {node} does not also name D7 directly")

    for node, record in nodes.items():
        if record["phase"] == "W3 live proof" and not transitively_precedes(
            "T3-W18", node
        ):
            dag_fail.append(
                f"DAG W3 live-proof node {node} does not transitively follow T3-W18"
            )

    for node, required in REQUIRED_DAG_SCOPE_ATOMS.items():
        if node not in nodes:
            continue
        missing = sorted(required - set(nodes[node]["scopes"]))
        if missing:
            dag_fail.append(f"DAG {node} is missing required path atoms {missing}")
        excluded = {
            atom: sorted(
                exclusion
                for exclusion in nodes[node]["exclusions"]
                if path_atom_covers(exclusion, atom)
            )
            for atom in sorted(required & set(nodes[node]["scopes"]))
        }
        excluded = {
            atom: carveouts for atom, carveouts in excluded.items() if carveouts
        }
        if excluded:
            dag_fail.append(
                f"DAG {node} excludes required path atoms from effective scope: "
                f"{excluded}"
            )

    for node, expected_scopes in EXACT_DAG_SCOPE_ATOMS.items():
        if node not in nodes:
            continue
        actual_scopes = set(nodes[node]["scopes"])
        if actual_scopes != expected_scopes:
            dag_fail.append(
                f"DAG {node} exact split-scope mismatch: "
                f"missing {sorted(expected_scopes - actual_scopes)}, "
                f"unexpected {sorted(actual_scopes - expected_scopes)}"
            )
        excluded = {
            atom: sorted(
                exclusion
                for exclusion in nodes[node]["exclusions"]
                if path_atom_covers(exclusion, atom)
            )
            for atom in sorted(expected_scopes & actual_scopes)
        }
        excluded = {
            atom: carveouts for atom, carveouts in excluded.items() if carveouts
        }
        if excluded:
            dag_fail.append(
                f"DAG {node} exact path atoms are absent from effective scope: "
                f"{excluded}"
            )

    for node, expected_artifacts in EXACT_DAG_ARTIFACTS.items():
        if node not in nodes:
            continue
        actual_artifacts = set(nodes[node]["artifacts"])
        if actual_artifacts != expected_artifacts:
            dag_fail.append(
                f"DAG {node} exact split-artifact mismatch: "
                f"expected {sorted(expected_artifacts)}, "
                f"got {sorted(actual_artifacts)}"
            )

    if "T1-W4" in nodes and "T6-W10" not in nodes["T1-W4"]["predecessors"]:
        dag_fail.append(
            "DAG T1-W4/A1.9 must declare T6-W10 as an exact hard predecessor"
        )

    if "T6-W4" in nodes:
        detector_scopes = sorted(
            scope
            for scope in nodes["T6-W4"]["scopes"]
            if path_atoms_overlap(scope, "deploy/cost-monitor/**")
        )
        if detector_scopes:
            dag_fail.append(
                "DAG T6-W4 producer-only boundary owns external-monitor scopes "
                f"{detector_scopes}"
            )

    # O-BILLING is intentionally armed only after T9-W1 removes/quarantines
    # the devenv poison-pill emitter. External owner actions are not graph
    # vertices, so require every DAG consumer of O-BILLING to follow T9-W1.
    for node, record in nodes.items():
        if "O-BILLING" in record["predecessors"] and not transitively_precedes(
            "T9-W1", node
        ):
            dag_fail.append(
                f"DAG O-BILLING consumer {node} does not transitively follow T9-W1"
            )

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
        # Phase labels are scheduling metadata, not evidence semantics. A
        # staged or principal probe owner can perform live work from W0/W1/W2,
        # so every actual probe phase (apart from the containment probe itself)
        # must inherit the completed containment deployment in the WP graph.
        # External obstacle tokens cannot substitute for this ancestry.
        if node != "T3-W18" and not transitively_precedes("T3-W18", node):
            dag_fail.append(
                f"DAG probe/test+probe node {node} does not transitively "
                "follow T3-W18 containment"
            )
        if node != "T7-W4b" and "T7-W4b" not in nodes[node]["predecessors"]:
            dag_fail.append(
                f"DAG probe/test+probe node {node} does not declare T7-W4b "
                "as an exact hard predecessor"
            )
        if not nodes[node]["artifacts"] and node != "T6-W14":
            dag_fail.append(
                f"DAG probe/test+probe node {node} has no evidence artifact filename"
            )
    unregistered_probe_nodes = sorted(probe_nodes - set(nodes))
    if unregistered_probe_nodes:
        dag_fail.append(
            f"DAG probe/test+probe registry nodes missing from graph: {unregistered_probe_nodes}"
        )

    rendered_ordinals, rendered_batches, ready_set_errors = parse_dag_ready_sets(
        text, batch_text
    )
    dag_fail.extend(f"DAG {error}" for error in ready_set_errors)

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
    if "T6-W10" in rendered_nodes and "T1-W4" in rendered_nodes:
        t6_batch = next(
            index for index, batch in enumerate(rendered_batches) if "T6-W10" in batch
        )
        t1_batch = next(
            index for index, batch in enumerate(rendered_batches) if "T1-W4" in batch
        )
        if t6_batch >= t1_batch:
            dag_fail.append(
                "DAG ready sets must serialize T6-W10 before T1-W4/A1.9 in a later batch"
            )

    return dag_fail


plan_path = Path(sys.argv[1])
raw_doc = plan_path.read_text(encoding="utf-8")
doc, visibility_errors = rendered_markdown(raw_doc, plan_path.name)
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
    if node in EXACT_DAG_SCOPE_ATOMS:
        atoms = tuple(sorted(EXACT_DAG_SCOPE_ATOMS[node]))
        exclusions = ()
    else:
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
delta_path = Path(__file__).with_name(STAGED_FILENAME)
triage_path = Path(__file__).with_name("union-triage-remaining.md")
handoff_path = Path(__file__).parent.parent / "handoff" / HANDOFF_FILENAME
fail.extend(
    validate_cross_document_contracts(
        plan_path, delta_path, dag_path, triage_path, handoff_path
    )
)
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
