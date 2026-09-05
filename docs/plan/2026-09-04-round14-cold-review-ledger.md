# Round-14 cold-review ledger — T3 source landing and repair boundary

**Date:** 2026-09-04
**Review input:** exact clean planning/source baseline `387c1b1cfa55f11ed1d219e5d671cc50a292d47d` (`origin/main`)
**Result:** **NOT QUIET; quiet count 0 · NOT FROZEN · NOT DISPATCHABLE**

This ledger records every Round-14 finding and does not close a finding by
renaming it, moving it, or adding a plan paragraph. The review input is the
pre-edit baseline; this documentation stack is a new normative snapshot and
must be reviewed again before any freeze or dispatch credit. No deploy, live
probe, restart, deletion, rearm, owner decision, or evidence mutation occurred.

## Current census and state

| measure | current value | interpretation |
|---|---:|---|
| canonical DAG vertices | 70 | registry count; not source-delivery count |
| deterministic batches / emissions | **23 / 70** | B00–B22; prose correction only, no graph change |
| source-delivery census | **15 / 70** | 14 historical Wave-1 deliveries plus the landed T3-W17 source packet; 55 remain |
| T3-W17 | `SOURCE_LANDED_REPAIR_REQUIRED` | code/test/evidence source is present, but R14 defects and complete focused validation remain open |
| A3.30 | **RED** | source landing is not acceptance, semantic, live, or green credit |
| T3-W18 | **BLOCKED** | no live deploy/probe until T3-W17-R14 is accepted by its own gates and predecessors |
| planning status | **NOT FROZEN / NOT DISPATCHABLE** | quiet count remains zero |

The earlier [Wave-1 handoff](../handoff/2026-09-02-wave1-implementation-state.md)
retains its historical **14/70** number under a superseded banner. It is not
rewritten to say 15/70. The current number is carried by this ledger, the
roadmap, and the backlog-resume handoff.

The source landing is anchored to the T3 Worker implementation commit
`d0447da` and its separate evidence-document commit `8721124`; those anchors
show delivery only. The evidence artifact is not a current semantic result and
its provenance/index sequence is itself an R14 obligation.

## Finding dispositions

All rows have a stable id, one of the required dispositions, and a closure
condition. `CONFIRMED` means the observation is retained as an open repair or
tracking item; it does not mean repaired. `REFUTED` is retained so the claim
cannot be reintroduced by later prose. `PENDING-REFUTATION` is not a green or
clean state.

| id | disposition | observation at the review input | required consequence / owner boundary |
|---|---|---|---|
| **R14-T3-01 claim-false liveness** | **CONFIRMED** | `lib.ts` `claimSpawn` returns a boolean and the drain branch at `runContainmentDrain` treats a false result as a break without an ownership-aware pre-effect protocol. | Freeze the typed pre-effect owner protocol in [`T3-W17-R14.md`](contracts/T3-W17-R14.md): `acquired`, `owned`, `busy`, `legacy_unknown`, `unavailable`; false/busy/legacy/unavailable leave the head and redrive `HELD`, issue no permit/effect, and never spin. |
| **R14-T3-02 schema-version boundary** | **CONFIRMED** | Version `1` is written by current containment records, but every route/effect/permit/binding/outbox/evidence boundary is not exhaustively refusal-tested against missing/unsupported versions. | Require one schema gate for every durable record, response, effect and evidence index; unsupported versions fail before mutation. |
| **R14-T3-03 expired HELD fence** | **CONFIRMED** | `reserveRedriveCandidate` has the expiry/reclaim path, but the exact `expires_ms === now`, stale-owner and post-eligibility non-reclaim seams are not a complete, named test contract. | Pin equality-at-now reclaim only for `HELD`; stale tuple is a no-op; `EFFECT_ELIGIBLE`/`COMPLETED` never reset or reclaim. |
| **R14-T3-04 repository case canonicalization** | **CONFIRMED** | `normalizeRedriveIdentity` validates and trims but preserves owner/repository case, permitting mixed-case spellings to form separate containment identities. | One canonical normalizer must be used before maps/keys/reservations/effects; canonical aliases and malformed aliases require explicit tests and fail-closed refusal. |
| **R14-T3-05 canonical job ids** | **CONFIRMED** | The webhook route accepts `^\d+$`; leading zeroes and unsafe decimal values can enter identity paths even though redrive records otherwise expect canonical decimal ids. | Refuse noncanonical decimal forms and values outside the safe integer bound at every route/key/effect boundary. |
| **R14-T3-06 repo-scoped containment binding** | **CONFIRMED** | `containmentEffectJobKey` and `bindContainmentSpawnClaim` currently use a job-only namespace (`containment:v1:job:<jobId>`), so identical job ids can collide across repositories. | Bind canonical repo/job/event/effect/permit/epoch/owner in one versioned record; no job-only authority. |
| **R14-T3-07 unbounded event scan/OOM** | **CONFIRMED** | `ContainmentDO.containedEventExists` lists the whole event prefix on each reservation transaction (`index.ts` around 604–609), with no bounded repo/job index. | Replace the all-prefix scan with a bounded transactionally maintained index; overflow fails closed and cannot append or admit. |
| **R14-T3-08 singleton-global authority** | **CONFIRMED** | The intended authority is `CONTAINMENT.idFromName("global")` and Wrangler migration v7, but current tests do not fully assert that no per-job/per-repo Durable Object or alternate authority can be introduced. | Add a singleton-global test and migration assertion; KV remains an idempotency cache, never sequencing authority. |
| **R14-T3-09 ordinary webhook reservation bypass** | **CONFIRMED** | `admitQueued` has an early normal/empty-backlog return, but the bypass is not protected by a route-level regression that proves no reservation lookup or reservation write occurs. | Keep ordinary empty-backlog webhook behavior outside redrive reservation and prove the reservation methods are not consulted. |
| **R14-T3-10 distinct invalid-config dedup** | **CONFIRMED** | Invalid-config keys combine switch and raw digest, but route/tick/retry coverage must prove distinct raw values and distinct switches never share a signal or counter increment. | Require separate durable identities and one `bumpOnce` per committed signal across immediate and scheduled delivery. |
| **R14-T3-11 100-event route/effect population** | **CONFIRMED** | Existing concurrency fixtures do not by themselves prove 100 distinct routed paused events become 100 ordered, uniquely bound effects. | Add an exact 100-event route-to-effect population, contiguous sequence, exact-once effects and no duplicate mutation proof. |
| **R14-T3-12 idle-status no-renew regression** | **CONFIRMED** | `runnerActivityVerdict` and `keepAliveLiveRunners` classify idle/offline/404 as no-renew, but the regression must remain pinned alongside unknown-renew behavior. | Keep idle/offline/404 no-renew and unknown-renew tests in the T3 corrective stack. |
| **R14-T3-13 ownership-aware claim architecture** | **CONFIRMED** | A boolean KV claim cannot distinguish exact owner, busy owner, legacy job-only claim, or unavailable/indeterminate storage before effect eligibility. | Singleton DO pre-effect owner ledger plus typed KV result; only exact pre-effect owner with no permit/effect may abort. Live producer and redrive paths join the same protocol. |
| **R14-PROV-01 evidence provenance/indexing sequence** | **CONFIRMED** | `T3-W17-containment-test.json` is tied to a historical evidence commit and lacks the R14-required append-only index fields, complete focused counts, external input SHA and self-reference refusal. | Generate versioned evidence only after focused tests, with source/contract/test tree digests, contiguous previous-root sequence, `live:false`, and no moving-HEAD/self-asserted SHA. |
| **R14-GATE-01 plan-integrity SHA comment spoof** | **CONFIRMED** | Plan-integrity completion can be spoofed by a comment/printed SHA instead of proving the checked-out `git rev-parse HEAD`, PR/merge ref semantics, and immutable completion record. | Phase-B gate repair must bind the result to the actual checked-out SHA and immutable completion record; no fix is claimed in Phase A. |
| **R14-GATE-02 dynamic/unknown `runs-on` false green** | **CONFIRMED** | The actionlint-check line regex can miss dynamic/unknown `runs-on` values inside YAML flow/inline mappings, allowing a false-green runner-label census. | Phase-B gate repair adds expression/inline/unknown fixtures and fail-closed exhaustive discovery. A comment-only `runs-on` mention is explicitly **not** a defect. |
| **R14-LEGACY-01 broader legacy per-job keys** | **CONFIRMED — tracked outside repair scope** | Legacy surfaces remain job-only or bare-key shaped (including `spawn:<jobId>`, `done:<jobId>`, `orphan:<jobId>`, `jtenant`, `jhandle`, bare job-id reads, and placement/evidence lookups). Writers/readers include `claimSpawn`, `bindContainmentSpawnClaim`, `recordOrphan`, `recordPlacement`, `clearPlacementRecord`, `fetchJobPlacement`, `recordStrandedJob`, `recordGhostContainer`, and the containment evidence writer/readers. | Record and audit separately; do not silently broaden this contract or claim those legacy keys repaired by R14. Acceptance remains blocked until those writers/readers receive a separate exact scope or repair. |
| **R14-DAG-01 emission prose mismatch** | **CONFIRMED** | The canonical B00–B22 ready-set already has 23 batches and 70 vertices, while prose said 22 batches/69 emissions. | Correct prose to 23/70 only; no edge, vertex, predecessor, or ready-set edit. |
| **R14-STATE-01 source landing is not semantic closure** | **CONFIRMED** | T3 code/test/evidence landed, but the above code/test defects and evidence provenance remain open. | Keep `SOURCE_LANDED_REPAIR_REQUIRED`, A3.30 RED, T3-W18 blocked, quiet=0; source delivery never becomes green by documentation. |
| **R14-REG-01 R1–R6 registry consistency** | **CONFIRMED** | R6 is consumed by the DAG/union triage while earlier plan prose and broad ownership language can still present only R1–R5 or overlap scopes. | One authoritative R1–R6 registry; exact owner/tuple schemas and disjoint paths; unresolved relay blocks named descendants. |
| **R14-DEC-01 D/O/R ambiguity gate** | **CONFIRMED** | D5, D6, D9, and D10 remain unresolved decision dependencies; silently changing their semantics or treating a missing owner outcome as satisfied would make dispatch unsound. | Use the non-branching unresolved schema below. Each named dependent row is blocked until the exact owner artifact is present; no agent selects an outcome. |
| **R14-LIVE-01 keepalive-burn claim** | **REFUTED** | Current keepalive code renews busy/unknown activity and does not renew idle/offline/404; the existing focused tests cover that behavior. | Preserve the no-renew regression. Do not state or infer an idle keepalive burn defect from this review. |

No `PENDING-REFUTATION` item is silently treated as clean. The legacy-key row
is confirmed as a tracking obligation while its broader semantic closure remains
outside T3-W17-R14; any future attempt to fold it into R14 requires a new exact
scope and review.

## Non-branching D/O/R gate schemas

These schemas represent unresolved state and intentionally do not choose an
owner outcome. A row naming one of these tokens is blocked until its exact
artifact validates. “Unknown”, missing, malformed, expired, stale, or
conflicting artifacts are `UNRESOLVED`, never a default branch.

```text
DECISION_GATE_V1=(schema_version,decision_id,status,question_digest,
option_set_digest,owner_identity,owner_role,owner_key_id,owner_key_epoch,
role_authority_digest,owner_artifact_id,owner_artifact_sha,review_input_sha,
canonical_payload_digest,revocation_state_digest,issued_at,expires_at,
consumed_at,signature_algorithm,signature_domain,signature)

OBSTACLE_GATE_V1=(schema_version,obstacle_id,status,capability_digest,
owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,
artifact_id,artifact_sha,review_input_sha,canonical_payload_digest,
revocation_state_digest,issued_at,expires_at,consumed_at,signature_algorithm,
signature_domain,signature)

RELAY_GATE_V1=(schema_version,relay_id,status,source_repo,source_commit_sha,
owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,
artifact_id,artifact_sha,review_input_sha,canonical_payload_digest,
revocation_state_digest,issued_at,expires_at,consumed_at,signature_algorithm,
signature_domain,signature)

R6_RELAY_V1=(schema_version,relay_id,status,source_repo,source_commit_sha,
owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,
tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,
a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,
test_artifact_digest,artifact_id,artifact_sha,review_input_sha,
canonical_payload_digest,revocation_state_digest,issued_at,expires_at,
consumed_at,signature_algorithm,signature_domain,signature)
```

The listed order is normative. For every gate, `canonical_payload_digest` is
`sha256(signature_domain || "\\n" || schema_version || "\\n" || canonical_bytes(all
ordered payload fields except canonical_payload_digest and signature))`; the
signature is Ed25519 over `signature_domain || "\\n" || schema_version || "\\n" ||
canonical_payload_digest || "\\n" || canonical_bytes(all ordered payload fields
except signature)`. Thus the digest excludes itself and the signature, while the
signature covers the domain, schema, digest, and every ordered field including
specialized R6 tenant/CAS fields. `owner_identity`, `owner_key_id`, `owner_key_epoch`, and
`role_authority_digest` are required even when the value is an explicit
unresolved sentinel; a missing field is malformed, not an implicit owner.
`revocation_state_digest`, `issued_at`, and `expires_at` make revocation and
expiry part of verification rather than prose. No registry row below has a
verified owner artifact yet.

The canonical serializer is versioned and domain-separated: `signature_algorithm=
ed25519-sha256-v1`, `signature_domain=corelink-gate/v1`, and the signature covers
the canonical payload digest plus every ordered field. Reject unless the schema,
serializer, algorithm/domain, signature, current revocation digest, and owner role
verify, `issued_at < now < expires_at`, and `consumed_at=null`; otherwise status is
not dispatchable. `SATISFIED` is the only dispatchable status; `UNRESOLVED`, `PENDING`,
`EXPIRED`, `REVOKED`, `CONFLICT`, and `UNKNOWN` block named descendants. R1–R6 use
one registry; cross-repo R6 cannot be self-attested here.

### Enforceable T3-W17 acceptance token

`R14_ACCEPTED_V1` is the only token that can make T3-W18 eligible after all other
predecessors are satisfied. Its exact canonical order is:

```text
R14_ACCEPTED_V1=(schema_version,token_id,status,producer_id,issuer_identity,
issuer_role,issuer_key_id,issuer_key_epoch,issuer_role_authority_digest,
subject_wp,source_seal_sha,contract_sha256,evidence_index_sha,review_input_sha,
artifact_id,artifact_path,artifact_sha,predecessor_digest,canonical_payload_digest,
revocation_state_digest,issued_at,expires_at,consumed_at,consumption_high_water,
signature_algorithm,signature_domain,signature)
```

The producer is exactly `T3-W17-R14-ACCEPTANCE-GATE`; its issuer must be the
owner-authorized review role represented by the identity/key/epoch/authority
fields, never the implementation author or live deployer. `artifact_path` is
exactly `docs/plan/evidence/T3-W17-containment-test.json`, `artifact_id` is the
version-1 evidence artifact id, `source_seal_sha` identifies the implementation
commit, `contract_sha256` identifies `docs/plan/contracts/T3-W17-R14.md`, and
`evidence_index_sha` identifies the embedded sequence record's current digest.
`subject_wp` is exactly `T3-W17`; `predecessor_digest` binds the accepted T3-W17
predecessor set and review input. `consumption_high_water` has the exact shape
`(consumer_id,counter,last_token_digest)` with `consumer_id=T3-W18`, an integer
counter, and the last consumed token digest; a token is single-use and cannot be
consumed at a counter lower than the recorded high-water mark.

The producer must issue this token only after the exact source, contract, focused
evidence/index, review-input, and predecessor digests validate. The verifier
requires `schema_version=1`, the exact serializer/domain/algorithm, valid issuer
role authority and current revocation digest, `issued_at < now < expires_at`,
`consumed_at=null`, unconsumed high-water sequence, and byte/path/hash equality
for every bound artifact. Any missing, stale, replayed, revoked, ambiguous, or
cross-repository token is `UNKNOWN` and leaves T3-W18 blocked; no plan paragraph
or structural PASS can issue or consume it.

Specifically, D5 (instance-delete token), D6 (legacy live-wire purge), D9
(N>1 fabricd flip), and D10 (independent CI host) remain unresolved. Their
dependent WPs are blocked by their exact token; this ledger makes no owner
choice and does not edit the dependency graph to make them disappear.

## Authoritative D1–D13 registry

This is the materialized decision registry consumed by the R14 planning stack.
Every row is `UNRESOLVED/RED`: no recommendation, schedule position, or
structural check is a decision artifact. `owner_identity`, key fields, and
review SHA are `—` because no independently signed owner record is present.
The blocked-descendant column is copied from the canonical plan/DAG; it is not
an inferred owner outcome.

Expected canonical artifact locations are fixed here even when the artifact is
absent. The path and artifact id are requirements, not evidence that the file
exists:

| id | expected artifact id and canonical location | present / review SHA |
|---|---|---|
| D1 | `DECISION-D1-RESOURCE-CEILING` · `docs/plan/evidence/decisions/D1-resource-ceiling.json` | absent / — |
| D2 | `DECISION-D2-DEVENV-QUARANTINE` · `docs/plan/evidence/decisions/D2-devenv-quarantine.json` | absent / — |
| D3 | `DECISION-D3-REPO-LICENSE` · `docs/plan/evidence/decisions/D3-repo-license.json` | absent / — |
| D4 | `DECISION-D4-ADMISSION-MODE` · `docs/plan/evidence/decisions/D4-admission-mode.json` | absent / — |
| D5 | `DECISION-D5-INSTANCE-DELETE-TOKEN` · `docs/plan/evidence/decisions/D5-instance-delete-token.json` | absent / — |
| D6 | `DECISION-D6-LEGACY-LIVE-WIRE` · `docs/plan/evidence/decisions/D6-legacy-live-wire.json` | absent / — |
| D7 | `DECISION-D7-OPENROUTER-ROTATION` · `docs/plan/evidence/decisions/D7-openrouter-rotation.json` | absent / — |
| D8 | `DECISION-D8-FREE-TIER` · `docs/plan/evidence/decisions/D8-free-tier.json` | absent / — |
| D9 | `DECISION-D9-FABRICD-FLIP` · `docs/plan/evidence/decisions/D9-fabricd-flip.json` | absent / — |
| D10 | `DECISION-D10-INDEPENDENT-CI-HOST` · `docs/plan/evidence/decisions/D10-independent-ci-host.json` | absent / — |
| D11 | `DECISION-D11-MEMOIZE-MISS` · `docs/adr/0011-memoize-miss-contract.md` | absent / — |
| D12 | `DECISION-D12-PG-REFUSAL` · `docs/adr/0012-pg-refusal-semantics.md` | absent / — |
| D13 | `DECISION-D13-RUNNER-TENANT-OWNER` · `docs/adr/0013-runner-tenant-owner-precedence.md` | absent / — |

Expected relay artifacts are likewise explicit. R1–R5 require an owner-signed
artifact at the listed local path; R6 requires an external immutable URI supplied
by the corelink-server owner plus the local immutable relay-index witness. None
is present in this repository:

| id | expected artifact id and canonical location | present / review SHA |
|---|---|---|
| R1 | `RELAY-R1-CAPACITY` · `docs/plan/evidence/relays/R1-capacity.json` | absent / — |
| R2 | `RELAY-R2-USAGE-BILLING` · `docs/plan/evidence/relays/R2-usage-billing.json` | absent / — |
| R3 | `RELAY-R3-ONBOARDING` · `docs/plan/evidence/relays/R3-onboarding.json` | absent / — |
| R4 | `RELAY-R4-PRODUCT-DECISION` · `docs/plan/evidence/relays/R4-product-decision.json` | absent / — |
| R5 | `RELAY-R5-RELEASE-DOCS` · `docs/plan/evidence/relays/R5-release-docs.json` | absent / — |
| R6 | `RELAY-R6-CAS-TENANT-ISOLATION` · external immutable URI **required** + `docs/plan/evidence/relays/R6-relay-index.json` | absent / — |

| id | status | owner role (identity unbound) | exact artifact / review SHA | blocked descendants or consumers |
|---|---|---|---|---|
| D1 | **UNRESOLVED/RED** | architecture/capacity owner | — / — | A4.9, A4.11, T3-W4, T4-W4, R1 |
| D2 | **UNRESOLVED/RED** | devenv/platform owner | — / — | A3.8, A4.8, T9-W1 |
| D3 | **UNRESOLVED/RED** | release/legal owner | — / — | C5, T5-W2, T5-W4, T5-W5, T5-W6 |
| D4 | **UNRESOLVED/RED** | architecture/ADR owner | — / — | T3-W5 |
| D5 | **UNRESOLVED/RED** | Cloudflare operations owner | — / — | orphan teardown, RC2 |
| D6 | **UNRESOLVED/RED** | product/legacy-surface owner | — / — | A7.3 |
| D7 | **UNRESOLVED/RED** | security/release owner | — / — | D3 and its C5 descendants |
| D8 | **UNRESOLVED/RED** | product/pricing owner | — / — | A5.6, A5.8, R4, T5-W4, T5-W5 |
| D9 | **UNRESOLVED/RED** | fabricd operations owner | — / — | — (no registered WP edge; any N>1 consumer is UNKNOWN/NON-DISPATCHABLE) |
| D10 | **UNRESOLVED/RED** | security/CI capacity owner | — / — | C6 credibility |
| D11 | **UNRESOLVED/RED** | memoize contract owner | `docs/adr/0011-memoize-miss-contract.md` / — | T6-W2 |
| D12 | **UNRESOLVED/RED** | server durability/finance owner | `docs/adr/0012-pg-refusal-semantics.md` / — | T1-W6 and durability-dependent proofs |
| D13 | **UNRESOLVED/RED** | governance/owner-of-record role | `docs/adr/0013-runner-tenant-owner-precedence.md` / — (absent) | AU4.18, T4-W1 |

`D13` is deliberately materialized as `UNRESOLVED/RED` with an absent
artifact; its staging text is not a recommendation or a substitute decision.
An implementation packet may not clear a row by editing this table.

## Authoritative R1–R6 relay registry

R1–R6 are one registry, not interchangeable prose labels. All six remain
`UNRESOLVED/RED`; no review SHA or satisfying artifact is currently available.
The consumer list is the exact named cross-row dependency, and an unresolved
relay blocks those consumers only.

| id | owner role (identity unbound) | status | artifact / review SHA | consumers |
|---|---|---|---|---|
| R1 | corelink-server capacity/architecture owner | **UNRESOLVED/RED** | — / — | T4-W4, T4-W7, A4.9/A4.11 |
| R2 | corelink-server usage/billing owner | **UNRESOLVED/RED** | — / — | T4-W2, T4-W7 |
| R3 | onboarding/account cross-repo owner | **UNRESOLVED/RED** | — / — | T5-W4, T5-W5 |
| R4 | product/owner decision role | **UNRESOLVED/RED** | — / — | A5.6, A5.8, T5-W4, T5-W5 |
| R5 | release/deploy and docs cross-TL owners | **UNRESOLVED/RED** | — / — | `deploy-06`, `docs-truth-20` closure |
| R6 | corelink-server CAS tenant-isolation owner | **UNRESOLVED/RED** | — / — | T5-W1, AU7.10; cross-repo artifact required |

For each row, the eventual signed relay must carry the full
`RELAY_GATE_V1` owner/key/authority, canonical, revocation, expiry, algorithm,
domain, and review-input fields. `R6_RELAY_V1` is only that full envelope plus
its tenant/CAS trial fields; R6 cannot be authored by this repository. A local
plan, test, or structural PASS cannot substitute for its sibling-repo artifact.

## Packet completeness requirements

The dispatch packet template in the remediation plan is binding for future
packets. It must name exact DAG paths and symbol allowlists wherever
determinable, plus: `FORBIDDEN ACTIONS`, `COMPLETENESS PROOF` (all owned
findings/files/predecessors and no broad glob), and `FOCAL VALIDATIONS` (the
behavior-level tests and negative fixtures). A packet without any one of those
fields is incomplete and not dispatchable. A broad scope such as `scripts/**`,
`deploy/**`, or a whole workflow family is narrowed to the canonical DAG row's
exact paths; if a path cannot be narrowed, the packet remains blocked pending a
human scope decision.

## Closure and review rules

- Structural plan/WP/AU/actionlint checks are local hygiene only; they do not
  close any row, make A3.30 green, freeze the plan, or authorize dispatch.
- The gate findings assigned to Phase B do not become repaired because the
  current Phase-A contract mentions them.
- The R14 source stack must receive fresh cold review over its exact committed
  bytes. Quiet count stays zero until the required consecutive quiet reviews
  and clean post-incident baseline are independently recorded.
- T3-W18 cannot consume R14 source landing as a live predecessor until T3-W17
  repair, focused evidence provenance, and all named hard gates are accepted.
