# Autonomous sprint state — 2026-09-05

## Delivered and deployed

The preceding #538–#547 containment repair/hard-stop train merged to
`main@cda90940f74735c006693d9b9e3be85c37b26f1a`, tree-equal to its reviewed
and locally certified tip. Source census: **16/70 delivered, 54 remaining**;
source delivery is not operational/go-live acceptance.

Sprint 1 starts with #548 (`e89f9a7c`, re-drive pause), followed by #549's
version-bound evidence. Both remain open. Exact source e89f9a7c deployed at
19:12:20Z as Worker `95b39a6a-cf7e-4c5b-95fd-53dc290498f9`, 100% traffic.
The four bounded smoke probes passed. At 19:37:18Z, complete read-only provider
pagination found zero container instances in the exact runner, check-host and
fabricd applications, with no prior plaintext-var drift. Persistent DO records
remain intact; they are not running containers. See
[the evidence](../plan/evidence/T3-W18-containment-live.json).

A3.30/T3-W18 remains **RED**: authentic three-state 10/10, nonempty ordered
resume, measured side effects and unchanged-running-box evidence are not done.
No queued/in-progress runs existed in this repo during the access audit;
historical completed jobs are not pre-existing live redrive candidates.

## Active source preparation

These isolated branches earn no completion, deployment or merge credit yet:

- `prep/t8-auth-file`: Rust file-only auth, FIFO refusal and tests;
  `a08978b`, 12 focused tests plus doc-tests passed.
- `prep/t8-auth-boot`: check-host/DevEnv 0400 bridge, clean re-exec and lifecycle
  cleanup. Cold review is correcting marker injection and coder/root ownership;
  earlier tip `2ca5d08` must not be certified.
- `prep/t8-auth-worker`: fixed Worker-owned auth file path and provider ingress;
  `6c164b9`, 49 focused tests and typecheck passed. These three lanes jointly
  implement T8-W4b, not three completed WPs.
- `prep/t6-tick`: T6-W4 durable default-off tick/ACK producer. Earlier `da0f2a6`
  is rejected for signature/deadline/concurrency defects; corrections consume
  the orchestrator's explicit ordered-JSON/Ed25519 wire decisions. No shared
  producer HMAC may replace role-separated ACK trust.
- `prep/t6-alert-rules`: disjoint offline T6-W9 rule/notification work. No
  monitor binding, external page delivery or outage-proof credit.

## Execution rules

The owner authorized autonomous deployment and maximum safe parallelism,
Luna-first (Terra for bounded harder work). Offline preparation can proceed
while operational gates are pending; actual integration, deployment and
acceptance dependencies remain enforced. Do not resurrect quiet-review loops
as a global authoring freeze. Do not fabricate provider contracts/credentials,
commercial decisions, authentic webhook payloads or live evidence.

Four scheduling trains cover the 54 remaining vertices: 12/14/15/13. They are
stacked merge trains, not four independent branches. Full CI runs only on
final stacked-sprint tips, locally or on CoreLink; focused checks run during
authoring. Zero Actions jobs never substitute for CI. Preserve commits and
merge top-down with exact-head/base and final-tree verification. Every commit,
including orchestration merge commits, needs DCO.

The size rule is **600 LOC per code file**, ~400 for new files. Documents,
whole WPs, PRs and diffs are not capped. Keep old godfiles from material growth.
The primary checkout is dirty/conflicted and is not an integration workspace.

## Recovery and local operations

Production containment recovery is forward-only with ContainmentDO/v7 retained.
Both pause switches exact `"1"` are the emergency posture. A direct rollback
across the new DO migration is unsupported. Disarming intake permits automatic
drain on scheduled/fresh traffic; the drain-request flag is not an interlock.
Durable PG remains intentionally disabled; do not infer re-arm from these probes.

Operational snapshots and sanitized logs are under
`/private/tmp/corelink-deploy-ops.V1KGSC/`. The new containment-admin recovery
copy is outside git in the owner's `.config/corelink-runners` directory with
0700 directory/0600 file modes. Never print or commit credentials.

Only a verified 710MiB regenerable Cargo registry cache was removed; its source
checkout was preserved. Four orphan test processes from a completed Runners
review were stopped; other projects' builds were untouched. Disk later rose
above30GiB, not solely from our cleanup. Rust focused builds use one shared
`/private/tmp/corelink-t8-focused-target`, debug0/incremental0; no concurrent full
workspace matrices. Keep the 12GiB stop guard.

The continuously maintained local execution anchor is
`/Users/gustavoschneiter/.codex/plans/corelink-deepseek-wp-acceleration.md`.
