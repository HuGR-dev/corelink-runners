# Session state — go-live remediation campaign, 2026-08-31

**Read this to continue the campaign.** It is written for a different model, session or
harness, so it assumes none of this conversation. It states what is true, how each claim
was verified, and what is deliberately still open.

Baseline when this was written: `origin/main` = `26c74b8`.

---

## 1. What the campaign is

Remediating the **2026-08-30 ultra audit** (247 findings) until CoreLink Runners is
go-live. The plan is `docs/plan/2026-08-30-golive-remediation-plan.md` (rev-5): a
capability chain C1–C7, an acceptance suite of 92 live items (`A1.x`–`A7.x`, `★` = added
after the first cold-critic round), 46 work packages, and two mechanical gates that must
both stay green:

Both take an argument; run from the repo root. Verified PASS at `26c74b8`:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
```
```bash
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
```

`plan-check` proves the 247 findings are covered, disjoint and without orphans.
`wp-check` proves every live item is owned exactly once and no WP exceeds 4 items —
current output: 94 suite rows, 92 live (`A2.2` and `A5.7` withdrawn), 46 WPs, 89 items
owned, 3 judged items routed to an owner (`A4.9`, `A5.1`, `A7.3`).

Two more inputs exist and are NOT yet merged into the suite:
`docs/plan/union-catalog-ledger.md` and `docs/plan/union-triage-remaining.md` — 30 NEW
findings from the 2026-08-25 catalog, renamed into an `AU` namespace to avoid id
collisions. **They are merged into the suite only after the cold critic converges**
(doctrine: two consecutive quiet rounds; round 2 found 15 gaps, round 3 never ran).

---

## 2. Live production state, and how each line was verified

| what | state | how it was verified |
|---|---|---|
| `corelink-fabricd` | **UP**, on the durable pg ledger | `scripts/ops/fabricd-boot-rate.sh` — 6/6, 6/6, 5/5 SERVED on a fixed config |
| fabricd auth | fail-closed and correct | `/v1/usage` → 401, `/v1/attestation/key` → 200 |
| fabricd container logs | **ON** (`observability.enabled`) | `wrangler containers info` shows `configuration.observability.logs.enabled: true` |
| `corelink-spawn-worker` | deployed, reaper fixed + capped | `stale_box_reaped` firing; max 40 reaps per invocation, matching the cap |
| runner leak | closed | the 6 h 22 m box was evicted; `reap_skipped_unverifiable` went 279/tick → 0 |
| `sbox:` / `spawn:` KV | normal churn, NOT a leak | claims measured at ~1.95 h against their documented 2 h TTL |

**Never conclude a rate from one probe.** Everything above is a rate on a fixed config,
because an identical configuration produced both a serving and a dead control plane
during this outage. `scripts/ops/fabricd-boot-rate.sh` exists for exactly this and
deploys nothing.

---

## 3. What landed today

| PR | what | why it matters |
|---|---|---|
| #524 | fabricd outage root-caused | a quota-suspended Neon project; `PgLedger::connect` fail-closes **before** `TcpListener::bind`, so a refusing database stops the control plane from ever serving |
| #525 | runner leak | `reapStaleBoxes` required a non-empty `inst`, which COLD spawns never carry, so those boxes were structurally unreapable — 279 records aged 7.4–22.3 h |
| #526 | ★A3.16 + ★A3.17 | the admission fail-open is now budgeted; the three cold causes no longer collapse into one silent return |
| #527 | durable ledger restored | the quota returned **and** the loop eating it is gone |

### The one thing to understand about #527

The crash loop was not only a symptom, it was the **consumer**. The keep-warm cron
retried every minute, each retry dialled the database, and a scale-to-zero database
woken every 60 s never autosuspends — so the outage was burning the compute allowance
whose exhaustion was refusing it. A healthy fabricd sleeps after 5 m idle, which lets
the database suspend. That is why the ledger could simply be turned back on.

`FABRIC_PG_DISABLED` remains documented at its var in
`deploy/cloudflare-fabricd/wrangler.jsonc` as an emergency bridge. **If the quota
recurs, move the database rather than living behind the switch** —
`docs/handoff/2026-07-03-PLAN-postgres-provisioning-and-vcpu-ceiling-arm.md` already
sanctions Supabase alongside Neon, a free project suffices, and the connection must be
direct or SESSION-mode (tokio-postgres uses prepared statements and deadpool holds
connections across calls, which transaction-mode pooling breaks).

---

## 4. Open work, in the order recommended to the owner

1. **Alert on `spawn_cold_mint_key_unarmed`, then arm `REQUIRE_MINT_KEY=1`.** The
   counter exists as of #526. Arming fail-closed *without* an alarm trades a silent
   misattribution for a fleet-wide CI stop, which is the same shape that caused the
   twelve-day outage. Watch it sit at zero for a few days first.
2. **★A4.10 — prove money past ingest.** Nothing has ever reached an invoice or a
   charge for a test tenant. Until that exists, everything else is plumbing for a tap
   nobody has confirmed bills. This outranks item 3.
3. **★A3.18 — `claimSpawn` is a non-atomic `get` → `put`** and `if (!kv) return true`
   fails open, so two deliveries of one `workflow_job.queued` can double-spawn. The
   honest fix is a DO-backed claim; the code comment already says as much. It is a real
   design change — do not improvise it. Its cost is bounded (an occasional duplicate
   box, now under the admission budget), which is why it sits below A4.10.
4. **Cold critic round 3** on the acceptance suite, then merge the 30 `AU` union items.
5. Five findings remain in `DEFER-needs-waiver` and need explicit owner waivers —
   `docs/plan/plan-check.py` names them.

---

## 5. Instrument traps — each of these cost hours or days

Do not re-learn these.

1. **`exitCode: 0` from `@cloudflare/containers` is a hardcoded placeholder**, not a
   process exit status (`dist/lib/container.js:1597`, the `!container.running` branch).
   A real code arrives only via the `stopped_with_code` branch. fabricd's real code was
   `1` the whole time. Reasoning from that `0` produced several dead ends.
2. **`wrangler tail` does NOT carry container stdout/stderr**, even with observability
   enabled. Verified with a control image that ran to a real `exit code: 1` in the same
   application and still emitted nothing. Container logs need a Workers Logs
   observability scope the wrangler OAuth token does not hold.
3. **So make the image report on itself.** `crates/corelink-fabric-server/Dockerfile.bootprobe`
   + `bootprobe-entrypoint.sh` beacon to the Worker's own URL with the payload **in the
   URL path** (tail shows `event.request.url`, never bodies). Send legible text, not
   base64 — an opaque blob is redacted as a suspected secret and the payload is lost.
   Build it with `gh workflow run build-fabricd-image.yml -f image_name=… -f dockerfile=…`.
   **Diagnostic only. Never pin it in a steady deploy.**
4. **`wrangler kv key list` reads LOCAL storage by default.** Without `--remote` it
   reported 0 keys against a namespace holding 8756. An empty result is not evidence
   until the instrument is shown to see.
5. **A tail capture spanning a deploy mixes versions.** Count `scriptVersion.id` per
   event before concluding a fix "did not take effect" — that exact false conclusion was
   drawn and retracted here.
6. **`vars` are declarative**: every `wrangler deploy` REPLACES the live values. Secrets
   survive. A var set in one branch's `wrangler.jsonc` is reverted by a deploy from
   another branch — which happened in this session (see §7).
7. **Registry introspection needs no docker daemon.** Mint pull credentials via
   `POST /accounts/{acc}/containers/registries/registry.cloudflare.com/credentials`
   with `expiration_minutes` (required), then read manifests and blobs over plain HTTPS.
   `/accounts/{acc}/containers/me` reports the account's real ceilings.

---

## 6. Constraints on the agent

- **The session fence is mechanized.** `.claude/settings.json` + the `PreToolUse` hook
  `.claude/hooks/forbid-sibling-paths.py` default-deny everything under
  `~/Documents/HuGR/` except this repo. Read-only single-command Bash against siblings
  is allowed; mutation is not.
- **`wrangler deploy` is intermittently blocked by the permission classifier.** It is
  not a capability gap. Retry once or twice; if it keeps refusing, hand the user the
  exact command rather than working around it.
- **Account creation and typing credentials are prohibited.** A new database project and
  its connection string are the owner's to create and to set
  (`wrangler secret put DATABASE_URL`). Everything on either side of that step is the
  agent's.
- **Deploying `deploy/cloudflare` rolls the `RunnerContainer` class only when the
  container config changes.** A code-only deploy reports `no changes` and kills nothing.
  Before any deploy that *does* change the container block, verify the fleet is idle:
  zero `in_progress`, zero `queued`, and no busy `cf-runner-*`.
- **Repo convention:** branch → PR → merge, gates green first. There is no branch
  protection (free plan + private repo), so the rule is manual discipline. Never
  `gh pr merge --auto`. After a push, `gh pr checks` can show the *previous* commit's
  green — match the run's `headSha` against the PR head before merging.

---

## 7. Mistakes made in this session, recorded so they are not repeated

- **Deployed from the wrong branch.** After committing the reaper fix, the branch was
  switched, so the tree on disk no longer carried it — and the owner was told to deploy
  from that directory. The deploy shipped `main`'s code. Always `grep` for the fix in
  the file on disk immediately before deploying.
- **An accidental deploy of the wrong Worker**, from a stale `cwd`. It uploaded a
  functionally identical version (`wrangler versions view` diff was metadata-only), but
  it was not intended. Use `--cwd` with an absolute path.
- **Shipped a change that removed a bound.** The reap widening dropped the GitHub-call
  ceiling both sibling sweeps carry for a stated reason. A cold review from another
  session caught it and added `REAP_MAX_VERIFY_PER_TICK = 40`. When widening the
  population a sweep touches, check what bounded the old population.
- **Called a healthy counter a leak.** `sbox:` and `spawn:` growth was flagged as
  alarming before being measured; the claims were ~1.95 h against a documented 2 h TTL.
- **Named a population wrong.** `inst: ""` was attributed to Option-C dispatch; it comes
  from COLD spawns (a repo webhook carries no `installation.id`, and
  `REPO_INSTALLATION_MAP` covers one repo). The fix was right, the diagnosis was not.

---

## 8. Canonical artifacts

| path | what |
|---|---|
| `docs/plan/2026-08-30-golive-remediation-plan.md` | the plan (rev-5) — suite, WPs, invariants, verification levels |
| `docs/plan/plan-check.py` · `wp-check.py` | the two mechanical gates; both must stay green |
| `docs/plan/2026-08-31-fabricd-investigation-state.md` | the outage investigation, its exclusions, and §8 remediation options |
| `docs/plan/union-catalog-ledger.md` · `union-triage-remaining.md` | the 30 unmerged `AU` findings |
| `scripts/ops/fabricd-boot-rate.sh` | the boot-rate instrument (observation only) |
| `crates/corelink-fabric-server/Dockerfile.bootprobe` | the self-reporting diagnostic image |
| `.claude/skills/corelink-container-triage/SKILL.md` | the container-triage evidence ladder and its traps |

---

## 9. First moves on picking this up

```bash
# 1. Is the control plane actually serving? A rate, never one probe.
scripts/ops/fabricd-boot-rate.sh 6 25

# 2. Is the reaper healthy and bounded? Expect max 40 reaps per invocation,
#    and reap_skipped_unverifiable at ~0.
cd deploy/cloudflare && npx wrangler tail corelink-spawn-worker --format json

# 3. Do the plan's own gates still pass? (both PASS at 26c74b8)
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
```

If any claim in §2 does not reproduce, trust the measurement over this document and say
so — every line here was true at `26c74b8` and nothing guarantees it still is.
