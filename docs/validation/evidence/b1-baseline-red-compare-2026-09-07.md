# B1 local gate baseline comparison

Comparison performed on isolated worktrees at:

- baseline: `cda90940f74735c006693d9b9e3be85c37b26f1`
- candidate: `f27de8ff`
- date: 2026-09-07

The exact offline commands were run once per SHA:

```sh
scripts/ci/probe-freshness-check.sh
scripts/ci/claim-artifact-lint.sh
scripts/ci/image-pin-freshness.sh --strict
```

## Gate result

| Gate | Baseline | Candidate | Delta | Classification |
| --- | --- | --- | --- | --- |
| `probe-freshness-check.sh` | exit 1; `RED: coverage_status RED: no evidence credit is available` | same exit/status/message | none | unchanged baseline RED |
| `claim-artifact-lint.sh` | exit 1; 1,690 errors | exit 1; 1,775 errors | +85 | worsened |
| `image-pin-freshness.sh --strict` | exit 1; `pins=3 red=1` | exit 1; `pins=3 red=1` | none | unchanged baseline RED |

The claim-lint count is emitted after the script's normal 100-error diagnostic
cap. A full diagnostic rerun, using an analysis-only copy of the unchanged
script, confirmed the counts above. The candidate adds 86 errors in 34 newly
added paths; two existing paths report one fewer line match each because their
prose moved, giving the net increase of 85. The two line-count decreases are
not evidence of a semantic repair.

### Exact image-pin paths

All three pins are RED on both SHAs for `missing-recorded-build-sha`:

- `deploy/cloudflare/wrangler.jsonc`: runner container and check-host container
- `deploy/cloudflare-fabricd/wrangler.jsonc`: fabricd container

The candidate only shifts the reported lines (`218/265/326` to
`220/267/327`). It does not improve or worsen the pin result.

### Exact candidate-only claim-lint paths

The 86 candidate-only errors are:

- Unindexed evidence JSON (6):
  - `docs/plan/evidence/2026-09-07-T6-W4-acceptance.json`
  - `docs/plan/evidence/T3-W18-containment-live.json`
  - `docs/plan/evidence/T3-W2-lifecycle.json`
  - `docs/plan/evidence/T4-W4-admission.json`
  - `docs/plan/evidence/T6-W4-owner-config-template.json`
  - `docs/plan/evidence/T6-W9-alert-rules.json`
- New present-tense claim-marker errors (80):
  - `deploy/cloudflare-t9-w1-authority/README.md`
  - `deploy/cost-monitor/infra/verifier-runtime-ops.md`
  - `docs/adr/0011-memoize-miss-contract.md`
  - `docs/adr/0013-runner-tenant-owner-precedence.md`
  - `docs/plan/delivery-ledger.md`
  - `docs/plan/evidence/2026-09-06-T8-W4b-version-bound-execution-card.md`
  - `docs/plan/execution/2026-09-06-closeout-three-bundles.md`
  - `docs/plan/execution/2026-09-06-credential-lifecycle-implementation.md`
  - `docs/plan/execution/2026-09-06-monitor-runtime-contract.md`
  - `docs/plan/execution/2026-09-06-monitor-token-contract.md`
  - `docs/plan/execution/2026-09-06-monitor-wave2-contract.md`
  - `docs/plan/execution/2026-09-06-shared-compute-implementation.md`
  - `docs/plan/execution/2026-09-06-sprint1-acceptance-matrix.md`
  - `docs/plan/execution/2026-09-06-t6-w13-live-execution-card.md`
  - `docs/plan/execution/2026-09-06-t6-w15-authority-factory-supplement.md`
  - `docs/plan/execution/2026-09-06-t9-w1-d2-authority-decision.md`
  - `docs/plan/execution/2026-09-06-t9-w1-live-acceptance-card.md`
  - `docs/plan/execution/2026-09-07-t6-w2-workspaces-routing.md`
  - `docs/plan/execution/2026-09-07-t6-w4-live-execution-card.md`
  - `docs/plan/execution/2026-09-07-t6-w9-live-probe-card.md`
  - `docs/plan/execution/rework-20260906/T4-W1-packet.md`
  - `docs/plan/execution/rework-20260906/T4-W2-packet.md`
  - `docs/plan/execution/rework-20260906/T8-W5-packet.md`
  - `docs/plan/relays/clw-required-hit.md`
  - `docs/plan/relays/devenv-authorized-start.md`
  - `docs/plan/s1-b1-gate-blueprint.md`
  - `docs/validation/evidence/sprint1-t1-w5-acceptance-2026-09-06.md`
  - `docs/validation/evidence/sprint1-t8-w4b-acceptance-2026-09-06.md`

The marker-error subtotal is 80 emitted error instances across 28 newly added
Markdown paths; some paths emit multiple lines. Together with the six
candidate-only unindexed JSON errors, this is 86. The two unindexed files
`docs/plan/evidence/freshness-v1.json` and
`docs/plan/evidence/T3-W17-containment-test.json` are baseline errors and are
called out separately above.

## Ledger/DAG ownership

- The unchanged freshness RED is A7.6, owned by `T7-W4b` in the historical
  Sprint 0 record. The DAG defines `T7-W4b` as a hard predecessor for every
  probe/test+probe row. In the current ledger this directly gates the B1 rows
  `T3-W18`, `T6-W4`, `T6-W9`, `T6-W13`, `T6-W15`, and `T1-W5`; the dependency
  is consumed again by `T2-W2b`/`T2-W4` in Sprint 2 and later probe WPs.
- The unchanged broad claim RED is A7.4/A7.5, owned by `T7-W3` in the
  historical Sprint 0 record, with `T7-W4b` downstream. It has no separate
  future repair WP in the ledger. The candidate's newly added evidence and
  execution-card paths are therefore a real current regression and must be
  repaired/indexed by the owning B1/Sprint 2 WP or by the root ledger owner;
  they cannot be counted as inherited baseline debt.
- The unchanged image-pin RED is A2.3/`T2-W2a` report-only historical debt.
  `T2-W2b` in Sprint 2 owns the build/publish/deploy repair, and `T2-W4`
  consumes that result for image ship. The three unchanged missing build-SHA
  records do not block a baseline-equivalence waiver, but they do block those
  future image-delivery WPs until resolved.

## Finite promotion recommendation

`BLOCKED` for clean B1 promotion of `f27de8ff`.

The probe and image-pin REDs are baseline-equivalent and may be carried as
explicit known debt. The claim gate is not baseline-equivalent: it gained 85
errors, including six unindexed evidence artifacts and new unmarked claims
in current acceptance/ledger documents. Repair those candidate-only paths (or
record a deliberate, owner-approved gate waiver that explicitly excludes
claim credit), then rerun the same three commands on the resulting frozen
SHA. No full CI or GitHub Actions billing is required for this comparison.
