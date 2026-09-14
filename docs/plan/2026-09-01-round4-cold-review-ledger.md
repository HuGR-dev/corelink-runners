# Round-4 cold-review ledger — rev-6 planning repair

**Reviewed snapshot:** historical `e8a9e78` planning input · **Date:** 2026-09-01 · **Result: NOT QUIET**

Eight independent read-only reviewers ran in parallel: four Sol critics over acceptance safety,
mechanical gates, incident coverage and the dependency graph; four Luna reviewers over document
reconciliation, mutation reproduction, handoff/memory alignment and PR integration. No reviewer
edited the snapshot it judged.

This round resets the quiet count to **zero**. The repairs below are normative, so the next cold
review must start from the resulting clean post-incident commit. No item is frozen, dispatched or
credited green by this ledger.

**Provenance correction.** `e8a9e78` is the historical round-3 planning snapshot reviewed here;
`eba6e8a` is the historical runtime-investigation snapshot. Neither is the current review input.
The current post-incident planning tree is `3fe8d06`, whose parent includes incident PR #529 merge
`b70deae`. Any review transcript or evidence copied forward must carry the SHA it actually read.

## Findings and disposition

| class | reproduced finding | disposition in rev-6 draft |
|---|---|---|
| Gate false PASS | `plan-check.py` collapsed a duplicated 248th source row through `set()` | Fixed: 247 physical and unique rows, grammar and duplicate checks are mandatory. |
| Gate false PASS | `wp-check.py` accepted a duplicate A row, Markdown ownership drift and scope drift because its internal dictionary was not reconciled to the document | Fixed: exact headings, physical uniqueness, kinds, wave tables, ownership and scopes are reconciled. |
| Gate false PASS | `au-check.py` normalized legacy WP aliases and hid a principal/AU collision | Fixed: legacy ids block; AU is validated as STAGING and explicitly cannot prove freeze. |
| Gate durability | none of the planning gates ran in CI or carried negative selftests | Fixed: `plan-integrity.yml` runs the three gates and `gates-selftest.py`; eight reproduced corruptions must block. |
| Acceptance contradiction | A3.14/INV-3 allowed a broker failure to spawn unattributed COLD while A3.17 required fail-closed mint behavior | Fixed for existing A rows: optional cache-only COLD is distinct; required identity/mint/entitlement/attribution obeys durable-store-or-retry with zero spawn side effects. |
| Acceptance placeholders | N, K, stated bounds/rates/tolerances and a contradictory boot threshold remained in the principal suite | Fixed with exact samples, times and tolerances; boot-sensitive probes are 10/10 except A1.8 at 20/20. |
| Runner burn control | A3.30 returned 503 for paused GitHub intake and arrived only after permanent re-drive work | Fixed in proposal: an early standalone WP records `paused` durably then returns 202; 503 is only persistence failure; intake and re-drive controls are independent. |
| Re-drive liveness | A3.29 could pass by never re-driving, and did not define unknown/ambiguous inventory | Fixed in proposal with a complete safety/liveness matrix and exactly one eligible replacement state. |
| Inventory safety | A3.31 tested only joined instances, conflated read/delete credentials and did not prove incomplete pagination refusal | Fixed in proposal with read-only O-CFINVENTORY, split complete enumeration, unjoined/ambiguous fixtures and zero destructive eligibility. |
| PG feedback loop | A1.11 allowed one reconnect per minute, the exact cadence that prevented autosuspend, and omitted exporter pre-bind failure | Fixed in proposal: demand-only singleflight, persistent exponential backoff, diagnostics-first bind for ledger and exporter refusal, and a passive no-request scale-to-zero proof. The incident establishes a **Postgres pre-bind failure class**; it does not distinguish `PgLedger` initialization from billing-exporter initialization. |
| Canary feedback loop | the real five-minute canary/fabricd wake loop, metrics-key 401 and safe re-enable contract had no item/WP | Fixed in proposed A6.21/T6-W13; fabric probes remain disabled until a non-waking isolated trial passes. |
| Cost guard | no independent detector joined provider inventory, durable attempts and idle demand | Fixed in proposed A6.20/T6-W12 with an external failure domain and explicit thresholds. |
| WP graph | cycles/inversions and overlapping scopes made the scheduler non-executable; legacy model names and cap-6 batches were stale | Dispatch remains blocked. Luna/Sol routing and cap 8 are recorded. T5-W2 repo work is split from T5-W6 publish proof, T2-W5 moved to Wave 3, O-CANARY has code→bind→deploy→proof order, broad docs scopes are narrowed, and T5-W2→T5-W3 plus T3-W4→T4-W4→T3-W10 are explicit serial edges. The full combined DAG is recalculated only after freeze. |
| Documentation | C1 was over-credited, AU counts conflated 30 findings with 31 ids, D11/D12 and the third gate were invisible, and the 2026-08-31 handoff was stale | Fixed in rev-6 draft and the new 2026-09-01 session handoff; the historical handoff remains unchanged. |

## Mechanical evidence after repair

From the repository root:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/gates-selftest.py
```

The first three commands are the primary structure gates; the fourth is the negative selftest, not
a fourth coverage gate. Expected structural result: 247 physical/unique findings; 94
physical/unique A rows, 92 live, 47 WPs; 30 AU source findings → 31 proposed acceptance ids; eight
corruptions blocked. These results are necessary structure evidence only. The round remains
**NOT QUIET** until an independent reviewer finds no new defect on a clean post-incident commit.

## Next review contract

1. **Done:** rebase the repair onto incident PR #529 merge `b70deae` and run the four commands above.
2. Treat current `3fe8d06` as the review input; retain `e8a9e78` and `eba6e8a` as SHA-labelled
   historical transcripts only.
3. Run and disposition Round 5 over the main plan, AU triage, round-3 delta, this ledger, incident
   evidence and all gate code.
4. Any accepted normative change keeps the quiet count at zero. Only a byte-identical follow-up
   review after one quiet round can become quiet round 2.

Until then: **NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
