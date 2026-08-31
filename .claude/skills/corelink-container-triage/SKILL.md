---
name: corelink-container-triage
version: 0.1.0
description: Diagnose a DOWN CoreLink Cloudflare-Container service (fabricd, spawn-worker containers, check-host) from live evidence instead of guessing. Covers the read-only evidence ladder (`wrangler containers info` → `containers instances` → `tail` with lifecycle hooks), the container-lifecycle instrumentation that makes a silent boot failure legible, the fabricd pre-bind boot sequence, and the traps that make wrong conclusions cheap — verifying against the DEPLOYED image commit rather than HEAD, and knowing when a forwarded env var never reached the container. Invoke whenever a `/v1` route returns "Failed to start container", a container will not boot, an instance is stuck, a service is unreachable, or an experiment on a container's env "changed nothing".
---

# corelink-container-triage — diagnose a down container from evidence

> Written 2026-08-30 during a live fabricd outage. Every rule here exists because a *plausible*
> conclusion was wrong. The method's value is not speed — it is that each step either produces
> evidence or is explicitly recorded as not-yet-known.

## The one law

**A claim without an artifact you read yourself is theory.** Symptom-matching to a known past
incident is the most expensive mistake available here: the 2026-08-30 outage matched the 2026-07-19
key-drift class perfectly, and that was not the cause.

## Step 0 — before touching anything

- **Use absolute paths.** The Bash tool's cwd persists across calls; a `cd` in one call silently
  breaks relative paths in the next. This wasted real time.
- **Check disk.** A full disk makes tools report partial results as success.
- Record the **rollback target** first: `wrangler deployments list --name <worker>` → the current
  version id. Do this *before* any deploy.

## Step 1 — the read-only evidence ladder (cheapest first, never skip ahead)

```bash
# 1. What does the edge actually return? Body text matters, not just the status.
curl -s -o /tmp/b -w "%{http_code}" -m 30 "https://<worker>.workers.dev/health"; cat /tmp/b

# 2. Application-level state, the pinned image, and the resource shape.
wrangler containers list                    # find the application ID
wrangler containers info <APP_ID>           # image digest, vcpu/memory, health{}, updated_at

# 3. THE DECISIVE READ, and the one that is easy to forget:
wrangler containers instances <APP_ID>      # per-instance STATE + LOCATION + CREATED
```

`containers instances` is what separates *"the process keeps crashing"* from *"one instance is stuck
`stopped` in one colo and is never replaced"*. Those have completely different fixes. Reach it early.

**Read `updated_at` on the application.** If it predates your deploys, **no container rollout
happened** — see the trap in Step 4.

## Step 2 — make the failure legible before theorising

A container that fails to boot produces one opaque edge message and nothing else. Add lifecycle
instrumentation to the Container DO subclass first; it is additive and behaviour-preserving
(the SDK's default `onError` logs and rethrows, `onStop` is a no-op — always call `super`):

```ts
override onStart(): void | Promise<void> {                       // did it EVER come up?
  console.error(JSON.stringify({ event: "<svc>_container_started" }));
  return super.onStart();
}
override async onActivityExpired(): Promise<void> {              // did WE stop it?
  console.error(JSON.stringify({ event: "<svc>_container_activity_expired" }));
  return super.onActivityExpired();
}
override onStop(params: StopParams): void | Promise<void> {      // how did it end?
  console.error(JSON.stringify({ event: "<svc>_container_stopped", exitCode: params.exitCode, reason: params.reason }));
  return super.onStop(params);
}
override onError(error: unknown): unknown {
  console.error(JSON.stringify({ event: "<svc>_container_error", error: String(error) }));
  return super.onError(error);
}
```

Read it while driving traffic (tail alone shows nothing without requests):

```bash
( sleep 8; curl -s -o /dev/null -m 40 "https://<worker>.workers.dev/health" ) &
timeout 50 wrangler tail <worker> --format pretty
```

**Interpreting the sequence:**

| observation | conclusion |
|---|---|
| no `_started` | never came up — the port never opened |
| no `_activity_expired` | *we* did not stop it (the SDK's only graceful-stop path is `onActivityExpired → stop()`) |
| `_stopped` with `reason: "runtime_signal"` | killed (SIGKILL — e.g. a watchdog `destroy()`, OOM) |
| `_stopped` with `exitCode` **and** a preceding `_started` | a real process exit — the code is meaningful |
| `_stopped` with **no** preceding `_started` | **the exit code is not evidence.** It may be synthetic |

**Hard limit, verified in the SDK, do not promise otherwise:** `@cloudflare/containers@0.3.7`
exposes **no container stdout/stderr**. `monitor` is `private`; no type carries process output
(`StopParams` is exactly `{ exitCode, reason }`). The container's own `eprintln!` diagnostics are
**unreachable** through any hook. Surfacing them needs a different mechanism (e.g. the container
POSTing its own boot status before it aborts).

## Step 3 — fabricd's pre-bind boot sequence (nine fallible steps)

`crates/corelink-fabric-server/src/main.rs` — **any** of these aborts boot before the port opens, and
all produce the identical edge error. Never name one without excluding the others:

| # | step | armed by |
|---|---|---|
| 1 | `config_from_env(…)?` | always |
| 2 | `boot_introspect_selfcheck(&cfg)?` — FATAL on a `<500` non-2xx from introspect | always (corelink auth backend) |
| 3 | `build_app_and_state(&cfg)?` | always |
| 4 | `reaper_config_from_env(…)?` | always |
| 5 | `maybe_spawn_crash_sweep_from_env(…)?` | `FABRIC_CRASH_PROBE_INTERVAL_SECS` |
| 6 | `maybe_spawn_billing_exporter(…).await?` — **connects to Postgres, applies DDL** | `FABRIC_BILLING_EXPORT_INTERVAL_SECS` |
| 7 | `quota_check_config_from_env(…)?` | `FABRIC_QUOTA_CHECK_INTERVAL_SECS` |
| 8 | `pending_max_age_from_env(…)?` | always |
| 9 | `TcpListener::bind(…)` | always |

A failing step returns `Err` from `main` ⇒ process exit **1**. Absence of an env var is the documented
disable path for the optional steps, which makes bisection cheap — *if* Step 4's trap is respected.

## Step 4 — the traps (each one produced a wrong answer)

**T1 — verify against the DEPLOYED commit, never HEAD.** The running binary is an image digest built
from some commit. Check the behaviour there:

```bash
git show <build-sha>:crates/corelink-fabric-server/src/server.rs | grep -n "<the guard>"
git cat-file -e <build-sha>:path/to/file && echo PRESENT || echo ABSENT
```

Two hypotheses died here: *"the image predates the CF Access code"* (it was added **in** that exact
build commit) and *"the boot probe omits the Access headers"* (it attaches them).

**T2 — `no changes to be made` means your env never reached the container.** A Worker deploy that
prints this for the container application did **not** roll the container. The singleton reads env at
**boot**, so a newly-forwarded variable only applies to a *newly started* container. If no container
ever reached `started`, **an env-flip experiment proves nothing** — do not record it as an exclusion.
Forcing a real rollout means changing the container application config (in practice: a new image).

**T3 — a truncated read is a wrong read.** Extracting an env-forwarding list with a fixed line window
silently produced a short list and a false "not forwarded" conclusion. Parse to the closing brace:

```bash
python3 - <<'PY'
import re
s=open('deploy/cloudflare-fabricd/src/index.ts').read()
i=s.index('this.envVars = {'); j=s.index('{',i); d=0
for k in range(j,len(s)):
    if s[k]=='{': d+=1
    elif s[k]=='}':
        d-=1
        if d==0: break
print(sorted(set(re.findall(r'\b([A-Z][A-Z0-9_]{3,})\s*:', s[j:k+1]))))
PY
```

**T4 — `vars` are declarative; a deploy replaces them.** Diff live against declared *before*
deploying, or you silently delete a production value (this has happened here before, mis-attributing
billing):

```bash
wrangler versions view <VERSION_ID> --name <worker> | grep -oE "env\.[A-Z_]+ \(" | sed 's/env\.//;s/ (//' | sort
```

Secrets are separate and survive a deploy; `wrangler secret list --name <worker>` shows names only.

## Step 5 — known structural defects of the diagnosis surface itself

These are why a fabricd outage is hard. Check whether they still hold before assuming a tool works:

- **The override in the FATAL message may be unreachable.** `boot_introspect_selfcheck` tells the
  operator to set `FABRIC_INTROSPECT_BOOTCHECK=warn`; that var lived only in a wrangler *comment* and
  was never forwarded, so a rejected key was an **unrecoverable** outage (`union-31`).
- **Container boot logs reach nobody** (`union-32`, above).
- **An optional subsystem can prevent `bind`.** Billing export is default-off yet armed in prod and
  awaited with `?` before the listener opens, so a database blip takes down the whole control plane
  (`union-33`).
- **The self-heal watchdog is disabled by a TOTAL outage** (`union-34`). `scheduled()` skips the
  health probe when `Date.now() - lastActivityMs > IDLE_MS` (4 min) **and deletes the watchdog
  lifecycle state**; `watchdogAction` then refuses to destroy a never-healthy container until
  `BOOT_GRACE_MS` (3 min) has elapsed since `firstSeenAt`. Destroying therefore needs three
  *continuous* minutes of traffic spaced under four minutes apart. In a total outage callers give up,
  activity lapses, the clock resets — and a stuck instance survives indefinitely. To force the
  watchdog by hand, drive traffic every ~25 s for >3 min and watch for `keep-warm[…] destroying`.

**The pattern to expect in this codebase:** each guard is individually defensible; composed, they
produce a system that fails closed, tells nobody why, cannot be overridden, and cannot heal itself.
When a fix here makes something "safer", ask what it does to *recoverability* and *observability*.

## Step 6 — writing it down

Record refuted hypotheses with their evidence, not just the answer, so the next person does not
re-walk them. If you retract a conclusion, retract it **loudly and in place** — a diagnosis document
that quietly drops a wrong claim is worse than one that never made it.
