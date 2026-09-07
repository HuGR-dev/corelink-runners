# T4-W1 — R1 execution packet

Decision owner: D0/root. Integration owner: I0/root. Sole executor: Luna.
Canonical acceptance and sprint remain as recorded in delivery-ledger.json.
This packet defines one bounded execution step; it grants no whole-WP completion.

## Frozen action

Copy the49-line frozen implementation anchor into the existing scaffold. Preserve all method signatures, key encoding, no-TTL attribution, transaction and paging behavior. No policy decisions or D13 implementation in this extraction.

## Exclusive writes

- `deploy/cloudflare/src/lib/job_attribution_authority.ts`

All other paths are read-only, including index.ts, lib.ts, metrics.ts, contracts,
ledger, shared tests and wrappers. No independent interface changes. If an anchor
cannot be transcribed, report the exact conflict to D0; do not invent a replacement.
Root already replaced the relevant ContainmentDO methods with delegating wrappers.

Source to transcribe: `T4-W1-implementation-anchor.txt` in this directory.

## Baseline and verification

The dispatch message supplies one exact40-character baseline; before editing,
verify `git rev-parse HEAD` equals it. Existing green components are preserved;
the focused failing fixtures are recorded in `R1-baseline.json`.
Create the dependency symlink only if absent, pointing to
`/private/tmp/corelink-sprint1-cleanup-next/deploy/cloudflare/node_modules`.
From deploy/cloudflare run:

```sh
./node_modules/.bin/vitest run test/job-attribution-list.test.ts test/job-attribution.test.ts test/job-attribution-completion.test.ts --pool=threads --maxWorkers=1 --minWorkers=1 --silent
```

No full suite, CI, Cargo build, deployment, production probe or sibling mutation.
Poll a running test session to actual exit; never turn a session ID into success.
One code commit, DCO-signed with configured identity, `[skip ci]` in the subject,
and `Co-Authored-By: Codex <noreply@openai.com>` as a separate paragraph.
Return only: actual SHA, changed paths, exact tests/count/exit, unresolved mismatch.
Root runs the disjoint candidate checker against the recorded dispatch baseline,
then reproduces the focused seam suite on the composed candidate.
