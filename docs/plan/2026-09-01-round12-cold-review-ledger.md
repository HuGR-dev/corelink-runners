# Round-12 cold-review ledger — fail-closed fences, evidence freshness and checker coverage

**Review input:** committed `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` · **Date:** 2026-09-01 ·
**Result: 7/8 NOT QUIET; 1/8 QUIET; quiet count 0** (seven reviewers reported new blockers and
one reported no new finding/signoff).

Round 12 was a fresh, read-only review of that exact input. The result is bounded to
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`; it does not describe a later repair tree, an
unqualified `HEAD`, or production state. The single signoff does not outweigh the seven blocker
reports and does not advance quietness.

This ledger records the Round-12 repair queue. It does not promote AU, freeze the plan, authorize
dispatch, or turn structural checks into semantic, evidence or production readiness. Round 11's
exact input `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and its ledger remain historical; a repair of
this input creates a new review input and cannot inherit quiet credit.

## Consolidated blockers

| # | category | consolidated Round-12 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | PG server fence | The server-side PostgreSQL refusal/fence was not specified as an authoritative, fail-closed admission boundary across every relevant server path. A client-side flag or partial startup guard could leave a request path able to proceed during refusal or drift. | Define one server-enforced fence with an explicit generation/epoch and refusal state. Every admission, ledger and exporter path must consult it before side effects; refusal, stale generation and unavailable fence state fail closed, are durable/auditable, and have deterministic negative tests. |
| 2 | Human-page ACK authentication | A human page acknowledgement was not bound to an authenticated operator, exact incident/page, action, and current monitor tuple. A copied, stale or arbitrary acknowledgement could release a gate. | Require a signed, role-authorized human ACK bound to incident id, page id, action, payload and monitor tuple, with signer epoch/trust/revocation checks, expiry and replay rejection. Test wrong operator, old page, altered body and duplicate cases before release. |
| 3 | Journal completeness / non-equivocation | The evidence journal did not prove that all relevant pages, ACKs, controls and attestations were recorded once, in order, without omission, rewrite or equivocation. | Use append-only/WORM storage with an exhaustive bounded manifest, ordered ids, counts, hash-chain/root commitments and immutable provider receipts. Detect gaps, duplicates, rewrites, omitted records, mixed windows and conflicting manifests; any failure restarts the evidence window. |
| 4 | Trusted time / freshness | Freshness and expiry relied on an insufficiently trusted clock or did not bind all evidence to a common time source. Clock skew, rollback or stale attestations could appear current. | Bind pages, ACKs, attestations and control records to a trusted monotonic/time-source identity, with bounded skew, explicit freshness/expiry and rollback detection. Unknown, conflicting, stale or future-skewed time fails closed and is covered by deterministic tests. |
| 5 | Canary activation-tuple contradiction | The canary activation predicate and its sealed tuple did not agree on which configuration, runtime and key material constituted the active probe. A canary could be considered armed or proven under contradictory tuple views. | Define one canonical activation tuple and require the canary, verifier and rearm decision to use byte-identical fields/digest. Any tuple/config/runtime/key drift disables probes and requires reproof; tests cover unset, malformed, stale and contradictory tuple inputs. |
| 6 | Stale triage doctrine | The triage instructions still permitted historical assumptions or stale evidence to be treated as current, including ambiguous ownership and containment/re-enable decisions. | Make triage explicitly version- and timestamp-bound: classify current versus historical evidence, preserve SHA provenance, identify owner/decision authority, and prohibit re-enable, deletion or dispatch from stale or ambiguous observations. |
| 7 | Producer ACK test cycle | The producer lanes did not have a complete, canonical test cycle proving signed ACK creation, durable ingest, verification, duplicate stability and fail-closed next-action behavior. | Exercise every tracked producer lane through emit → durable ingest → verify → ACK → retry/duplicate → next action, with exact envelope binding and negative tests before the action. A missing lane or unverified cycle is a blocker. |
| 8 | Signer-rotation recovery | Signer rotation and recovery were not fully specified as an atomic transition. A rotated, unavailable, stale or revoked signer could leave the system accepting old material or unable to recover without weakening trust. | Preseal active/next/revoked signer ids and epochs, overlap rules, rotation evidence and recovery custody. Verify old/new boundary behavior, rollback prevention, revocation, verifier restart and loss-of-primary cases; recovery must fail closed without bypassing the tuple. |
| 9 | fabricd idle no-wake | The idle/scale-to-zero contract did not prove that fabricd and its observers remain quiescent without periodic probes, retries or hidden wake paths. A monitoring loop could recreate the resource burn after containment. | Make idle no-wake an executable invariant: no fabric fetch, retry, socket or scheduled wake while probes are disabled and no work is present. Keep spawn metrics independent, bound retries, and test cold start, idle sleep, restart and malformed configuration paths. |
| 10 | Canary fail-visible behavior | When a probe is disabled, unhealthy, misconfigured or unable to obtain evidence, the canary could report a green/ambiguous result rather than a visible degraded state. | Emit an explicit fail-visible status with reason, version, tuple and timestamp; distinguish SKIPPED, FAILED, UNKNOWN and SERVED. No skipped or unverifiable health probe may count as green, rearm evidence or quietness. |
| 11 | Exact-`0` PG-flag enablement | The PG escape-hatch flag's enable/disable semantics were not constrained to the exact intended value, leaving unset, malformed or alternate strings able to alter containment unexpectedly. | Keep containment/PG-off armed only for the exact string `FABRIC_PG_DISABLED=1`; only the exact string `FABRIC_PG_DISABLED=0`, after all other gates are satisfied, may rearm PG. Unset, blank, whitespace, case variants, booleans and every other malformed or alternate value remain fail-closed/off. For the canary, only the exact string `FABRIC_PROBES_ENABLED=1` arms probes; unset, blank, whitespace, case variants, booleans and every other value keep probes off. |
| 12 | O-CFRATE | The O-CFRATE objective and acceptance evidence did not establish an explicit bounded cost/failure-rate budget across the complete observed interval, including retries, idle wakeups and failed attempts. | Define O-CFRATE's numerator, denominator, interval, source, owner, alert threshold and terminal evidence. Include every retry/wakeup/failure class and require a version-bound, independently verifiable measurement; missing or partial accounting is not PASS. |
| 13 | Checker exclusion / workflow false-PASS | The planning checker and workflow wiring could exclude relevant files, mutations or selftests while still returning PASS. Local success therefore did not prove that the required CI lane exercised the full contract. | Make discovery/path filters, required-check binding and mutation inventory exhaustive and fail closed. Add negative fixtures for excluded files, skipped jobs, altered workflow paths and false PASS; report the exact mutation count only after the checker agent reproduces it on the repaired tree. |

The seven blocker reports overlap within these thirteen failure domains; each domain is retained
once. The Round-12 signoff is bounded to the exact input and does not provide quiet, freeze,
dispatch or green credit.

## Evidence and status boundary

The repaired Round-12 negative selftest reports **131 meaningful corruptions blocked (66 prior +
65 Round-12/repair-audit mutations)** with the literal output `plan gate self-test: PASS — baselines accepted
and 131 corruptions blocked`. Do not infer review credit from that diagnostic, an unqualified
`HEAD`, a dirty tree or a self-referential hash. Pair it with the externally supplied full SHA of a
clean signed repair input before treating it as version-bound structural evidence.

A read-only containment check at `2026-09-01T21:14:17Z` recorded fabricd `0/3` active and flags
`FABRIC_PG_DISABLED=1` / `FABRIC_PROBES_ENABLED=0` (the `1/0` flag shorthand). PR #530 merge
`65540af` is source-only evidence, not a production deploy or re-enable authorization. No live
Cloudflare mutation was performed or authorized by this review.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**

## Required next sequence

1. Preserve `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` as immutable Round-12 provenance. Repair
   all thirteen domains in a new signed tree while retaining unresolved Round-11 obligations and
   both containment flags.
2. Run the planning/checker gates, every tracked shell selftest, Ruff and the diff check from the
   repository root. Verify workflow path filters and required-check binding; local PASS is not CI
   evidence. The checker reports the literal current result: **131 corruptions blocked**.
3. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change creates a new input and resets staged quiet count to zero.
4. Promote only in a new signed, full-SHA promotion commit; promotion resets quiet count to zero.
   Run two further quiet reviews against the byte-identical promoted bytes.
5. Only after those reviews and one clean, version-bound red baseline may the owner discuss freeze,
   cap-8 DAG verification, implementation branches/PRs and manual merge. No promotion, freeze,
   dispatch or green credit follows from this Round-12 result.
