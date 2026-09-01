# Session state — go-live remediation campaign, 2026-09-01

**Read this to continue the campaign.** This is a post-incident handoff for a new
session or model. It records the production containment, the planning state, and the
safe next moves. It does not replace the historical handoff from 2026-08-31.

The repository is `corelink-runners`. The current planning worktree is
`.claude/worktrees/golive-rev6-r3`, on branch `plan/golive-rev6-r3`. The immutable Round-11 review
input is `fd9b226d3bcda055092b5e34f0cf9adc41a802bd`, with the Round-10 input
`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`, the Round-9 input
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`, Round-8 input
`9f6e281ca617113a840ac268dcb680b258064c39`, Round-7 input
`289826e358050c7d6b4517fc8a21f79c733c7e32` and incident PR #529 merge `b70deae` in its ancestry.
Cold-review Round 11 was **5/8 NOT QUIET; 3/8 QUIET; quiet count 0** (five reviewers reported new
blockers and three found no new finding/signoff). Round 10 was **7/8 NOT QUIET; 1/8 QUIET; quiet
count 0**, and Round 9, Round 8, Round 7, Round 6 and earlier rounds are retained as historical
ledgers. The Round-11 repair tree is a later tree and has no SHA asserted by this handoff; its
review result cannot be transferred from `fd9b226d…`.

## 1. Production containment: the Cloudflare burn is stopped

PR #529 was merged as `b70deae`. The emergency PostgreSQL escape hatch remains armed:
`FABRIC_PG_DISABLED=1`. This is containment, not a durable go-live acceptance.

The second wake-up loop was also found and stopped. `corelink-canary` ran every five
minutes and queried fabricd status and health; that cadence matched fabricd's five
minute `sleepAfter`, preventing scale-to-zero. The canary now has
`FABRIC_PROBES_ENABLED=0`, deployed as version
`852277c1-9778-459f-b1ff-9d56fbe7c32f`. It skips those fabric probes while retaining
spawn metrics.

Executable canary fail-closed hardening is committed in planning history at `13ce612`: only the
exact value `FABRIC_PROBES_ENABLED=1` arms probes, while unset or malformed values remain off. PR
#530 merged that hardening to `main` as `65540afe15fb65bfd431b631acfc971a7b0a2331`; source delivery
does not prove a production deploy and does not authorize re-enable.

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

The current mechanical checks are the three primary gates, actionlint classification, the negative
selftest, Ruff, and the diff check:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/actionlint-check.py
python3 docs/plan/gates-selftest.py
find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
ruff check docs/plan
git diff --check
```

The shell selftest command is a local reproduction of the required CI discovery/execution contract;
the CI job and required-check/path-filter binding must also be verified. An unset or invalid canary
configuration must fail closed; the explicit containment flags remain `FABRIC_PG_DISABLED=1` and
`FABRIC_PROBES_ENABLED=0`.

The exact historical results remain bounded to their own clean inputs: Round 8
(`9f6e281ca617113a840ac268dcb680b258064c39`) recorded 28 blocked selftest corruptions; Round 9
(`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`) recorded a selftest that reported 38 while executing
40 mutations. On the Round-10 input (`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`), the plan still
carried the historical 41-fixture instruction while the selftest code advertised a different
target; that stale mismatch is itself a blocker. These facts must not be presented as the current
repair count.

The current repair selftest verifies **66 meaningful corruptions (57 prior + 9 Round-11 mutations)**.
This is a diagnostic of the later repair tree, not evidence from the immutable Round-11 input and
does not provide quiet, promotion, freeze, dispatch or green credit. Keep the literal result
separate from the review transcript and attach an externally supplied full SHA before treating the
repair tree as a review input; never use a self-referential hash.

Round 10's seven blocker categories remain historical and open until independently verified: final
monitor redeploy/provider-rearm ordering; total FIFO queue residence in every SLO clock; three
missing canonical tests; T1-W5 live-probe containment ancestry; A6.17's immutable seven-day
false-page window; stale gate/selftest instructions; and a broken pre-merge selftest fixture.
Round 11 adds the current identity, journal/scheduler, ACK, canary, interlock, signer-trust and
shell-CI blockers recorded in its ledger. Structural checks cannot close these semantic or evidence
gaps.

The current Round-11 ledger is
`docs/plan/2026-09-01-round11-cold-review-ledger.md`. Its exact input is
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and its result is **5/8 NOT QUIET; 3/8 QUIET; quiet
count 0**. The seven current blockers are: future T6-W14 identities are not presealed; A6.17's
journal is mutable/non-exhaustive and omits scheduler runtime; ACKs are unauthenticated/unbound;
unset or invalid canary configuration can fail open; interlock check/use races remain; signer trust
is absent; and shell selftests are not proven in CI. The current repair selftest verifies 66
meaningful corruptions (57 prior + 9 Round-11 mutations), but that result belongs to the later
repair tree and cannot alter Round 11's quiet count of zero or authorize promotion, freeze, dispatch
or green credit.

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
new finding. Its consolidated ledger is `docs/plan/2026-09-01-round9-cold-review-ledger.md`. Round
10 then reviewed exact clean input `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`: seven reviewers
found new blockers and one found no new finding. Its consolidated ledger is
`docs/plan/2026-09-01-round10-cold-review-ledger.md`. All five rounds' findings remain open in the
subsequent repair tree until each disposition is independently verified. Round 11's findings are
tracked separately against `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and are likewise open.

The current repair-in-progress split gives the external monitor base to **T6-W15**. The exact
required serial chain is **`T6-W15 → T6-W12` final provider/live deploy + active-final reproof
`→ T1-W6` durable-PG re-arm**; no re-arm may run against an earlier monitor/provider version.
The repair draft now
tracks 69 DAG vertices (48 principal, 9 staged-new and 12 AU), with 9 proposal-only staged-new WPs;
**T6-W15** is the new identifier for the 48th principal WP (A6.10), not a staged-new WP. These
numbers describe the later repair tree, not the exact Round-10 or Round-11 input, and do not change
either NOT QUIET result or the quiet count of zero.

Round 10 consolidated seven blocker categories: the exact final monitor/provider chain
`T6-W15 → T6-W12 → T1-W6`; total FIFO residence with at most one unacknowledged head per
source/credential lane and a ≤60-second enqueue-to-ACK/terminal bound; three missing canonical
tests; T1-W5 live-probe containment ancestry; A6.17's immutable seven-day window with continuous
attestation, independent sensitivity controls and gap restart; stale selftest/gate instructions;
and a broken pre-merge selftest fixture. They remain red and are recorded in the Round-10 ledger.

`D11` (memoize-miss contract), `D12` (Postgres refusal semantics) and `D13` (tenant
owner-of-record precedence) are visible in the round-3 delta and remain **staged/unresolved**; none
is owner-signed or dispatchable. Do not claim
freeze, go-live, or AU convergence from the mechanical PASS results. The three gates establish
structural consistency only, and the selftest establishes rejection of known structural
corruptions; none is semantic or tamper-proof evidence. The cold-review doctrine requires two
consecutive quiet rounds on byte-identical staged repair bytes, a promotion reset to quiet count
zero, and two further consecutive quiet rounds on byte-identical promoted bytes.

## 4. Safe next steps

1. Preserve the exact Round-11 review-input provenance at
   `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` (and the historical Round-10/Round-9/Round-8/Round-7
   inputs `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`, `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`,
   `9f6e281ca617113a840ac268dcb680b258064c39` and `289826e358050c7d6b4517fc8a21f79c733c7e32`),
   confirm the merge-base with `origin/main` is `b70deae`, and keep every repair tree separately
   SHA-labelled and clean before review.
2. Repair the seven Round-11 blocker domains in one newly signed tree: preseal the complete future
   T6-W14 identity and signer-trust sets; make A6.17's journal append-only, exhaustive and
   scheduler/runtime-backed; authenticate and bind ACKs; fail closed on unset/invalid canary
   configuration; make interlock check/use atomic and versioned; and wire every tracked shell
   selftest into the required CI lane. Keep `FABRIC_PG_DISABLED=1` and
   `FABRIC_PROBES_ENABLED=0` explicit and armed.
3. Run the five planning/checker commands, every tracked shell selftest, Ruff and the diff check
   from the repository root:

   ```bash
   python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
   python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
   python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
   python3 docs/plan/actionlint-check.py
   python3 docs/plan/gates-selftest.py
   find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
   ruff check docs/plan
   git diff --check
   ```

   The shell command locally reproduces the CI discovery/execution contract. Verify the CI job's
   path filters and required-check binding separately; local PASS is not CI evidence. Preserve the
   verified 66-mutation result (57 prior + 9 Round-11 mutations) and pair it with an
   externally supplied full SHA before treating it as a review transcript; do not use an
   unqualified `HEAD` or embed a self-hash.
4. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change creates a new input and resets the staged quiet count to zero. Round 11 is
   `fd9b226d…`; no later repair tree can inherit its result. D11/D12/D13 and live obstacles remain
   red even if a review is quiet.
5. Promote the staged suite/checker bytes only in a new signed, full-SHA promotion commit. Promotion
   resets quiet count to zero; its result cannot inherit either staged quiet round.
6. Run two consecutive quiet cold reviews against the byte-identical promoted bytes. Only after
   those two promoted quiet rounds capture one clean, version-bound red baseline, freeze the plan,
   verify the combined DAG at cap 8, create implementation branches/PRs, and require green checks
   before a **manual** merge. No freeze or dispatch occurs before that sequence.
7. Keep both containment flags unchanged until their separately gated replacement/re-enable
   evidence exists; repair the separate spawn-metrics key and alert path independently. No current
   review result authorizes promotion, freeze, dispatch, green credit or live mutation.

Safe read-only checks:

```bash
git fetch origin
git show --no-patch --decorate b70deae
git status --short --branch
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/actionlint-check.py
python3 docs/plan/gates-selftest.py
find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
ruff check docs/plan
git diff --check
```

## 5. Provenance and future DAG

Keep review transcripts SHA-labelled. `eba6e8a` is the historical runtime-investigation snapshot;
`e8a9e78` is the historical round-3 planning snapshot used by the round-4 ledger; `b70deae` is the
merged containment commit (#529); `3fe8d06` is the historical Round-5 planning input;
`af4ed85dad289e333e9bf09f129fb2faa243136d` is the historical Round-6 review input;
`289826e358050c7d6b4517fc8a21f79c733c7e32` is the exact Round-7 review input;
`9f6e281ca617113a840ac268dcb680b258064c39` is the exact Round-8 review input; and
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f` is the exact Round-9 review input;
`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` is the exact Round-10 review input; and
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` is the exact Round-11 review input. These are distinct
artifacts and must not be combined into one baseline or one unlabelled transcript. Round-7's,
Round-8's, Round-9's, Round-10's and Round-11's clean/signoff observations are bounded to their respective
exact inputs; none is a claim about the later repair tree, an unqualified `HEAD`, or a
self-referential future hash.

The canonical draft graph is now `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; it is
mechanically acyclic and capped at eight, but explicitly **NOT DISPATCHABLE**. Round 11 remains NOT
QUIET (5/8 NOT QUIET; 3/8 QUIET; quiet count 0); its mechanical checks do not establish semantic
or tamper-proof proof. After two quiet rounds on the byte-identical
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
`T6-W13 → T6-W14`, with T6-W14 also waiting on T6-W12. The repair split's exact durability chain
is `T6-W15 → T6-W12 → T1-W6`, with T6-W12 carrying the final provider/live deploy and active-final
reproof before T1-W6's re-arm. The future T6-W14 identity set (owner, exact paths, capabilities,
inputs/outputs and all predecessors) must be presealed and signer-trusted before any of those
future edges can be considered. These are future edges, not current dispatch authorization.

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
