# F-20260905-008 source resolution (B2 W2 component)

The B2 source component for `F-20260905-008` is resolved in the paired integrated
trees, while the finding itself remains open for live qualification. The source
mapping below is bounded to the four W2 acceptance units in the canonical triage
and does not provide credit for `AU4.16b` or `AU3.23b`.

| acceptance unit | source proof | exact source tip | result |
| --- | --- | --- | --- |
| `AU4.16a` | Runner `deploy/cloudflare/test/suspension-revocation.test.ts`: durable suspension dispatch enumerates all attribution pages, uses exact `(jobId, tenant, patId)`, retains retry state after a failed revoke, and resumes after restart; paired server suspension/consumer suite | Runners source predecessor `020c4bebe`; immutable integrated Runner tree `tree23317f682abe82576f82c3d8e7e39c8e2e787457`; Server source predecessor `775ea1b2b`; immutable integrated Server tree `treeb0e51dfbde494a7b2e4c45b6ddac9cc20fad310b` | source PASS |
| `AU3.23a` | Runner `deploy/cloudflare/test/credential-revocation.test.ts`: failed completion revoke remains durably fenced and retries after restart; late registration is retained; paired server retry/failure and lifecycle-generation suite | Runners source predecessor `020c4bebe`; immutable integrated Runner tree `tree23317f682abe82576f82c3d8e7e39c8e2e787457`; Server source predecessor `775ea1b2b`; immutable integrated Server tree `treeb0e51dfbde494a7b2e4c45b6ddac9cc20fad310b` | source PASS |
| `AU4.17` | Runner completion-revocation tests assert missing or conflicting tenant attribution refuses before HTTP and never falls back to ambient tenant configuration; paired server attribution checks | Runners source predecessor `020c4bebe`; immutable integrated Runner tree `tree23317f682abe82576f82c3d8e7e39c8e2e787457`; Server source predecessor `775ea1b2b`; immutable integrated Server tree `treeb0e51dfbde494a7b2e4c45b6ddac9cc20fad310b` | source PASS |
| `AU3.24` | Runner suspension/completion credential tests assert exact identity, malformed authority refusal, terminal re-registration refusal, and bounded cursor traversal; paired server credential-route suite | Runners source predecessor `020c4bebe`; immutable integrated Runner tree `tree23317f682abe82576f82c3d8e7e39c8e2e787457`; Server source predecessor `775ea1b2b`; immutable integrated Server tree `treeb0e51dfbde494a7b2e4c45b6ddac9cc20fad310b` | source PASS |

Cold paired verification recorded **90/90 Server tests PASS**, **18/18 Runner
tests PASS**, and typechecks PASS. The Runner 18-test command covers
`credential-revocation.test.ts` and `suspension-revocation.test.ts`; the Server
90-test result is from the paired server repository at `775ea1b2b`.

The cited commit IDs are source predecessors used to compose the integrated
trees. The final DCO SHA mapping is captured in the external execution evidence
for the integrated trees; this commit intentionally does not repeat a
self-referential final SHA.

This source result does not prove lifecycle generation behavior in production,
legacy migration coverage, or CAS refusal timing. Those remain the open portion
of `F-20260905-008` and are assigned to W3 `T8-W6`: `AU4.16b` requires 3/3
independent 75-second suspension refusal runs and `AU3.23b` requires 10/10
independent 75-second completion refusal runs, each version-bound to the
deployed Worker. No W3/live PASS is claimed here.
