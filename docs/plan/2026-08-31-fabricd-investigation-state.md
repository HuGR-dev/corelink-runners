# fabricd outage — ROOT-CAUSED AND SERVICE RESTORED, 2026-08-31

> **ROOT CAUSE FOUND.** The Neon Postgres project behind `DATABASE_URL` has been
> refusing every connection since ~2026-08-19 with a plan-quota error. Captured from
> the container's own stderr:
>
> ```
> [boot] introspect key VALIDATED (HTTP 200) at .../internal/v1/auth/introspect — auth path ready
> Error: PgLedger: cannot acquire connection for DDL: Error occurred while creating a new object:
>        db error — cause: Backend(Error { kind: Db, cause: Some(DbError {
>        severity: "ERROR", code: SqlState(…),
>        message: "Your account or project has excee[ded …]"
> ```
>
> `PgLedger::connect` runs inside `build_app_and_state` (server.rs:1274), **before**
> `TcpListener::bind`, and its error is propagated with `?`. So a quota-suspended
> database does not degrade durability — it aborts the whole control plane before it
> can serve anything. Nothing about the image, the entrypoint, the colo, the instance
> cap, the registry path or Cloudflare was ever involved; each was excluded by a
> measured rate, and the list is kept below because those exclusions are what made
> the remaining surface small enough to instrument.
>
> **The owner's correction was right:** it was never a support issue.
>
> **How it was finally read:** every log surface reachable from this session was
> blind — `onStart` never fires, `health.errors` is empty, the `exitCode: 0` the
> Worker reports is @cloudflare/containers' hardcoded placeholder for "stopped with
> no exit code" (`dist/lib/container.js:1597`), and container stdout goes to Workers
> Logs, which needs a scope this session lacks (`wrangler tail` does NOT carry it —
> verified with a control image that executed to a real exit 1 and still emitted
> nothing). So the image was made to report on itself, over the network, to the one
> readable surface: the Worker's own request log, payload in the URL path.
> See `crates/corelink-fabric-server/Dockerfile.bootprobe`.
>
> **SERVICE IS BACK (2026-08-31 15:35Z).** `FABRIC_PG_DISABLED=1` makes the proxy
> behave as if DATABASE_URL were unset, so fabricd boots on the in-memory ledger and
> stops dialling the suspended database. Measured on the fixed config: **10/10
> SERVED**, against 0/35 before. `/health` 200 · `/v1/attestation/key` 200 serving
> the production key · `/v1/usage` 401 fail-closed.
>
> This is DEGRADED and meant to be undone — see §8.

---

# Original investigation state (kept for the record)

**Read this instead of the session transcript.** Written 2026-08-31 by the session that got it
wrong, to hand a fresh context the facts without the dead ends.

> **Standing correction from the owner:** *"it is not a support issue — you are doing something
> wrong."* Treat the platform-fault conclusion as UNPROVEN and probably a symptom of my own
> incomplete checking. The support package (`2026-08-31-cloudflare-support-package.md`) is a draft
> that should NOT be sent until the unexamined leads in §4 are closed.

---

## 1. The symptom, in one line

The `corelink-fabricd` container never binds its port. Every route returns
`500 Failed to start container`. `onStart` never fires; `onStop` reports `exitCode 0, reason "exit"`.
`wrangler containers info` shows `health.errors: []` — the platform surfaces nothing.

Job execution is unaffected (a different Worker); only the control plane is down. Since 2026-08-19.

## 2. Facts established by measurement, not inference

- **0 / 35 SERVED** across five runs and four configurations
  (`scripts/ops/fabricd-boot-rate.sh`, raw TSVs in `docs/plan/evidence/`).
- **It served once.** For roughly fifteen minutes it returned `/health` 200, `/v1/usage` 401
  fail-closed, and `/v1/attestation/key` 200 with the production key id `faa5b7726ccd2c52`. That
  window is real and is the single most important unexplained fact.
- The configuration that served now measures **0/6**. Config does not explain the difference.
- **Control:** the `checkhostcontainer` image, pinned at this SAME application, colo and shape,
  **executed and exited 1**. A different failure shape — so the application can run *a* container.
- **The binary is fine off-platform:** built from this tree and run locally it serves `/health` 200
  and stays up.

## 3. Ruled out, each by a live experiment (do not re-walk these)

binary · image (3 versions across a month, incl. one with a verified-live record) · Dockerfile
(unchanged since 2026-06-25, used by working images) · instance (several real rollouts, app versions
5→8) · DO identity (renamed twice) · placement (`locationHint` does not move it; still `bog04`) ·
machine shape (`standard-2` ↔ `standard-4`, measured) · the introspect boot guard (`warn`, measured)
· the pre-bind Postgres export (disarmed, measured) · env size via a minimal env (booted at the time,
but see §5 — that reading is suspect).

## 4. NOT examined — start here

These are the gaps that make the platform conclusion premature. Roughly in order of value:

1. **Architecture of the built image.** Never checked. The image is built by
   `.github/workflows/build-fabricd-image.yml` with a bare `docker build` (no `--platform`) on a
   `runs-on: corelink` box, whose architecture I never established. An arch mismatch would produce
   exactly this: exec fails, no port, no useful error. The control image was built by a *different*
   workflow (`build-cf-container-images.yml`) — **verify they build on the same arch before treating
   the control as arch-equivalent.**
2. **Inspect the pushed image itself.** Never done. Manifest, config, entrypoint, layer count, total
   size. Needs registry auth, which this session did not have (`docker buildx imagetools inspect`
   timed out unauthenticated). `wrangler containers push` authenticates docker as a side effect —
   that is a way in.
3. **Image size / pull time.** The fabricd image is a full Rust release build; the control image is
   different. A pull that exceeds a start deadline looks like "crashed while checking for ports".
   Compare sizes.
4. **Account-level instance quota.** `wrangler containers instances` on the runner app lists **716
   `inactive`** instance records plus a handful running. Nobody checked whether inactive records
   count against a cap, or whether the account is near one.
5. **Why the control exited 1.** Never read. If it exited 1 for a *config* reason, execution is
   proven. If it exited 1 for another reason, the control is weaker than claimed.
6. **Push a trivial image to THIS application.** The cleanest possible control (e.g. a hello-world
   that binds 8080). Not attempted.
7. **`defaultPort` vs the platform's expectation** for this app version. Assumed correct, never
   verified against a working app's configuration.

## 5. Methodological warnings — the traps this session actually fell into

- **Single observations on an intermittent fault.** Three consecutive successes were committed as a
  root cause; five subsequent failures on the same config refuted it. **Nothing here is evidence
  unless it is a rate.** The "minimal env booted" reading in §3 is exactly this shape and should be
  re-measured, not trusted.
- **Changing more than one variable per deploy.** Done twice, and both times it destroyed the
  ability to attribute the result.
- **Concluding about the other side while only varying your own.** Seven experiments varied our
  config and held the platform constant, then read the constant failure as evidence *about* the
  platform. The control reversed it. Run the control first.
- **Reading a claim in a comment as a fact.** Several wrangler comments assert "verified LIVE" for
  images; at least one of those images never served.
- **`wrangler versions view` is the arbiter** of whether two configs are actually identical. Use it
  before saying "same config".

## 6. Live state right now

Prod is at the intended steady config — no diagnostic knobs armed. Image pinned by **git-SHA tag**,
not `@sha256`: run `33347304356` emitted
`::warning::Could not resolve an @sha256 digest (push output + imagetools both empty)`, which is a
**regression of #400** and an open INV-2 deviation. Three DO instances exist
(`fabricd-singleton`, `-r2`, `-enam`); the live one is `-enam`. Two temporary code changes remain,
both harmless and documented in place: lifecycle logging on the Container subclass, and a
`locationHint` wrapper that measurably does nothing.

## 7. The one question worth keeping in front

**It served for fifteen minutes.** Whatever the cause is, it is something that was briefly
satisfied and then stopped being satisfied — not a static misconfiguration, because a static
misconfiguration cannot serve. Any hypothesis that cannot explain that window is wrong, including
every hypothesis this session produced.

---

## 8. Remediation (2026-08-31)

**The blocker is a database plan quota, not code.** Two ways forward:

**A — restore the database (recommended).** Raise the Neon plan or wait out the quota
period, then confirm with the boot probe. fabricd needs no change: the moment Neon
accepts connections, `PgLedger::connect` succeeds and the control plane binds. This
keeps the durable ledger, so lease state and the billing records derived from it stay
intact. It costs money and only the owner can authorise it.

**B — drop `DATABASE_URL` (emergency only).** Absence of the secret selects the
in-memory ledger and fabricd boots immediately. It trades away durability: lease state
does not survive a restart, N>1 stays refused, and the durable billing export is inert.
The pg data is unreachable either way while the quota holds, so this loses no data that
is currently readable — but it must be reverted the moment the database is back, and
anything accrued in memory meanwhile is gone.

### Owed regardless of which is chosen

1. **An optional background task must not be a hard pre-bind dependency.** The durable
   billing exporter (`maybe_spawn_billing_exporter`) also runs before `bind` and also
   propagates with `?`. It was not the cause here, but it is the same trap armed and
   waiting: a database blip would take the control plane down through it too. It should
   retry in the background and surface its state, not abort boot.
2. **A dead control plane must be loud.** This ran twelve days. The keep-warm cron saw
   every failure and logged "cold boot in progress, NOT destroying" each minute without
   ever escalating.
3. **Container log streaming stays on.** `observability: { enabled: true }` was missing
   from this Worker and is now set; it is why the outage was investigated blind.

---

## 9. Restored 2026-08-31, and why this way

The crash loop was not just a symptom, it was a consumer. The keep-warm cron retries
every minute and each retry dials the database; a scale-to-zero database woken every
60 s never autosuspends, so fabricd was burning the compute allowance whose exhaustion
was refusing it. Waiting for the quota period to roll would therefore have bought a
working control plane until roughly the same day of the next month.

`FABRIC_PG_DISABLED=1` reuses a state the code already models coherently: the pg
backend, the vCPU ceiling the #265 guard ties to it, and the pg-only billing export
all arm together inside one block in the proxy, so they suppress together. Nothing
half-arms. The DATABASE_URL secret is left bound — it is a credential this session
cannot restore, and a var is reversible from the config by anyone.

The database is still the real fix, and option A in §8 stands. A free project on an
equivalent provider is enough to take it: `docs/handoff/2026-07-03-PLAN-postgres-
provisioning-and-vcpu-ceiling-arm.md` already sanctions Supabase alongside Neon, with
the public-CA TLS `FABRIC_PG_TLS=require` verifies and the CREATE TYPE/TABLE grant the
DDL needs. Use a direct or session-mode connection — tokio-postgres uses prepared
statements and deadpool holds connections across calls, which transaction-mode pooling
breaks.
