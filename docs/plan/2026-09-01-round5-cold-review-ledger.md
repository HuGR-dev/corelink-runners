# Round-5 cold-review ledger — post-incident documentation reconciliation

**Review input:** `3fe8d06` (planning HEAD; parent includes `b70deae`, incident PR #529) ·
**Date:** 2026-09-01 · **Result: NOT QUIET — 8/8 reviewers reported blockers**

Eight independent, read-only reviewers checked the current handoff, the round-4 ledger, incident
evidence, the staged AU triage, the three structure gates and their negative selftest. No reviewer
edited the input. This ledger records the blockers and their disposition; it does not freeze the
plan, promote AU, authorize a baseline, or authorize dispatch.

## Blockers

| # | category | blocker found at the review input | disposition / required consequence |
|---:|---|---|---|
| 1 | Evidence boundary | The incident establishes a refusing Postgres dependency in the **pre-bind** path, but does not identify `PgLedger` initialization versus billing-exporter initialization. | Correct the wording; keep D12/A1.11 red until both refusal paths have separate evidence. |
| 2 | Production state | The current merged containment is `b70deae` (#529): `FABRIC_PG_DISABLED=1` leaves an in-memory ledger and suspends durable replay, vCPU ceiling and billing export. | Treat as emergency containment only; no durable-ledger or go-live credit. |
| 3 | Resource evidence | The fixed inventory measured **3/3 fabricd instances inactive** at `2026-09-01T16:40:40Z`; this proves post-containment scale-to-zero, not the 10/10 boot-sensitive acceptance or durable recovery. | Retain the timestamped measurement, but do not promote it to acceptance evidence. |
| 4 | Observability | `spawn=401` is a separate observability-key drift. Fabric probes remain disabled to avoid the five-minute wake loop; the 401 is not a runner-burn diagnosis. | Repair and alert this key path independently; keep `FABRIC_PROBES_ENABLED=0` until a bounded no-wake re-enable proof. |
| 5 | Gate semantics | Structural PASS was easy to read as readiness. There are three primary gates and one negative selftest; none establishes production readiness or quietness. | State the distinction and run the exact commands below from one clean tree. |
| 6 | Provenance | `e8a9e78` and `eba6e8a` are historical planning/runtime snapshots, not the current input. An aggregate or transcript without a source SHA is not admissible evidence. | Label every transcript by SHA; review current `3fe8d06` separately from historical snapshots. |
| 7 | Decisions and staging | D11/D12 were visible but unresolved; AU4.18 also left owner-of-record precedence to the implementer. The AU test/probe splits were incomplete. | Stage D13, split AU4.16 and AU3.26, and keep all 30 findings / 33 ids outside the principal suite until signed decisions and quiet review. |
| 8 | Orchestration and gates | The combined DAG was not executable, scope collisions remained, and nine false-PASS classes bypassed the gates/selftest or CI path filters. | Add one canonical cap-8 DAG, exact scopes/dependencies, hardened gates, 18 negative fixtures and complete CI inputs. No freeze, dispatch or green credit follows from structural PASS. |

The quiet count remains **zero**. A documentation correction does not close the underlying
production, decision, or acceptance blocker.

## Mechanical contract

The three primary structure gates are:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md
```

The negative selftest is separate:

```bash
python3 docs/plan/gates-selftest.py
```

The expected structural shape is 247/247 findings, 94 physical/unique A rows (92 live), 47 WPs,
and 30 AU source findings mapped to 33 proposed AU ids. The selftest must block all 18 known
corruptions. These are structure results only: they do not make the plan quiet, frozen, green,
durable, or dispatchable.

## SHA-labelled evidence

| SHA | role | admissibility |
|---|---|---|
| `eba6e8a` | historical 2026-08-31 runtime-investigation snapshot | dated context only |
| `e8a9e78` | historical round-3 planning snapshot reviewed by round 4 | historical ledger input only |
| `b70deae` | merged containment commit for PR #529 | current production containment lineage |
| `3fe8d06` | current planning review input | Round-5 input; not a freeze baseline |

Review and operational transcripts must carry one of these (or the exact newer source SHA) and the
deploy/version tuple they describe. Do not merge historical `e8a9e78`/`eba6e8a` observations into a
current `3fe8d06` baseline.

## Repair round and future DAG

1. Re-run the three primary gates and negative selftest from a clean tree; retain their output.
2. Run a new independent cold repair round over the main plan, AU triage, incident evidence,
   round-3 delta, round-4 ledger and all gate code. Disposition every finding in a SHA-labelled
   ledger.
3. Keep the evidence boundary and separate spawn-key drift explicit; D11, D12 and D13 remain red
   owner decisions. Obtain two
   consecutive quiet rounds over byte-identical inputs.
4. Capture one clean, version-bound red acceptance baseline. Only then freeze and verify the
   byte-identical combined DAG with a maximum of eight concurrent agents.

The canonical future path is:

```text
freeze + baseline
  → verify combined DAG (cap 8; no parallel scope collisions)
  → Wave 0 / owner arming
  → Wave 1 ∥ serial Wave-2 worker chain
  → T2-W3 workflow closer
  → Wave 3 live proofs
  → Wave 4 post-decision work
```

Reserved serialization starts `T3-W17 → T3-W18` before the later worker chain reaches
`T3-W16 → T3-W15`, and includes `T1-W5 → T1-W6`. T6-W13 is the immediate key/alert lane;
the later re-enable is `T6-W13 → T6-W14`, with T6-W14 also waiting on T6-W12. These are future
edges, not current dispatch authority. The canonical source is
`docs/plan/2026-09-01-reconciled-dispatch-dag.md`, whose status remains NOT DISPATCHABLE.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**
