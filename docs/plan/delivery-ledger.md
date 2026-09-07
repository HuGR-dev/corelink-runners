# Delivery ledger

`delivery-ledger.json` is the execution record for three serialized operational
sprints. The canonical dispatch registry still owns WP identities and dependency
edges. The ledger preserves all 70 vertices, the exact 12/14/28 operational
scopes, and the historical 12/14/15/13 origin scopes; it does not replace the
acceptance catalog or count findings as WPs.

```sh
python3 scripts/dev/delivery-ledger.py --check
python3 scripts/dev/delivery-ledger.py --report
python3 scripts/dev/delivery-ledger.py --ready-for-ci 1
python3 scripts/dev/delivery-ledger.py --ready-for-delivery 1
```

These commands inspect records and Git objects. They do not launch CI, tests,
providers or deployments. Full CI and heavy tests run only at complete sprint
delivery, after all agreed implementation is composed. Small, necessary checks
may run during development. CI readiness does not require subsequent live
acceptance to have passed.

The 16 `recorded_delivered` entries preserve the previous session's source census;
they are historical records, not independently recertified sprint delivery.
`prepared` commits retain useful existing work without granting whole-WP credit.
The outgoing handoff and historical evidence remain dated source observations.

The integrator updates the ledger before delegating, after each author/reviewer
result, when integrating a commit, and after sprint validation/acceptance/merge.
Every active author has an owner and exclusive paths. Read-only investigators
have no write scope. Authors work in separate worktrees; only the integrator
changes this ledger and the delivery index.

Default allocation is one Luna executor per WP, responsible for implementation
and review corrections through acceptance. Additional agents on the same WP
require explicitly different, non-overlapping scopes recorded here before
delegation. Root coordinates, reviews and integrates; it does not duplicate
an executor's edits. Parallel authorship uses isolated worktrees and preserves
canonical integration dependencies. A returned commit is a review candidate,
not automatic implementation or delivery credit.

The owner explicitly invoked HuGR TechLead during this session. Its local entry
is `/Users/gustavoschneiter/.claude/skills/techlead/SKILL.md`, resolving to the
HuGR techlead repository. The lead read its contract, decomposition, packet,
verification and rolling-loop subskills. Apply rolling dispatch and integration:
freeze cross-WP interfaces before authoring; source-ready soft dependencies may
use those contracts, while real external/hard capabilities remain prerequisites.
An author return frees the execution seat and enters review; it is not a seal.
First-pass read-only review may run independently, with final judgment and
integration retained by root. Integrate approved source when its prerequisites
permit, without waiting for unrelated authors. The owner's sprint-only full CI
and heavy-test rule takes precedence over per-merge examples in the skill.
The Arsenal scheduler/relay tools are not exposed in this API session; do not
claim they were armed or that a SubagentStop hook enforced author completion.
Use the existing ledger, actual Git objects and explicit check results as the
execution record. Do not add permission gates or repeat plan decomposition just
to reproduce unavailable skill tooling.

A new finding receives a stable ID, affected WP and sprint, severity, owner,
reproduction, acceptance, and whether it blocks delivery. Fix it within the
current work whenever practical. Larger fixes stay explicit in the backlog;
security/correctness blockers prevent the affected delivery. Never silently
move a finding to a later sprint to turn a gate green. `fixed` requires a commit
and concrete evidence; it does not mean deployed. No unresolved debt is hidden
in comments or represented as completed work.

Review approval binds the actual implementation commit. Sprint CI binds the
composed tip. Acceptance evidence follows its actual deployed source/version.
A sprint is delivered only with complete implementation, resolved blockers,
review, successful complete-sprint CI, applicable acceptance and a merge record.
The checker verifies these records structurally; the integrator remains
responsible for inspecting raw evidence and external authority.
