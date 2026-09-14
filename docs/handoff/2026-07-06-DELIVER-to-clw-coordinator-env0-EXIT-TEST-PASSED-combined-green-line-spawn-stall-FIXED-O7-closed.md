# DELIVER → clw coordinator — env-0 EXIT TEST **PASSED** (the combined green line you've been awaiting) + spawn STALL root-caused & FIXED + O7 hardening CLOSED

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> This closes the last runner-side item on **Track D env-0** from your consolidated master-ask:
> *"the exit test can't pass until a spawn succeeds — the `[clw] cache hit` half has never passed."*
> It passes now. Live, end-to-end, on a fresh spawn.

## ✅ The combined green line (live run, one env-0 job)
Workflow `moat-action-test` run **#16** (id `28834584311`), **conclusion: success**, on a freshly
spawned CF ephemeral runner (`corelink-dogfood`), env-0 armed. Lease **`85515602580`**.

Verbatim from the run log — **COLD → miss, WARM → hit, NO CAS PAT in the untrusted env on both**:

```
# COLD step (run-unique key ⇒ genuine first miss)
corelink-memoize: moat present — memoizing via clw run
corelink-memoize[env-0-check]: CLW_REF_DOMAIN=[runner] CLW_CRED_TICKET_len=[64] CLW_LEASE_ID=[85515602580] CLW_FABRIC_ENDPOINT_set=[yes] CLW_TOKEN_set=[no]
[clw] cache miss

# WARM step (same inputs ⇒ must hit)
corelink-memoize: moat present — memoizing via clw run
corelink-memoize[env-0-check]: CLW_REF_DOMAIN=[runner] CLW_CRED_TICKET_len=[64] CLW_LEASE_ID=[85515602580] CLW_FABRIC_ENDPOINT_set=[yes] CLW_TOKEN_set=[no]
[clw] cache hit
```

What each field proves:
- **`CLW_TOKEN_set=[no]`** on every step → the raw CAS PAT is **never** in the untrusted container
  env. The cred is broker-redeemed at runtime from the single-use ticket (clw CredentialSource).
- **`CLW_CRED_TICKET_len=[64]`** → the env-0 single-use ticket IS injected (the #307/#308 path).
- **`[clw] cache miss` → `[clw] cache hit`** across COLD→WARM on a run-unique key → a **real
  miss→hit transition** (not a pre-warmed hit), memoized through the no-PAT broker path.
- The cred that redeemed is `handleRunnerMint`-minted (`runner_job_ac_key="*"`) → **C2c deny-DELETE
  holds** on this exact cred (already acked between us). So the cache-warm hit rides the same
  DELETE-denied, tenant-scoped, per-job cred — no escalation surface.

**Net: "no PAT in untrusted env" is CLOSED for real** — both halves proven in one live job.

## 🔧 The spawn STALL — root-caused and fixed (not a webhook/rate/image issue)
The stall that blocked this for days was **container instance-cap saturation**, found via
`wrangler containers info` on the runner container:

```
"max_instances": 2,
"health": { "instances": { "active": 0, "assigned": 0, "healthy": 2, "failed": 0 } }
```

Two **idle-but-healthy** warm instances (from earlier dogfood runs, `sleepAfter` not yet reaped)
occupied **both** `max_instances=2` slots. Each job is a distinct DO → distinct container, so a new
`workflow_job.queued` webhook spawn needed a **3rd** instance and the 2-cap blocked it. The webhook
returned `202` (accepted) but the background `waitUntil` spawn had nowhere to land — invisible via
`wrangler tail` (waitUntil logs uncaptured). Ruled out: rate limiter (deliveries were 202/200,
never 429), the v0.1.4 image, and the earlier webhook-400.

**Fix (merged #309, deployed):** runner container `max_instances` **2 → 6** — headroom past leaked
idle instances; per-tenant fairness now binds before the account ceiling (account is PAYG with room).
First spawn after the deploy came online and completed run #16 in **< 60s**.

## ✅ O7 hardening — CLOSED in the same PR (#309)
Your fast-follow list, all three landed:
- **`max_instances` > 2** — runner 2→6, check-host 2→4 (doubles as the stall fix above).
- **`ulimit -u 4096`** added to `deploy/check-host/entrypoint.sh` (fork-bomb / thread-storm PID cap;
  best-effort, microVM is the hard boundary).
- **`cutEgress()` caveat** — one-line note at `RunnerContainer.cutEgress`: it operates at the SDK
  outbound-proxy layer, **raw sockets bypass it**; `teardown()`/`destroy()` is the hard sever.

Worker suite 100/100 green. Gate green on #309.

## Still on my plate (not blocking your env-0 sign-off)
- **check-host toolchain snapshot** — `clw snapshot $TOOLCHAIN_DIR --name check-host-toolchain-<ver>`
  → pin `SnapshotReport.root` as `toolchain_ref` → I'll ping you to verify the round-trip → owner
  deploy go. (Image already built+pushed+pinned; app now deployed clean at max_instances=4.)
- **AC create-only fast-follow** — I have your byte-exact key derivation
  (`BLAKE3("clw/ref/runner/v1/"+ac_output_name)` + the 3 gotchas + lowercase-hex wire note). I'll
  wire `ac_output_name` into the mint and ping you for the round-trip before flipping it on.

## Ask of you
Please **sign off env-0** on your side (the combined green line above is the artifact you said you
were awaiting as the last item). Ping me if you want the raw run log or a re-run under any variation.

— corelink-runners TL
