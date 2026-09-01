# Session state — go-live remediation campaign, 2026-09-01

**Read this to continue the campaign.** This is a post-incident handoff for a new
session or model. It records the production containment, the planning state, and the
safe next moves. It does not replace the historical handoff from 2026-08-31.

The repository is `corelink-runners`. The current planning worktree is
`.claude/worktrees/golive-rev6-r3`, on branch `plan/golive-rev6-r3`. The immutable Round-9 review
input was committed `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`, with the Round-8 input
`9f6e281ca617113a840ac268dcb680b258064c39`, Round-7 input
`289826e358050c7d6b4517fc8a21f79c733c7e32` and incident PR #529 merge `b70deae` in its ancestry.
Cold-review Round 9 was **NOT QUIET** (7/8 reviewers reported new blockers; 1/8 found no new
finding and signed off) and the quiet count remains zero. Round 8, Round 7, Round 6 and earlier
rounds are retained as historical ledgers. The Round-9 repair tree is a later tree and has no SHA
asserted by this handoff; its review result cannot be transferred from `f5df50d…`.

## 1. Production containment: the Cloudflare burn is stopped

PR #529 was merged as `b70deae`. The emergency PostgreSQL escape hatch remains armed:
`FABRIC_PG_DISABLED=1`. This is containment, not a durable go-live acceptance.

The second wake-up loop was also found and stopped. `corelink-canary` ran every five
minutes and queried fabricd status and health; that cadence matched fabricd's five
minute `sleepAfter`, preventing scale-to-zero. The canary now has
`FABRIC_PROBES_ENABLED=0`, deployed as version
`852277c1-9778-459f-b1ff-9d56fbe7c32f`. It skips those fabric probes while retaining
spawn metrics.

Verified canary run:

```text
fabric=404 health=SKIPPED spawn=401 | triggered=1 | no alerts
```

The `spawn=401` is a separate observability-key drift, not a container-burn loop. It remains
unrepaired and is not evidence of a current runner leak. The fabricd boot-rate evidence after
containment was 6/6 SERVED; attestation returned 200 and usage returned 401 as expected for the
tested auth state. Detailed inventories at `2026-09-01T16:40:40Z` and again at
`2026-09-01T17:52:13Z` found 3/3 fabricd instances inactive and `non_inactive=[]`; the newest record
was still the containment-era `2026-09-01T16:40:13Z` instance. These measurements prove
post-containment scale-to-zero; they are not durable-ledger or go-live credit.

The observed runner spike (802 instances between 18:00 and 21:00 UTC on 2026-08-31) is historical.
A separate read-only sample at `2026-09-01T17:52:13Z` saw 108 runner records (104 inactive, three
running and one stopped), including 59 created since 17:39Z. That sample coincided with four
in-progress `corelink-server` workflows and provider runners attached to their `corelink` jobs, so
it is measured CI load, not proof of the redrive leak. The structural redrive amplifier remains a
planning concern. Do not delete instances based on names alone.

## 2. Root cause and evidence boundary

The incident evidence establishes this bounded class: a refusing Postgres dependency was reached
before `TcpListener::bind`, while one-minute retries and the five-minute canary cadence kept the
container active. It does **not** identify whether the failing pre-bind initializer was
`PgLedger` or the billing exporter; that distinction remains unresolved and is owned by D12/A1.11.
The escape hatch and canary probe disablement broke both wake-up paths.

The evidence is operational containment, not a green durability claim. Keep the
following distinctions explicit:

- `FABRIC_PG_DISABLED=1` is an emergency bridge; it must not become the durable ledger
  architecture.
- The incident proves a Postgres pre-bind failure class, not a ledger-versus-exporter attribution.
- The canary's skipped health probes are intentional and must not be reported as a
  successful fabric health check.
- The 401 spawn metric is a key/configuration problem to repair separately; it is not
  proof of a resource leak.

## 3. Planning and gate state

The main plan is `docs/plan/2026-08-30-golive-remediation-plan.md`. The current branch
has repaired visibility for the union-catalog additions, but the plan still requires a
fresh cold review before freeze.

Three primary mechanical gates and one negative selftest are now relevant:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/gates-selftest.py
```

Observed on the exact Round-8 review input `9f6e281ca617113a840ac268dcb680b258064c39` before the
current repair-in-progress cycle:

- `plan-check`: PASS, 247/247 findings covered (`W2=21`, `DEFER=5`).
- `wp-check`: PASS, 94 suite rows, 92 live, 47 WPs, 89 owned, 3 judged.
- `au-check`: AU STAGING PASS, 30 AU source findings and 33 proposed AU acceptance IDs
  are structurally owned exactly once; this is explicitly not freeze evidence. The three extra
  rows are the live halves of the AU3.23, AU4.16 and AU3.26 test/probe splits.
- `gates-selftest`: PASS. It accepts all three baselines and proves 28 corruptions block,
  including physical/opaque rows, kind and summary drift, source→AU swaps, tombstoned aliases,
  HTML-comment hiding, phantom DAG nodes, evidence-route removal, capability/serial-order
  corruption and broad/narrow parallel scope collisions.

These 28-case results are structural evidence for the exact Round-8 input only. They do not make
the input quiet, semantically ready, production-ready, freeze-eligible or dispatchable, and they
must be reproduced on any later clean, signed repair tree rather than attributed to an unqualified
`HEAD`.

The two union inputs are `docs/plan/union-catalog-ledger.md` and
`docs/plan/union-triage-remaining.md`. They are proposed AU scope, not yet a frozen
addition to the acceptance suite. Round 5 identified weak falsifiability, the A3.16/A3.17 scope
split, unbounded KV-miss behavior, redrive/kill-switch gaps, Postgres refusal/alert gaps, DAG scope
collisions and false-PASS gates. The repair tree dispositions are recorded in
`docs/plan/2026-09-01-round3-remediation-delta.md`,
`docs/plan/2026-09-01-round5-cold-review-ledger.md`,
`docs/plan/2026-09-01-round6-cold-review-ledger.md` and the canonical DAG. Round 6 found seven
blocker categories spanning provenance, semantic acceptance, gate meaning, DAG dispatchability,
containment evidence and staged decisions; one reviewer signed off with no new finding. The
corrected AU registry is 30 source findings / 33 proposed ids, with T3-W5 as the 12th new AU WP and
4 existing-WP extensions. The staged intake contract returns 202 only after a durable paused
record and uses 503 only when persistence fails.

Round 7 found six reviewers with new blockers and two with no new finding; its consolidated ledger
is `docs/plan/2026-09-01-round7-cold-review-ledger.md`. Round 8 then reviewed the exact signed
repair input `9f6e281ca617113a840ac268dcb680b258064c39`: five reviewers found new blockers and
three found no new finding. Its consolidated ledger is
`docs/plan/2026-09-01-round8-cold-review-ledger.md`. Round 9 reviewed the exact signed repair input
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`: seven reviewers found new blockers and one found no
new finding. Its consolidated ledger is `docs/plan/2026-09-01-round9-cold-review-ledger.md`.
All three rounds' findings remain open in the subsequent repair tree until each disposition is
independently verified.

The current repair-in-progress split gives the external monitor base to **T6-W15**, before both
`T1-W6` durable-PG re-arm and `T6-W12` provider-inventory/cost live proof. The repair draft now
tracks 69 DAG vertices (48 principal, 9 staged-new and 12 AU), with 9 proposal-only staged-new WPs;
**T6-W15** is the new identifier for the 48th principal WP (A6.10), not a staged-new WP. These
numbers describe the later repair tree, not the exact Round-9 input, and do not change the 7/8 NOT
QUIET result or quiet count zero.

Round 9 consolidated seven blocker categories: independent-monitor host and credential separation;
end-to-end source/poll/retry/delivery timing; lifecycle and canary ordered evidence; durable shared
incident transitions and recovery; T6-W15/T6-W12 scope ownership and no-partial-credit rules;
dispatch/containment authorization boundaries; and the SHA-labelled evidence boundary. They remain
red and are recorded in the Round-9 ledger.

`D11` (memoize-miss contract), `D12` (Postgres refusal semantics) and `D13` (tenant
owner-of-record precedence) are visible in the round-3 delta and remain **staged/unresolved**; none
is owner-signed or dispatchable. Do not claim
freeze, go-live, or AU convergence from the mechanical PASS results. The three gates establish
structural consistency only, and the selftest establishes rejection of known structural
corruptions; none is semantic or tamper-proof evidence. The cold-review doctrine requires two
consecutive quiet rounds on byte-identical staged repair bytes, a promotion reset to quiet count
zero, and two further consecutive quiet rounds on byte-identical promoted bytes.

## 4. Safe next steps

1. Preserve the exact Round-9 review-input provenance at
   `f5df50d7659254ed5e4579ab75df2a4d44ceea0f` (and the earlier Round-8/Round-7 inputs
   `9f6e281ca617113a840ac268dcb680b258064c39` and
   `289826e358050c7d6b4517fc8a21f79c733c7e32`), confirm the merge-base with `origin/main` is
   `b70deae`, and keep the subsequent repair tree separately SHA-labelled and clean before review.
2. Run the three primary gates plus the negative selftest above, then `git diff --check` from the
   repository root.
3. Finish the Round-9 repair cycle in one newly signed commit and record its full SHA. Run all four
   structure checks, the current negative selftest target, actionlint classification and
   `git diff --check` from that clean signed input; do not substitute an unqualified `HEAD` or a
   dirty-tree transcript.
4. Run two consecutive quiet, read-only cold reviews against the byte-identical staged repair
   commit. Any normative change creates a new input and resets the staged quiet count to zero. The
   Round-9 review input is `f5df50d…`; a later repair tree cannot inherit its result.
   D11/D12/D13 and live obstacles remain red even if a review is quiet.
5. Promote the staged suite/checker bytes only in a new signed, full-SHA promotion commit. Promotion
   resets quiet count to zero; its result cannot inherit either staged quiet round.
6. Run two consecutive quiet cold reviews against the byte-identical promoted bytes. Only after
   those two promoted quiet rounds capture one clean, version-bound red baseline, freeze the plan,
   verify the combined DAG at cap 8, create implementation branches/PRs, and require green checks
   before a **manual** merge. No freeze or dispatch occurs before that sequence.
7. Keep `FABRIC_PROBES_ENABLED=0` until a bounded, non-waking probe strategy is verified; repair
   the separate spawn-metrics key and alert path independently.

Safe read-only checks:

```bash
git fetch origin
git show --no-patch --decorate b70deae
git status --short --branch
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/gates-selftest.py
git diff --check
```

## 5. Provenance and future DAG

Keep review transcripts SHA-labelled. `eba6e8a` is the historical runtime-investigation snapshot;
`e8a9e78` is the historical round-3 planning snapshot used by the round-4 ledger; `b70deae` is the
merged containment commit (#529); `3fe8d06` is the historical Round-5 planning input;
`af4ed85dad289e333e9bf09f129fb2faa243136d` is the historical Round-6 review input;
`289826e358050c7d6b4517fc8a21f79c733c7e32` is the exact Round-7 review input;
`9f6e281ca617113a840ac268dcb680b258064c39` is the exact Round-8 review input; and
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f` is the exact Round-9 review input. These are distinct
artifacts and must not be combined into one baseline or one unlabelled transcript. Round-7's,
Round-8's and Round-9's clean/signoff observations are bounded to their respective exact inputs;
none is a claim about the later repair tree, an unqualified `HEAD`, or a self-referential future hash.

The canonical draft graph is now `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; it is
mechanically acyclic and capped at eight, but explicitly **NOT DISPATCHABLE**. Round 9 remains NOT
QUIET; its mechanical checks do not establish semantic or tamper-proof proof. After two quiet rounds on the byte-identical
staged repair, a signed promotion (which resets quiet count), two quiet rounds on the byte-identical
promoted bytes, and the single post-incident baseline, the path is:

```text
baseline + freeze
  → verify the byte-identical combined DAG (cap 8; no parallel scope collisions)
  → Wave 0 / owner arming
  → Wave 1 ∥ the serial Wave-2 worker chain
  → T2-W3 workflow closer
  → Wave 3 live proofs (mint → PG/exporter → independent alert → canary, with declared deps)
  → Wave 4 post-decision work
```

The worker safety spine starts `T3-W17 → T3-W18` (repo kill switches → live re-drive-only arming)
before the later worker chain reaches `T3-W16 → T3-W15` (authoritative join → permanent redrive).
`T1-W5 → T1-W6` keeps mint and PG/exporter evidence separate. `T6-W13` is the immediate metrics-key
and alert lane while fabric probes remain disabled; the later no-wake re-enable is
`T6-W13 → T6-W14`, with T6-W14 also waiting on T6-W12. The repair split puts the monitor base in
T6-W15 before T1-W6 and T6-W12. These are future edges, not current dispatch authorization.

## 6. Rules and operational traps

- Follow `CLAUDE.md` and the maintenance/container-triage skills before touching live
  Cloudflare resources. The repository convention is branch → PR → green gates →
  manual merge; never use auto-merge.
- Never print, commit, or place secrets in plans, handoffs, logs, or commands. Account
  creation and credential entry remain owner actions.
- Before any deploy, use an absolute `--cwd`, verify the intended branch and file
  contents, and remember that Wrangler `vars` are declarative and replace live values.
- A deploy that changes the container block can roll the fleet. Before it, verify zero
  `in_progress`, zero `queued`, and no busy `cf-runner-*` containers.
- `wrangler kv key list` is local unless `--remote` is explicit. Tail captures can mix
  script versions. Container exit code `0` may be a library placeholder. These are
  evidence traps, not production facts.
- Do not merge a PR merely because `gh pr checks` is green: match each run's
  `headSha` to the PR head first.

This handoff intentionally contains no credentials or secret values. If live evidence
contradicts it, trust the fresh measurement, record the timestamp and version, and
update the planning state before making a deployment decision.
