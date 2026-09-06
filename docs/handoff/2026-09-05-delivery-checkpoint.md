# Delivery checkpoint — 2026-09-05

Continue in `/private/tmp/corelink-delivery-sprint1-20260905`, branch
`delivery/sprint1-20260905`. The original checkout has pre-existing conflicts;
do not reset, clean, or use it as the integration tree. Earlier session-state
and session-handoff files remain historical inputs.

The mechanical source of execution state is
[`delivery-ledger.json`](../plan/delivery-ledger.json). Run
`python3 scripts/dev/delivery-ledger.py --check --report` to recover counts,
owners, findings and next actions. The canonical DAG retains all 70 WPs and
the agreed four sprint scopes. Sprint 1 has 7/12 complete source implementations;
Sprint 2 has 2/14 (T3-W1 and T4-W2) approved prepared implementations.
No new sprint or WP is recorded as delivered. Historical source records remain
separate from independently verified sprint delivery.

Full CI and heavy suites run only when the complete sprint is composed. T6-W9's
C1–C5 matrix is integrated at `47ae1bc370c6a68f79aa126e203ecf661b59415a`,
with independent source approval and 52 focused canary tests. This freezes the
routing contract; it does not implement T6-W12 detectors or bind an external host.
T4-W2 source `ffc5dadff397c546817673d04ad62e8ab527deae` passed root review
and two cold recovery tests; it remains prepared behind canonical dependencies.

Root owns final review, integration and ledger. HuGR TechLead rolling dispatch
is in use with one Luna executor per WP and distinct read-only review when useful.
The executor lanes returned their candidates; inspect ledger and actual agent
status before resuming. T8-W1 `2053a7221e3a1fdabdbb3c53113d3a6741acfe0f`
now exercises 100 concurrent calls through production ContainmentDO and the
slot-acquisition caller: five permits, 95 refusals. A3.14/A3.15 remain partial.
T8-W5 `86e9c0865070174edf149e17dfe4d8984530ee7c` separates registered,
revoke_requested and revoked state. Root added production-authority tests in
`3ee3d74297327bd0efc0a8acd9f18faaf4c50c7c`; nine focused tests passed,
including live credential retry isolation, restart, stale KV/remint preservation
and legacy unknown refusal. F008 still prevents its whole-WP seal.
The earlier candidate that revoked healthy credentials was rejected and never
integrated or deployed.

T3-W3 `77cb30f6c5627c56d582d9d3c65f2d76295e9474` has seven cold focused
tests passing. Root verified the complete relay applies against a fresh archive
of actual server `c0a4466d930073d05ebedc3ac8b8d9cc8474b68f`, tree
`79b51bfd163fb41e31fb9ab52105110a06e6d4cf`. The real sibling remains unchanged.
T3-W2 `6496cb3578be0b5c127d248b5341aaa4146699f5` removes unsafe KV-only
stale-claim deletion and slot release. A3.12 authoritative recovery remains F007,
with no acceptance credit. F008 records suspension epoch, delayed-event and
legacy migration gaps; F006 records transactional credential identity/retry safety.

The Sprint 2 preparation tree is `/private/tmp/corelink-delivery-sprint2-20260905`.
Unsatisfied hard dependencies prevent entry into the active delivery stack and
delivery credit. T4-W1's attribution TTL is fixed and authoritative enumeration
exists, but D13 owner precedence is unratified. T3-W1's approved redaction source
is `a54470eabad870568ae8ab8fbeec83c0f45099ba`; its Rust tests await the sprint gate.
No new CI, deployment, live provider action or sibling mutation was performed.

Current implementation includes confirmed teardown/capacity retention, durable
provider references and exact restore, mint readiness, brokered DevEnv credentials,
durable billing settlement, and credential cleanup despite billing failures.
F-20260905-005 explicitly retains the discovery/reconciliation debt for creation
before binding and historical unknown rows. Its Northflank work is additional to
the canonical Cloudflare reconciliation packet; Cloudflare capabilities do not
prove Northflank cancellation or identity. Unknown resources retain capacity.

Pending owner answers, already requested:

- Exception to the `CLAUDE.md:138–147` sibling fence for applying the reviewed
  `clw-required-hit.patch` in an isolated `corelink-workspaces` worktree.
- The same explicit exception for applying `devenv-authorized-start.patch` and
  its local migration in an isolated `corelink-server` worktree. Both patches
  are reviewable artifacts, not applied upstream capabilities or releases.
- Whether `max_vcpu_h` is shared by Runners and DevEnv or separate per product.
  Current DevEnv entitlement admission does not enforce that monthly ceiling.
- The owner-approved non-Cloudflare monitor runtime/account/alert capability.
  Do not invent O-MONITORHOST approval or a WORM journal from mutable storage.

Do not repeat these permission questions without checking for an answer. An
elapsed timeout is not approval. Continue independent authorized implementation
when possible, and retain the production containment interlocks until their
version-bound recovery prerequisites are met.

The A3.1/A3.2 engine/Worker teardown contract was pulled forward together for
T3-W10 correctness: the Worker returns 503 on an unconfirmed destroy. T3-W1
now has complete prepared source in Sprint 2; runtime verification, integration
dependencies and delivery acceptance are not waived.
