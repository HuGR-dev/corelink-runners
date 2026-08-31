# Cloudflare support package — container application will not start

**Status: DRAFT. Not submitted.** Submitting it is the owner's call — it is outbound
communication to a third party, and it needs a Cloudflare dashboard session this session does not
have. Everything below is evidence gathered read-only from `wrangler` and public HTTP probes.

---

## The one-paragraph version (paste this as the ticket body opener)

A Container-backed Durable Object application on our account stops starting its container. Every
request returns `500 Failed to start container` and the container never binds its port. This is not
image-specific: three different images of ours fail identically, while a **different, known-good
image of ours starts normally in the same application, in the same colo, on the same machine
shape** — so the application itself can execute containers. The application served normally for a
roughly fifteen-minute window on 2026-08-31 and has failed **35 consecutive measured attempts**
since, across four configurations that vary every parameter we control. We are asking what the
platform sees on the container-start path for this application.

## Identifiers

| field | value |
|---|---|
| account id | `6a1fc1c626fc2628823e60b9db01f5cd` |
| container application | `corelink-fabricd-fabricdcontainer` |
| application id | `a0337af9-26db-42c0-a816-0a63ba0e58a3` |
| worker | `corelink-fabricd` |
| public URL | `https://corelink-fabricd.gmhelmold.workers.dev` |
| colo (every instance, always) | `bog04` |
| durable-object instances | `260c9faf…` · `c70e6532…` (`-r2`) · `12e222c7…` (`-enam`) |
| runtime / shape | firecracker · `standard-2` (1 vCPU / 6 GiB / 12 GB) |

## Symptom, exactly as the platform reports it

Edge response, on every route:

```
500  Failed to start container: There has been an internal error connecting to the port
500  Failed to start container: The container is not running, consider calling start()
```

From `wrangler tail`, with lifecycle hooks we added on the `Container` subclass:

```
{"event":"fabricd_container_error","error":"Container crashed while checking for ports,
 did you start the container and setup the entrypoint correctly?", "stack":"...
   at FabricdContainer.waitForPort (index.js:530:32)
   at async FabricdContainer.startAndWaitForPorts (index.js:486:19)"}
{"event":"fabricd_container_stopped","exitCode":0,"reason":"exit"}
```

`onStart` **never fires**. `onActivityExpired` never fires, so nothing of ours stops it.
`wrangler containers info` reports `health: { errors: [], … }` — the platform surfaces no error.

## What we ruled out, and how

Each row is a live experiment, not an inspection.

| ruled out | method |
|---|---|
| our binary | built the same source off-platform and ran it: `/health` 200, stays up, prints every boot line |
| the image | **three** images spanning a month fail identically, including one with a recorded "verified live" history |
| the image recipe | Dockerfile unchanged since 2026-06-25 and used by images that demonstrably worked |
| the application / colo / shape | **the control**: pinned a different, known-good image of ours (`corelink-spawn-worker-checkhostcontainer`, listens on the same port 8080) at this same application — it executed and exited `1`. A *different* failure shape. The application runs containers |
| the instance | several genuine rollouts (`Modified application`, app versions 5 → 8), each recreating the instance |
| DO identity / placement | renamed the DO (new object, new instance) and applied `locationHint: "enam"`; still placed in `bog04` |
| machine shape | `standard-2` → `standard-4`, measured, no change |
| our env / config | measured with the boot-guard override armed, with the optional pre-bind Postgres export disarmed, and with a minimal env — measured, no change |

## The measurement that matters

We stopped drawing conclusions from single probes after an identical configuration produced both a
serving plane and a dead one. Every figure below is a **rate**, from `scripts/ops/fabricd-boot-rate.sh`,
which observes only:

| configuration | SERVED |
|---|---|
| steady config | 0 / 12 |
| \+ boot-guard override armed | 0 / 6 |
| \+ optional pre-bind pg export disarmed | 0 / 6 |
| \+ `standard-4` | 0 / 6 |
| steady config, re-measured later | 0 / 5 |
| **total** | **0 / 35** |

The `standard-4` row **is** the configuration that served three consecutive real responses earlier
(`/health` 200, an authenticated route correctly 401-ing, and our attestation key served). It now
measures 0/6. **Configuration does not explain the difference; time does.**

Raw timestamped observations are retained (TSV, one row per attempt, each classified) and can be
attached.

## The questions we are asking

1. What does the platform record on the container-start path for application
   `a0337af9-26db-42c0-a816-0a63ba0e58a3` since 2026-08-19? `health.errors` is empty at our end.
2. `onStop` reports `exitCode: 0` with `reason: "exit"` while `onStart` never fires. Is that a real
   process exit code, or a placeholder for a container that never started? Our control run produced a
   truthful `1` through the same field, which suggests the field is real — and a process exiting
   *successfully* without binding is not a state our binary has off-platform.
3. Is there a constraint on this application — colo capacity in `bog04`, a per-application quota, an
   environment-size limit on the injected `envVars` (~47 variables, one of them a base64 key) — that
   would prevent a start while a smaller-env image in the same application starts normally?
4. `locationHint` did not move the DO out of `bog04`. Is placement pinned for container-backed DOs,
   and is there a supported way to relocate one?

## What we would accept as a next step

Either a platform-side explanation, or confirmation that deleting and recreating the container
application is safe with respect to the bound Durable Object namespace (`44cecee85676403fba2d65b71fd60a6a`).
We have deliberately not done that, because we cannot tell from the outside whether it orphans the
namespace binding, and the service is already fully unavailable.

## Impact

This is our control plane. It has been unavailable since 2026-08-19 — twelve days. Job execution on
a separate worker is unaffected, so the outward symptom was silent.
