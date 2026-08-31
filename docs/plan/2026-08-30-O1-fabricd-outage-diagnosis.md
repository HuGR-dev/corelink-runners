# O1 — fabricd outage: diagnosis from live evidence

**Date:** 2026-08-30 · **Baseline:** `8631abb` · **Access:** Cloudflare OAuth, account
`6a1fc1c626fc2628823e60b9db01f5cd`, scopes `workers (write) · workers_tail (read) · workers_kv ·
d1`. Every line below is evidence I read myself; where I could not close a question I say so instead
of naming a cause.

---

## 1. What is actually true right now

| observation | evidence |
|---|---|
| every public `/v1` route and `/health` return `500 Failed to start container` | direct probes, 2026-08-30 |
| the container application is **not** missing or unhealthy at the platform level: `state: active`, `instances: 1`, `health: { errors: [], healthy: 1, active: 0, failed: 0 }` | `wrangler containers info a0337af9-…` |
| the container **crashes before binding its port** | `wrangler tail corelink-fabricd`: `Container error: Error: Container crashed while checking for ports, did you start the container and setup the entrypoint correctly?` |
| no fabricd deploy since 2026-08-19 | `wrangler versions list --name corelink-fabricd` |
| the pinned image `db3b03ef` was built from `e5f07f8` — **the Inc-3 commit itself** | `git log -1 e5f07f8` |
| the job fleet is unaffected: 25 runner instances live; `corelink-smoke` green 23:14 | `wrangler containers list`; `gh run list` |

**So this is a boot crash, not a hang, not a missing deploy, and not a bad image reference.**

## 2. Hypotheses tested and REFUTED (recorded so nobody re-walks them)

1. **"The boot probe doesn't send the CF Access service-token headers."** Refuted —
   `UreqIntrospect::post` attaches them (`crates/corelink-fabric-server/src/corelink_auth.rs:171`).
2. **"The deployed image predates the CF Access code."** Refuted — `cf_access.rs` was **added in**
   `e5f07f8`, which is the exact build commit of the pinned digest.
3. **"The Access secrets aren't bound."** Refuted — `CORELINK_CF_ACCESS_CLIENT_ID` and
   `..._SECRET` are both bound (`wrangler secret list --name corelink-fabricd`) **and** forwarded
   into the container (`deploy/cloudflare-fabricd/src/index.ts:154-158`).
4. **"`FABRIC_PUBLIC_BASE_URL` isn't forwarded (the #332 guard)."** Refuted — it is forwarded
   (`index.ts:214`). *(My first extraction of the env list truncated at line 200 and produced a
   wrong answer; the block actually runs `137..282` and forwards 47 vars. Corrected before use.)*

## 3. Where the crash can be — the complete pre-bind boot sequence

`crates/corelink-fabric-server/src/main.rs` has **nine fallible steps before**
`TcpListener::bind` (line 84). Any one of them aborts boot and produces exactly the platform error
observed. Naming a single cause without narrowing these would be a guess:

| # | step | armed in prod? |
|---|---|---|
| 1 | `config_from_env(…)?` | always |
| 2 | `boot_introspect_selfcheck(&cfg)?` — FATAL on a `<500` non-2xx from introspect | always (CoreLink auth backend) |
| 3 | `build_app_and_state(&cfg)?` | always |
| 4 | `reaper_config_from_env(…)?` | always |
| 5 | `maybe_spawn_crash_sweep_from_env(…)?` | only with `FABRIC_CRASH_PROBE_INTERVAL_SECS` |
| 6 | **`maybe_spawn_billing_exporter(&state,&cfg).await?`** — connects to Postgres and applies the sink DDL | **YES — `FABRIC_BILLING_EXPORT_INTERVAL_SECS: "60"` (`wrangler.jsonc:90`)** |
| 7 | `quota_check_config_from_env(…)?` | only with `FABRIC_QUOTA_CHECK_INTERVAL_SECS` |
| 8 | `pending_max_age_from_env(…)?` | always |
| 9 | `TcpListener::bind(…)` | always |

**Two co-equal prime suspects:**

- **Step 2** — the introspect key is rejected (`FABRIC_INTROSPECT_AUTH_KEY` drift, or the CF Access
  service token is no longer accepted by corelink-server's Access policy). This is the 2026-07-19
  key-drift class the guard was built for.
- **Step 6** — Postgres is unreachable or the DDL fails. **This is armed in production and runs
  before the port binds**, so any managed-Postgres blip takes the entire control plane down. That
  coupling is a defect in its own right (§4.3).

I cannot distinguish them from outside, for the reason in §4.2.

## 4. Three structural defects this outage exposed — none of them in the 247

### 4.1 The documented escape hatch cannot reach the container — `union-31` (HIGH)

The FATAL message tells the operator: *"or set `FABRIC_INTROSPECT_BOOTCHECK=warn` to override."*
`FABRIC_INTROSPECT_BOOTCHECK` appears **only inside a comment** at
`deploy/cloudflare-fabricd/wrangler.jsonc:196`. It is not in `vars` and not in the 47-var envVars
block. **Setting it does nothing.** A fail-closed guard whose only override is unreachable converts a
recoverable incident into an unrecoverable one.

*Acceptance item:* **test:** every override named in a FATAL/guard message is present in the
container's forwarded env set (a lint over guard strings vs `envVars`).

### 4.2 The container's own boot log is unobservable — `union-32` (HIGH)

Each of the nine steps prints a distinct, diagnostic `eprintln!`. Not one is reachable: the fabricd
Worker forwards no container stdout/stderr anywhere (`grep -n "onError|monitor()|logs|stdout|stderr|
console.error" deploy/cloudflare-fabricd/src/index.ts` → **no matches**). `wrangler tail` shows only
the Worker side.

The hardening (#408) correctly turned a *silent* outage into a *loud* one — but the shout has no
channel. Combined with 4.1, the system is now: fails closed · says why to nobody · cannot be
overridden. That is strictly worse than the silent failure it replaced, and it is why this outage is
being diagnosed by elimination instead of by reading one line.

**CORRECTION (made while implementing the fix — the original wording promised more than the platform
can deliver).** `@cloudflare/containers@0.3.7` exposes **no container stdout/stderr at all**: `monitor`
is `private` (`dist/lib/container.d.ts:289`) and no type in the SDK carries process output
(`dist/types/index.d.ts` — `StopParams` is exactly `{ exitCode: number; reason: 'exit' |
'runtime_signal' }`). So the `[boot]` lines **cannot** be surfaced by overriding a hook. What *is*
reachable is the exit signal and any runtime error. Full boot-log observability needs a different
mechanism — most likely the container POSTing its own boot status before the guard aborts — and
remains **open**, not fixed.

*Acceptance item (corrected):* **probe:** a forced container boot failure emits a structured
`fabricd_container_stopped` record carrying `exitCode` and `reason` in `wrangler tail`.
*Separate, still open:* the container's own `[boot]` diagnostic reaches an operator.

### 4.3 Billing export is a hard pre-bind boot dependency — `union-33` (MEDIUM→HIGH)

Step 6 is an **optional, default-off** feature (`maybe_spawn_billing_exporter`) that is **armed in
prod** and whose async Postgres connect + DDL is awaited with `?` **before** the listener binds. So a
transient database problem does not degrade billing — it takes down acquire, usage, attestation and
credential redemption. An optional subsystem must never be able to prevent the control plane from
serving.

*Acceptance item:* **test:** no optional subsystem's initialisation can prevent `bind`; a failure in
one degrades that subsystem and is alarmed, never the server.

## 5. Recovery — the minimum that closes this, in order

1. **One `wrangler deploy` of the fabricd *Worker* only** (no image rebuild, no container change),
   carrying 4.1 and 4.2: forward `FABRIC_INTROSPECT_BOOTCHECK`, and surface container stderr.
   Rollback is `wrangler rollback` to the current version id. This single change makes the cause
   **self-revealing** and gives an override — after it, the next boot names its own failure.
2. Read the revealed cause; fix the credential (step 2) or the database (step 6).
3. Only then: rebuild + repin the image to ≥ #515 (`fabricd-deploy-01`, `hist-04`) — a separate,
   larger change that must not be entangled with restoring service.

**Ordering rationale:** the temptation is to re-set the introspect secret and roll, hoping it is step
2. If the cause is step 6, that spends a deploy and teaches nothing. Making the system say what is
wrong costs the same one deploy and cannot be wrong.

## 6. What I did not do

I did not deploy, did not change a secret, did not restart anything. Everything above is read-only.
Step 5.1 is a production mutation and needs the owner's explicit go-ahead on that specific change.

---

# ADDENDUM — live instrumentation, and a RETRACTION (2026-08-30, later)

The Worker fix from §5.1 was deployed (versions `a20af070` → `ff1987ae` → `f5e337dc`). What it taught
changed the diagnosis, including invalidating two of my own conclusions.

## A. RETRACTION — two exclusions I claimed are NOT safe

I reported that `FABRIC_INTROSPECT_BOOTCHECK=warn` changed nothing and therefore **excluded** the
introspect boot self-check, and later that removing `FABRIC_BILLING_EXPORT_INTERVAL_SECS` changed
nothing and therefore **excluded** the Postgres connect. **Both exclusions are withdrawn.**

`wrangler containers info` reports the container application's `updated_at` as **2026-08-19**, and
all three Worker deploys printed `no changes to be made` for the container. This repo documents the
consequence itself (`wrangler.jsonc:203-204`): a binary is *"rebuilt only to force a container
rollout so the proxy's newly-forwarded envVars take effect (the singleton reads env at boot)"*.
Since no container ever reached `started`, there is no evidence either variable was ever read by a
running process. An experiment whose treatment may never have been applied proves nothing.

## B. What the instrumentation DID establish

- `fabricd_container_started` **never fires** — the container never reaches started; the port never
  opens.
- `fabricd_container_activity_expired` **never fires** — nothing in this Worker stopped it; the SDK's
  only graceful-stop path (`container.js:748 → this.stop()`) did not run.
- The watchdog is excluded independently: it uses `destroy()` = SIGKILL.
- `onStop` reports `exitCode: 0, reason: "exit"`. **Not load-bearing:** with the container never
  started, this may be a synthetic stop rather than a real process exit code. Reading it as
  "`main()` returned `Ok`" would over-read an SDK field whose meaning in this state is unverified.

## C. The decisive observation — one instance, stuck

```
wrangler containers instances a0337af9-…
INSTANCE  260c9faf…  NAME fabricd-singleton  STATE stopped  LOCATION bog04  VERSION 5
CREATED   2026-08-19T22:58:00Z
```

A single instance, **`stopped`**, pinned to one colo, unchanged since 2026-08-19. Every request tries
to start it and fails the port check. So the question is not only *"why did the process fail"* but
*"why was this instance never replaced"* — and that second question has a complete answer.

## D. `union-34` (HIGH) — the self-heal watchdog is disabled by a TOTAL outage

`scheduled()` runs each minute and, before probing, consults an idle gate
(`deploy/cloudflare-fabricd/src/index.ts:955-967`):

```js
idleSkip = Date.now() - lastActivityMs > IDLE_MS;   // IDLE_MS = 4 min
if (idleSkip) { watchdogState.delete(id); continue; }  // no probe at all
```

and `watchdogAction` (`:776-798`) refuses to destroy a container that has never been healthy until
`now - firstSeenAt >= BOOT_GRACE_MS` (**3 min**).

Composed: a lapse in traffic longer than 4 minutes **deletes the lifecycle state**, so when traffic
resumes the boot-grace clock restarts from zero and the verdict is `skip-booting` again. Destroying
therefore requires **three continuous minutes of traffic spaced under four minutes apart.**

In a *partial* outage that holds — real traffic continues, the watchdog fires, the box is replaced.
In a **total** outage it cannot: every request 500s, callers stop, activity lapses, the clock resets,
and the container is never replaced. **The self-heal is structurally unavailable in exactly the
failure it exists for**, which is why a stopped instance has survived eleven days.

This is the same shape as the other findings in this session: each guard is individually reasonable,
and composed they produce a system that fails closed, tells nobody, cannot be overridden, and cannot
heal itself.

*Acceptance items:* **test:** a container that has never been healthy is destroyed after a bounded
wall-clock period **regardless of traffic**; the idle gate must not delete watchdog lifecycle state.
**probe:** a synthetically stopped singleton is replaced without operator action.

## E. Revised recovery

The lever is a **container rollout**, which by this repo's own practice means a new image — and the
rebuild to ≥ #515 is already owed (`fabricd-deploy-01`, `hist-04`). That single action replaces the
stuck instance *and* lands the merged fixes. Only after a container has actually started can §3's
nine-step table be bisected, because only then is a forwarded variable observably applied.

Two temporary settings must be reverted once service is restored: `FABRIC_INTROSPECT_BOOTCHECK=warn`
and the disarmed `FABRIC_BILLING_EXPORT_INTERVAL_SECS` (original value `"60"`).
