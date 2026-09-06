# Delivery checkpoint — 2026-09-05

Continue in `/private/tmp/corelink-delivery-sprint1-20260905`, branch
`delivery/sprint1-20260905`. The original checkout has pre-existing conflicts;
do not reset, clean, or use it as the integration tree. Earlier session-state
and session-handoff files remain historical inputs.

The mechanical source of execution state is
[`delivery-ledger.json`](../plan/delivery-ledger.json). Run
`python3 scripts/dev/delivery-ledger.py --check --report` to recover counts,
owners, findings and next actions. The canonical DAG retains all 70 WPs and
the agreed four sprint scopes. Six of twelve Sprint 1 WPs have complete code;
no new sprint or WP is recorded as delivered. Historical source records are
not independently recertified delivery.

Full CI and heavy test suites run only when the complete sprint is composed.
This checkpoint has successful Rust metadata compilation and TypeScript checks,
plus focused author tests recorded separately in the execution evidence. No
sprint CI, deployment, live provider action or sibling mutation was performed.
The current fan-out has one Luna executor for each of T4-W1, T4-W2, T3-W3,
T3-W2 and T8-W5, in `/private/tmp/corelink-wp-<lowercase_wp>-20260905`.
Root owns review, integration and ledger updates. Review corrections return
to the same executor. The source preparation tree for Sprint 2 is
`/private/tmp/corelink-delivery-sprint2-20260905`; pending hard dependencies
prevent delivery credit and entry into the active delivery stack.
T4-W1's first candidate retained the defective 7200-second attribution TTL;
T8-W5's first candidate lacked the actual suspension producer connection.
Both are under correction, not accepted implementations. T3-W1's redaction
candidate `a54470eabad870568ae8ab8fbeec83c0f45099ba` passed source review and
is held as prepared; authored Rust runtime tests await sprint validation.
T3-W2 occupies the completed executor lane. Consult the ledger and
live agent status before resuming any author task.

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
