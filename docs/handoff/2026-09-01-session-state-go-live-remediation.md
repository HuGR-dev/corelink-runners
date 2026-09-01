# Session state — go-live remediation campaign, 2026-09-01

**Read this to continue the campaign.** This is a post-incident handoff for a new
session or model. It records the production containment, the planning state, and the
safe next moves. It does not replace the historical handoff from 2026-08-31.

The repository is `corelink-runners`. The current planning worktree is
`.claude/worktrees/golive-rev6-r3`, on branch `plan/golive-rev6-r3`. The plan is
deliberately **NOT FROZEN**: the current cold-critic round is **NOT QUIET**.

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

The `spawn=401` is a separate observability-key drift, not a container-burn loop.
The fabricd boot-rate evidence after containment was 6/6 SERVED; attestation returned
200 and usage returned 401 as expected for the tested auth state. A detailed inventory
at `2026-09-01T16:40:40Z` found 3/3 fabricd instances inactive and
`non_inactive=[]`.

The observed runner spike (802 instances between 18:00 and 21:00 UTC on 2026-08-31)
is historical and is not evidence of a current runner burn. The structural redrive
amplifier remains a planning concern, but no active runner leak was found. Do not
delete instances based on names alone.

## 2. Root cause and evidence boundary

The original fabricd loop was: one-minute keep-warm retries dialled a refusing live
database; `PgLedger::connect` failed before `TcpListener::bind`; the five-minute
container idle window never elapsed; the database could not autosuspend, consuming the
quota that caused the refusal. The escape hatch and canary probe disablement broke both
wake-up paths.

The evidence is operational containment, not a green durability claim. Keep the
following distinctions explicit:

- `FABRIC_PG_DISABLED=1` is an emergency bridge; it must not become the durable ledger
  architecture.
- The canary's skipped health probes are intentional and must not be reported as a
  successful fabric health check.
- The 401 spawn metric is a key/configuration problem to repair separately; it is not
  proof of a resource leak.

## 3. Planning and gate state

The main plan is `docs/plan/2026-08-30-golive-remediation-plan.md`. The current branch
has repaired visibility for the union-catalog additions, but the plan still requires a
fresh cold review before freeze.

Three mechanical checks are now relevant:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py
python3 docs/plan/gates-selftest.py
```

Observed on the current planning tree:

- `plan-check`: PASS, 247/247 findings covered (`W2=21`, `DEFER=5`).
- `wp-check`: PASS, 94 suite rows, 92 live, 47 WPs, 89 owned, 3 judged.
- `au-check`: AU STAGING PASS, 30 AU source findings and 31 proposed AU acceptance IDs
  are structurally owned exactly once; this is explicitly not freeze evidence. The 31
  count is intentional: AU3.23 has separate test and probe acceptance IDs.
- `gates-selftest`: PASS. It accepts all three baselines and proves eight corruptions
  block: source/A-row duplication, ownership drift, renamed headings, legacy AU alias,
  missing serial edge and AU ownership outside the canonical file scope.

The two union inputs are `docs/plan/union-catalog-ledger.md` and
`docs/plan/union-triage-remaining.md`. They are proposed AU scope, not yet a frozen
addition to the acceptance suite. The current cold critic identified unresolved gaps:
missing D11/D12 semantics, weak falsifiability in round-2 wording, the A3.16/A3.17
scope split, unbounded KV-miss behavior, redrive and kill-switch controls, and the
PostgreSQL refusal/circuit-breaker/alert path. The round-3 delta is recorded in
`docs/plan/2026-09-01-round3-remediation-delta.md`; its staged intake contract now
returns 202 only after a durable paused record and uses 503 only when persistence fails.

Do not claim freeze, go-live, or AU convergence from the mechanical PASS results.
Those checks establish structural consistency only; the cold-review doctrine requires
two consecutive quiet rounds.

## 4. Safe next steps

1. Verify that `origin/main` contains merge `b70deae`, then rebase the planning branch
   onto the current main. Preserve the incident commits and inspect the exact diff.
2. Run all four checks above plus `git diff --check` from the repository root.
3. Run a fresh read-only cold critic against the complete plan, union ledger, triage,
   and all three checkers. Record every finding; do not silently promote AU items.
4. Repair the planning documents/checkers, rerun their mutation probes, and repeat the
   cold review until two consecutive rounds are quiet.
5. Only then freeze the plan, create a branch/PR for implementation WPs, and require
   green checks before a **manual** merge.
6. Separately repair the spawn-metrics key drift and add an alert before considering
   fail-closed mint-key enforcement. Keep the canary probes disabled until a bounded,
   non-waking probe strategy is verified.

Safe read-only checks:

```bash
git fetch origin
git show --no-patch --decorate b70deae
git status --short --branch
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py
python3 docs/plan/gates-selftest.py
git diff --check
```

## 5. Rules and operational traps

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
