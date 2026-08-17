# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 2026-08-17 — docker-shim: `docker buildx build` compatibility

- **fix(runner): the `docker` shim now translates `docker buildx build …` →
  `nerdctl build …` (and no-ops `buildx imagetools inspect`).** The shim
  `exec`s `nerdctl "$@"`, but nerdctl has no `buildx` subcommand, so an
  unmodified `docker buildx build` died with `unknown shorthand flag: 't' in
  -t` (verified on a live `runs-on: corelink` lease). This blocked any tooling
  that shells to buildx — notably `wrangler containers build`, which is why
  `build-cf-container-images.yml` still has to run on hosted `ubuntu-latest`.
  Plain `docker build` already worked here (proven F2 + reprobed 2026-08-17), so
  the shim just drops the `buildx` word for `build` and best-effort-no-ops the
  `imagetools inspect` metadata query (callers `|| true` and fall back to the
  tag). `docker login` / `docker push` already passed through, so the full
  build→login→push chain to `registry.cloudflare.com` now works daemonlessly on
  a lease. Takes effect on the next runner-image rebuild; unblocks migrating the
  image-build lane off hosted (zero-hosted mandate). Translation logic unit-tested
  for all forms (build / imagetools / version / login / push).

### 2026-08-17 — image-build lane: hosted → self-hosted daemonless (unblocks rebuilds)

- **ci: `build-cf-container-images.yml` now runs on `runs-on: corelink`,
  daemonless.** It ran on GitHub-hosted `ubuntu-latest` because `wrangler
  containers build` shells to `docker buildx`, which the shim didn't support —
  so under the zero-hosted billing block the lane was **dead** (`steps:0` at "Set
  up job") and the runner image could NOT be rebuilt at all. Replaced with the
  proven daemonless chain: plain `docker build` (shim → nerdctl/buildkit) +
  `wrangler containers push <tag>` (wrangler mints the CF registry cred and pushes
  via the docker binary — no buildx, no daemon, no hosted spend). New
  `scripts/ci/resolve-pushed-ref.sh` parses the immutable manifest digest from the
  push transcript for an X4 pin (falls back to the tag). **Proven end-to-end on a
  live `corelink` lease (2026-08-17):** both images built amd64-native and pushed —
  RunnerContainer `@sha256:27973ad0…` (carries the baked action-archive cache +
  buildx-compat shim) and CheckHostContainer `@sha256:3703b5c0…`. Pairs with the
  docker-shim `buildx build`→`build` fix. Deploying the new image to the fleet is a
  separate, deliberate repin of `deploy/cloudflare/wrangler.jsonc` (green-spawn
  verified) — not done here.

### 2026-08-17 — bake the action-archive cache into the runner image (kills codeload-429)

- **ci(runner): pre-seed `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` in the ephemeral
  runner image so a spawned box never cold-downloads `actions/checkout`
  (+friends) from codeload.github.com.** Every runner egresses through one IP; a
  broad corelink-server PR fans ~20 gates out at once, each cold-downloading the
  action archive, and concurrent downloads on one IP trip codeload's per-IP rate
  limit (`429 Too Many Requests`), failing jobs at "Set up job" with no test run.
  New `deploy/runner/Dockerfile` stage `action-cache-download` fetches each pin in
  `deploy/runner/action-archive-pins.txt` (the hot corelink-server actions) to
  `/opt/action-archive-cache/<owner>_<repo>/<sha>.tar.gz`; the final stage COPYs it
  and sets `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE` (+ `_SYMLINK_CACHED_ACTIONS`), so
  the runner (≥2.335) extracts locally and never hits codeload. Integrity anchor is
  the git commit SHA (git-archive tarballs are not byte-reproducible, so no tarball
  sha256 pin); `curl -f` + `gzip -t` fail the build closed on a bad pin. This is the
  image-baked half of the fix — the persistent Mac builders set the same env + a
  seed script (corelink-server `scripts/seed-runner-action-cache.sh`), proven by use
  2026-08-17 (job logs "Found action archive … in cache directory", 0 codeload).

### 2026-08-16 — release binary was a Linux ELF mislabeled as Apple-Silicon

- **fix(release): `.github/workflows/release.yml`'s `binary` job ran on
  GitHub-hosted `ubuntu-latest`, built `corelink-cli` with no `--target`
  (native x86_64 Linux ELF), and uploaded it as the release asset
  `corelink-aarch64-apple-darwin`.** On the next `v*` tag this would have
  shipped a Linux binary labeled Apple-Silicon — any macOS-ARM customer
  running it would hit "Exec format error". Rewritten to build a matrix of
  targets this repo's self-hosted `corelink` fleet can actually produce
  correctly — `x86_64-unknown-linux-gnu` (native) plus
  `aarch64-unknown-linux-gnu` / `x86_64-pc-windows-gnu` (via
  `cargo-zigbuild`, mirroring the proven pattern in `corelink-server`'s
  `release-cli.yml` and `corelink-workspaces`' `release.yml`) — with each
  asset truthfully named after its real target triple. `aarch64-apple-darwin`
  is intentionally NOT built: this repo has no self-hosted Mac builder (the
  sibling repos build darwin natively on real Apple hardware; cross-building
  to an Apple target needs the non-redistributable Apple SDK). `docs/release.md`
  updated to match. Follow-up: add a darwin leg once a self-hosted Mac
  builder exists for this repo.
- **ci(build-cf-container-images): fix a self-contradictory comment.** The
  job comment claimed "Self-hosted Linux x64 builder (label
  `corelink-builder`). NOT GH-hosted" while `runs-on: ubuntu-latest` sat one
  line below it. Corrected to state the truth: it deliberately stays
  GitHub-hosted because it needs a Docker daemon (`wrangler containers
  build`) that the self-hosted fleet doesn't run, kept cheap via its
  existing `workflow_dispatch`-only trigger.

### 2026-08-13 — BuildKit baked in the fleet image (daemonless container builds)

- **feat(runner-image): bake BuildKit (`buildkitd`+`buildctl`+runc) into
  `deploy/runner/Dockerfile`, X4-pinned (v0.32.2, sha256 from the real
  92,882,168-byte release tarball).** CoreLink runners can now build container
  images WITHOUT a docker daemon: `buildkitd` runs per-job inside the lease
  (rootful via the runner's passwordless sudo) and `buildctl` drives the build +
  a direct push to the CF managed registry. This is the motor for `docker build`-
  class jobs on `runs-on: corelink` and for building our own prod image off
  GitHub-hosted runners. BuildKit is **not** DinD — it is a userspace builder,
  and the per-lease Firecracker microVM is the isolation boundary. Proven
  end-to-end in a live lease (build FROM ubuntu:24.04 + daemonless registry push,
  run 31664445243) before this bake. The `buildkit-qemu-*` cross-arch emulators
  are deliberately dropped (native amd64 only, ~100 MB saved).
- **docs(S1.6.1): reconcile the design-vs-wired drift.** The Dockerfile no longer
  claims "Docker is deliberately NOT added / DinD infeasible" while the product
  scenario promised container builds; S1.6.1 now reads 🟡 built (BuildKit baked,
  daemonless), with the literal `docker build`-CLI drop-in (a `docker`→buildkit
  shim) named as the next increment.

### 2026-08-04 — the recovery net was healthy and pointed at the wrong repo

- **fix(cloudflare): a `HuGR-Labs/corelink-server` job sat QUEUED for 25 minutes with every
  `corelink` runner offline, and nothing re-drove it.** GitHub fires `workflow_job.queued`
  exactly ONCE. That event was dropped, so the job stayed queued+labeled+runnerless with no
  second webhook and no client-side retry. A cancel+rerun got a box **instantly** — which is
  what proves the spawn was **lost**, not refused for capacity.
  **The safety net for exactly this was working the whole time.** `redriveOrphanedJobs` runs
  on the 1-minute cron, lists queued/labeled/runnerless jobs past `RECONCILE_MIN_AGE_MS`,
  clears any stale spawn claim and re-drives them. It is opt-in on `RECONCILER_REPOS`, and
  that allowlist held only `HuGR-Labs/corelink-runners` — so `parseReconcilerRepos` returned a
  list the stranded repo was not in, `redriveOrphanedJobs` skipped it, and **nothing anywhere
  went red.** A missing allowlist entry fails silently by construction; it only ever surfaces
  as "CI is stuck again".
  It became the wrong list on **2026-08-03**, when corelink-server moved its Rust PR-gate and
  docs-ci lanes onto `runs-on: corelink` (server #1020/#1021/#1026). That made it the fabric's
  **heaviest consumer** while leaving it the one first-party repo the recovery path did not
  cover — the gap opened the day the traffic arrived.
  **Fix:** `RECONCILER_REPOS` now covers both first-party repos.
  **Deliberately NOT changed:** `REPO_INSTALLATION_MAP`. That map decides TENANT resolution,
  and nobody has verified installation `150584374` covers corelink-server; guessing it would
  mis-resolve a tenant. Unmapped ⇒ COLD re-drive, the documented fallback, and precisely the
  level corelink-server spawns already run at through the webhook path. So this changes
  recovery and nothing else.
  **Regression-locked** by `test/reconciler-allowlist.test.ts`, which asserts the *shipped*
  `wrangler.jsonc` value rather than a literal, and was proven RED two ways: dropping
  corelink-server fails with the strand explanation, and a `HuGR-Labs corelink-server` typo
  fails on the silent-drop check (`parseReconcilerRepos` filters on a literal `/`, so a typo
  would have halved the allowlist with no error). 393/393 tests pass.

### 2026-08-03 — the keep-alive sweep renews only boxes GitHub says are working

- **fix(cloudflare): a box that never registered stops being renewed for two hours.**
  The keep-alive sweep (added 2026-08-02 to stop `sleepAfter = "15m"` acting as a hard cap on
  job *duration*) renewed the idle timeout of **every** box that still had an `rhandle:`
  binding. Its own comment justified that — *"a box that still has an `rhandle:` binding is …
  one with a job on it"*, *"a stuck box is still reclaimed"* — and **both statements were
  false in one common case.** The binding is written at **spawn** (`index.ts`, `spawnRunner`),
  not at registration, with `JOB_PAT_TTL_S = 7200`. A box that boots and never registers with
  GitHub therefore never produces a completion event naming it, nothing ever drops its binding,
  and the 1-minute cron renewed it ~120 times — defeating the 15-minute idle window entirely and
  holding a standard-4 (4 vCPU / 12 GiB) out of `max_instances: 20` for two hours. Plausibly a
  **larger** slot sink than the ghost containers fixed the same day, and a candidate for why a
  nominal ceiling of 20 behaves like ~7. Both comments are corrected in place: a comment that
  asserts an invariant the code does not hold is how this survived a review that found it.

- **The binding proves a box was STARTED; only GitHub can say it is WORKING.** `rhandle:` now
  carries the GitHub runner id + repo + installation alongside the DO handle, and the sweep
  verifies each box against `GET /repos/{owner}/{repo}/actions/runners/{runner_id}` — documented
  to return `status` (`"online"`/`"offline"`) and `busy` (boolean). Only `busy: true` renews.
  The id is recorded at spawn, from the `generate-jitconfig` response, precisely because waiting
  for a registration that may never happen would leave the leaking box unverifiable. Values
  written before this change are bare handle strings; they parse as unverifiable and keep being
  renewed, so a deploy does not start reclaiming the in-flight boxes it knows least about.

- **⛔ Keyed on the RUNNER, never on the job.** `generate-jitconfig` binds a runner to a repo +
  label set and to nothing else, so GitHub assigns queued jobs to idle runners by label match
  and the job→box mapping is a permutation: *"job A is still queued"* does **not** imply *"the
  box we started for A is idle"*. Teardown keyed on the spawn's jobId is what SIGKILLed five
  live customer jobs on 2026-08-02. The sweep asks about one specific runner id and acts only on
  that runner's own reported state. The regression guard for that incident is the most important
  cell in `test/keepalive-verified-busy.test.ts`.

- **Fail SAFE, not clean.** An inconclusive check — no credential, API error, rate limit,
  unrecognised body, an undocumented `status`, a legacy binding, a cold spawn, or a tick past
  the per-tick verification cap — **keeps the box renewed**. Leaking a slot is recoverable;
  killing a running customer job is not. And stopping is not killing: renewing sets the deadline
  to now + 15 m, so a box must be continuously verified-not-busy across ~14 one-minute ticks to
  actually sleep, and a single busy observation anywhere in that span restores the full window.

- **Observable.** New registered counters `keepalive_renewed_busy`, `keepalive_stopped_idle`,
  `keepalive_renewed_unverifiable` — together the live-binding count, with `unverifiable`
  metering exactly how much the fail-safe default is costing. Auditing `COUNTER_NAMES` while
  adding them turned up **four** counters bumped at their seams but never registered
  (`webhook_installation_not_allowlisted`, `rate_limit_deadletter_capped`,
  `vcpu_ceiling_approaching`, `vcpu_ceiling_exceeded`), so `snapshot()` never 0-filled them and
  "this never happened" looked identical to "no such signal". Now registered.

- **Not fixed, deliberately: the #444 re-drive still does not destroy the box it replaces.**
  The only identifier that record carries is the **job** id, and killing a box on a job-keyed
  lookup is exactly the 2026-08-02 correlation. It no longer needs to: whatever that box is, the
  sweep now judges it on its own runner's state — still busy with somebody else's job ⇒ renewed
  (correct), genuinely idle or never registered ⇒ reclaimed by `sleepAfter` within one idle
  window. The leak that branch left collapses from ~2 h to ~15 min with no new kill path.

- Suite: **365 → 390** tests (22 files). Eight cells go red against the pre-fix
  `src/index.ts` and green with it.

### 2026-08-03 — Node.js 22 + pnpm baked, so a `run:` step on the box can call them

- **feat(runner-image): bake Node.js v22.23.2 and pnpm 10.32.1 (digest-verified, X4 floor).**
  The actions-runner agent already ships its own Node under `externals/node20` / `node24`,
  which is why JS-based `uses:` actions work on this box today — but that Node is private to
  the agent and is not on `PATH`, so a workflow `run:` step calling `node`/`npm`/`pnpm` saw
  nothing. Two new digest-pinned stages (`node-download`, `pnpm-download`) install to
  `/usr/local/node` + `/usr/local/pnpm`, symlinked onto `/usr/local/bin` while still root and
  before the `USER runner` switch, so they resolve for the unprivileged user that runs the job.
  The build ends with `node --version && npm --version && pnpm --version`, so a broken symlink
  fails the build closed rather than a customer's job.

- **The versions are corelink-server's, not "latest".** node 22 is what its
  `actions/setup-node` steps ask for (`node-version: 22` in admin-ui-ci, admin-ui-e2e,
  lighthouse-ci) and v22.23.2 is the current 22.x LTS; pnpm 10.32.1 is its root
  `package.json` `packageManager` and its `setup-pnpm` composite default. The pnpm pin must
  match **exactly** — that composite compares `pnpm --version` to the pinned string and falls
  back to downloading pnpm on any mismatch.

- **Stated honestly: this unblocks nothing.** Those workflows already carry
  `actions/setup-node`, and the `setup-pnpm` composite has a `pnpm/action-setup` fallback
  written for "a future hosted/Linux runner", so they could move to `runs-on: corelink` today
  and pay a per-run download. Baking buys two things: it removes that download from every run
  (~287 runs / 3 days across the four heaviest JS workflows, 2943 billed `ubuntu-latest`
  minutes / $17.66), and it stops the move depending on a fallback branch that has never once
  executed. `lighthouse-ci` and docs-ci's axe / a11y-baseline jobs additionally need a system
  Chrome and are **not** served by this change.

- **Browsers deliberately not baked, with the numbers.** node+pnpm adds ≈166 MiB uncompressed
  / ≈50 MiB gzipped (node's 62 MiB of `include/` headers are stripped — node-gyp fetches its
  own). Playwright's 1.61.x browser set would add ~395 MiB of download / ~1 GiB on disk, 6–8×
  as much, paid on every spawn by every Rust job, and version-locked to a `@playwright/test`
  pin that is already skewed between the two apps (1.61.1 vs 1.61.0). `playwright install` at
  job time stays correct; a fatter shared image does not.

- **Not proven by this change:** the binaries' presence on the built image. That is provable
  only after a rebuild + roll — `docs/runbook/runner-image-rollout.md` gains the `run:`-step
  behavioural check (a `uses:` step would pass either way, on the agent's private Node).

### 2026-08-03 — a retried container start no longer leaves a ghost box holding a fleet slot

- **fix(cloudflare): cancel the superseded start attempt instead of abandoning it.**
  `FLEET_MAX_CONCURRENCY` is 20 and `max_instances` is 20, and the fleet behaves like ~7: a
  24-box fan-out on 2026-08-03 placed 13, peaked at **7 simultaneous**, and left 11 jobs queued
  for 17 minutes; the Containers API reported `healthy: 7 / active: 0` long after the jobs had
  finished. `sleepAfter` explains a *finished* box holding its slot for 15 minutes. It does not
  explain boxes that never ran anything.

  `startWithRetry` did. On a failed or hung start it minted a **fresh DO handle** for the next
  attempt and dropped the previous one on the floor. Nothing referenced that handle again — no
  `jhandle:`/`rhandle:` binding is written for an attempt that failed — so no teardown path
  could reach it and the keep-alive sweep never saw it. And "the start failed" never meant "no
  container was created": `@cloudflare/containers` 0.3.7 issues `container.start()` and only
  then polls up to 8 s for the instance (`dist/lib/container.js:1378-1421`), while our own 8 s
  race rejects a promise the DO-side RPC keeps running to completion regardless. Each failed
  attempt could therefore leave a **standard-4 (4 vCPU / 12 GiB) instance running with nobody
  holding its handle**, out of `max_instances: 20`, until `sleepAfter` reaped it 15 minutes
  later — up to 2 per spawn, and worst exactly during the bursts the fleet exists to absorb.

- **One JIT registration per ATTEMPT, never shared.** The mint moved from `driveSpawn` (once per
  job) into the retry loop (once per attempt). `generate-jitconfig` registers a **single-use**
  runner, so booting two boxes with one config meant at most one could ever register — GitHub
  answers the second with "A session for this runner already exists" — and *which* one won was a
  race we did not control. When the abandoned box won it, the box we tracked and bound
  `rhandle:` to was the one that could not work.

- **The cancelled attempt loses its registration first, then its container.** Order is
  deliberate: deleting the registration (`DELETE /repos/{repo}/actions/runners/{id}`) is the
  only step that bounds the worst outcome — a late-booting box CLAIMING a customer's job on a
  container no binding points at, which the keep-alive sweep would never renew and `sleepAfter`
  would SIGKILL mid-job 15 minutes later. It does not depend on the destroy succeeding.

- **A destroy that races a still-provisioning start is now survivable.** Each cancelled attempt
  writes a durable `ghost:<handle>` record; `sweepGhostContainers` (1-minute cron, ahead of the
  re-drive reconcilers so they place into a fleet whose ghosts are already back) re-destroys and
  **confirms via `isAlive()` before forgetting it**. A box still up after a destroy keeps its
  record and is logged at ERROR (`ghost_container_still_alive`) rather than dropped.

- **It is counted and named.** New golden signals `container_start_abandoned` (attempts we
  cancelled) and `ghost_container_reaped` (ghosts the cron has confirmed down), plus
  `placement_unconfirmed`, which the previous change already bumped but never listed — so it
  read as absent instead of zero on the snapshot. A container that exists and can never do work
  is lost fleet capacity; it must be a named event, not an unexplained hole.

- `/v1/spawn` (the fabric contract) cancels its superseded attempts too, in the correct DO
  namespace for the mode. That path takes its JIT from the caller and cannot re-mint one, so the
  destroy is not merely capacity hygiene there — it is what stops a late box from consuming the
  single-use registration the surviving box needs.

- `test/ghost-containers.test.ts` (14 cells): 11 of them fail on the pre-fix code and pass after.
  Suite 351 → 365.

### 2026-08-03 — the compile-cache pilot stops measuring nothing

- **feat(runner-image): bake `sccache` v0.17.0 into the runner box image.** CoreLink
  server PR #1017 wired the `runs-on: corelink` lane of its `corelink-reapi` gate to send
  every `rustc` invocation through CoreLink's own `/cargo/<tenant>` WebDAV cache — the
  cache earning its keep on the compute we sell. It has been **inert since it landed**:
  the binary is not on the box, so the lane printed `sccache not on the box image — lane
  compiles cold, nothing breaks` (job `91830316792`) and went green having cached nothing.
  Fail-open was the right design; it also meant the pilot proved nothing. This installs
  the client half.

- **Baked at build time, not fetched at job time.** The download happens once on a hosted
  image builder, so an ephemeral runner needs **no runtime egress to GitHub** to get its
  cache client — its only required egress stays `corelink-api.humangr.com`, which is what
  keeps the ADR-0003 egress posture narrow. And emphatically not `cargo install sccache`:
  compiling the cache client would cost more build time than the cache ever saves.

- **Same X4 shape as every other pinned artifact in that Dockerfile.** A digest-pinned
  download stage on the identical `ubuntu:24.04@sha256:786a8b…` base, `ARG SCCACHE_VERSION`
  + `ARG SCCACHE_SHA256`, and `sha256sum -c` **before** extraction — never after. The pin
  was verified the way the rustup-init pin was: the 9,561,816-byte tarball downloaded over
  TLS, `shasum -a 256` recomputed locally, matched against upstream's published `.sha256`
  sidecar. The static musl build needs no runtime deps, and the binary is inert unless a
  workflow sets `RUSTC_WRAPPER=sccache`.

- **docs(runbook): `docs/runbook/runner-image-rollout.md`** — build → re-pin → deploy →
  **roll** → verify. The fourth step is the one that gets skipped: a `wrangler deploy` does
  not reboot a container that is already running, so the roll must be forced through the
  Cloudflare Containers API (`POST …/containers/applications/<app>/rollouts`), and the
  verification must read the Containers API rather than trusting a green checkmark. Landing
  the Dockerfile change does **not** put the binary on the fleet; only that procedure does.

### 2026-08-03 — a spawn that "succeeded" and placed nothing no longer loses the job

- **fix(cloudflare): confirm placement instead of assuming it.** A burst above the concurrency
  ceiling could still lose jobs — but not for any of the reasons the recovery path was built to
  handle. In corelink-server run `30826164339` (24-job fan-out, ceiling 20) the Worker drove all
  24 spawns, refused 8 at the ceiling, hard-failed 3 container starts, and **recovered every one
  of them**: 9 `orphan_recorded`, 9 `orphan_retry_recovered`, and **zero** `orphan_retry_giveup`
  / zero `orphan_refusal_giveup`. The 3-strike budget was never approached. 11 jobs were lost
  anyway, and sat `queued` with no runner for the next 17 minutes while the fleet was idle and
  the 1-minute cron ticked 17 times.

  They were lost in the one state nothing watched. `recordOrphan` runs from
  `driveSpawnGuarded`'s CATCH, so the dead-letter only ever sees a spawn that THREW — an error,
  or the typed ceiling refusal. A spawn that returns normally and still leaves the job unplaced
  has no branch at all: the Worker logs `runner_spawned` and moves on. But `driveSpawn` returns
  as soon as the CONTAINER started, and a container that starts is not a job that got placed —
  the box has still to come online and claim it. When it does not, GitHub simply keeps the job
  queued, `workflow_job.queued` is never redelivered, and nothing anywhere reports a failure.

- **A successful spawn now writes a PROVISIONAL record.** The same `orphan:<jobId>` dead-letter,
  carrying `placedMs` — "a box was started; placement unconfirmed". Reusing one record is
  deliberate: it keeps ONE lifecycle and ONE set of bounds per job, so a job cannot launder
  itself out of `MAX_ORPHAN_ATTEMPTS` by passing through the success path. `attempts` and
  `firstRecordedMs` are carried over, so a re-spawn never hands back a fresh budget.

- **Confirmation is a question to GitHub, not a webhook.** Past a 3-minute grace window the
  reconciler reads that one job (`/actions/jobs/{id}`) and re-drives **only** on an
  authoritative "still queued, still no runner". Keying this on a `workflow_job.in_progress`
  delivery was rejected: it would make correctness depend on a delivery we do not control and
  currently ignore, and getting it wrong re-spawns a HEALTHY job — burning a concurrency slot
  and real COGS on something that was never in trouble. Every other answer, **including an
  unreachable API**, leaves the job alone. `workflow_job.completed` clears the record directly,
  so the common case costs zero API calls.

- **Still fast-fails.** An unconfirmed placement rejoins the ordinary retry path and **bumps**
  `attempts`, so a box that can never come online (a bad image, a broken registration) dead-
  letters loudly within `MAX_ORPHAN_ATTEMPTS` ticks instead of respawning forever on our COGS.
  Ceiling refusals still do not spend that budget — a full fleet is backpressure, not a fault —
  and stay bounded by the absolute `ORPHAN_TTL_S` window.

- **The reconciler no longer DELETES on a successful drive.** That was correct only while
  "drive returned" meant "job placed". It does not, and deleting there discarded the provisional
  record precisely for the jobs already known to be in trouble.

- Behavioural regression (`test/burst-above-ceiling.test.ts`) replays the measured burst through
  the real reconciler tick by tick: 24 jobs, ceiling 20, 11 boxes that start and never register.
  Before, exactly `job-14`…`job-24` strand; after, all 24 are placed. Sibling cases assert that a
  permanently-broken job stops within the 3-strike bound and that a healthy in-flight job is
  never re-driven.

### 2026-08-02 — warn the customer BEFORE the overage, not on the invoice

- **feat(cloudflare): near-ceiling warning on the monthly `max_vcpu_h` allowance.** Overage is
  priced at $0.30/vCPU-h (3× COGS), so crossing the included allowance is expensive — and for
  an SMB self-serve buyer a surprise invoice is a **churn event**, not an upgrade conversation.

  The two halves could not see each other: the ALLOWANCE lives in D1
  (`runners_entitlement.max_vcpu_h`) and the CONSUMPTION only exists in this Worker, the one
  component that sees every job finish. The mint now forwards the allowance
  (corelink-server #975); this caches it per tenant at spawn (`vceil:<tenant>` — a property of
  the subscription, so one refreshed key beats a write per job) and, at completion, accumulates
  the job's vCPU-seconds into `vused:<tenant>:<period>` and evaluates the thresholds.

- **Warns at 80% and again at 100%, each exactly ONCE per period.** A tenant parked at 85% runs
  hundreds of jobs; an alert that repeats on every one gets filtered, which is the same as no
  alert. Marker keys (`vwarn:<tenant>:<period>:<threshold>`) are written BEFORE announcing —
  a duplicate is worse than a late one here. Only the HIGHEST newly-crossed threshold is
  reported, so one long job that jumps 0% → 150% says *"you are over"* rather than replaying
  the history. A new month resets both the counter and the markers, because the allowance is
  monthly.

- **Deliberately NOT a gate.** Crossing the ceiling never stops a job — the customer keeps
  building and pays the overage. Stopping someone's CI mid-sprint is a worse outcome than
  charging them, which is exactly why overage exists instead of a hard block. The warning's
  only power is to make the bill unsurprising.

- **Absent is not zero.** No ceiling on file ⇒ never warn. Treating absent as 0 would divide by
  zero and warn every tenant who never bought a metered tier — the loudest possible way to be
  wrong. Guarded on both sides (server #975 omits 0/negative; this refuses them again).

- **Consistent unit with the bill:** the warning counts ALLOCATED wall-clock × vCPU, the same
  arithmetic as the billable `runner_vcpu_seconds` event. If those diverged the warning would
  fire at the wrong moment. The KV counter is a racy read-modify-write and that is ACCEPTED and
  documented: it drives a human-facing warning, not an invoice — the invoice comes from the
  per-job idempotent usage ledger.

- **Mutation-tested,** not just covered: flipping `>=` to `>` reds 3, treating an absent ceiling
  as 0 reds 1, dropping the dedup reds 2, and first-wins instead of highest-wins reds 2.
  348 tests pass; `tsc --noEmit` clean.

### 2026-08-02 — 4 max-size containers were burning idle for a feature that is not live

- **fix(cloudflare): cap `CheckHostContainer` at 1 instance until it is live-flipped (was 4).**
  A cost audit read the Containers API and found this app at
  `healthy: 4, active: 0, assigned: 0` — four boxes of **4 vCPU / 12 GiB / 20 GB**, which is
  this account's per-deployment CEILING (`vcpu_per_deployment=4`,
  `memory_mib_per_deployment=12288`, `disk_mb_per_deployment=20000`) — held since the app was
  created on 2026-07-07, doing nothing.

  Cloudflare bills memory and disk by **ALLOCATION**, not by use, so idle boxes at the maximum
  instance size are the most expensive thing that can be running. The prior headroom was not
  wrong in principle (it is the same leaked-idle reasoning that sized the runner container) — it
  was sized for a LIVE service, and check-host is still not live-flipped, so "ample headroom" was
  headroom for zero traffic.

  1 rather than 0 keeps the app deployable and smoke-testable. Raise it back to 4+ **in the same
  change that live-flips check-host**, not before.

  ⚠️ Config-only: this takes effect on the next `deploy-spawn-worker.yml` dispatch, which is
  manual by design. Until then the four boxes stay up.

### 2026-08-02 — the runner usage meter was in the wrong unit (4× under-bill)

- **feat(cloudflare): emit `runner_vcpu_seconds`, the BILLABLE runner compute unit.** The owner
  superseded the *"concurrency priced, minutes unlimited"* runner model: minutes above a tier's
  included `runners_entitlement.max_vcpu_h` are now billed as overage at $0.30/vCPU-h (3× the
  measured $0.10/vCPU-h COGS).

  `buildUsageEvent` pushed `runner_slot_seconds` — wall-clock seconds a SLOT was held. The
  entitlement it meters against is denominated in vCPU-**HOURS**. On the current 4-vCPU box those
  differ by **exactly 4×**, and *both read as "seconds"* — so arming the (already-built) push
  as-is would have billed a quarter of what it should, and looked entirely reasonable doing it.

  `qty` is now `allocated_seconds × the box's vCPU count`, multiplied HERE — the emitter is the
  only component that knows the box it just tore down. That keeps the wire shape frozen AND makes
  the unit correct when the fleet stops being one size: an 8-vCPU or high-memory SKU bills right
  the day it ships. `RUNNER_BOX_VCPU` is a defaulted PARAMETER, not a constant read, precisely so
  a mixed-size fleet only has to pass the real number.

  ALLOCATED wall-clock, never `cpuTimeSec`: Cloudflare bills memory + disk by allocation
  (measured 490 allocated-s vs 170 cpu-s on a real run), so CPU-time metering under-counts COGS
  ~3× and breaks the ladder's loss-proof floor.

- **`conformance/UsageEvent.json` is deliberately UNCHANGED.** It is fabricd's golden and fabricd
  still emits the CAPACITY kind, so the Rust consumer test is untouched by construction — only
  TypeScript moved. In the emitter's own conformance test, `event_kind` and `qty` moved off
  equality-with-the-vector onto the billing ARITHMETIC (`qty === allocatedSeconds × vCPU`, plus
  `qty === vector.qty × RUNNER_BOX_VCPU` so the two kinds stay reconcilable). That follows the
  convention the file already used for `source` and `idem_key`, and it is a STRONGER pin, not a
  weakened one: an equality against one frozen example passed just as happily with the multiplier
  missing.

- **Proven RED:** removing the multiplier reds 4 tests (`expected 3 to be 12`,
  `expected 120 to be 480`, `expected 3 to be 48`). New coverage: an explicit `vcpu` override (a
  16-vCPU box bills 4× a standard-4), and a guard that a nonsense vCPU count (0 / negative / NaN /
  absent) falls back to the fleet default rather than ZEROING the bill — a zeroed bill is
  indistinguishable from a job that never ran. 335 tests pass; `tsc --noEmit` clean.

- **Still unarmed:** `BILLING_INGEST_URL` is unset, so nothing is pushed. The 60-day usage ledger
  (101 records) is the backfill source once it is armed. Server half:
  HuGR-Labs/corelink-server#972.

### 2026-08-02 — a spawn refused by the RATE LIMITER no longer strands the job

- **fix(cloudflare): the 429 branch dropped the job permanently — the same defect #437 fixed
  for the ceiling refusal, surviving in a sibling branch.** `WEBHOOK_LIMITER` refuses past
  30 spawns / 60 s per repo and the handler simply `return`ed 429: no claim, no slot, and —
  because the check ran BEFORE the repo and installation id were even resolved — no
  dead-letter. GitHub delivers `workflow_job.queued` exactly once and does not redeliver a
  non-2xx, so the customer's job sat `queued` forever with nothing reported as failed.

  This is the burst shape the product exists to serve: our own dogfood has **24 workflows on
  `runs-on: corelink`** and the 2026-08-01 burst was ~24 jobs — under the cliff, but not by
  much. Any customer matrix past 30 concurrent `queued` events lost the overflow silently.

  The limiter now runs AFTER the installation-id resolution (so a refusal knows enough to
  record the job) and a refused job is dead-lettered for the scheduled reconciler, exactly
  as a ceiling refusal is. **The 429 itself is unchanged — only whether the job survives it.**

- **The dead-lettering is BOUNDED, and that bound is the load-bearing part.** Recording every
  refusal would have made the limiter the AMPLIFIER rather than the bound: `driveSpawn` mints
  the per-job CAS PAT **before** it checks the concurrency slot, so every reconciler retry
  costs a real HTTP mint against corelink-server even when the spawn is then refused at the
  ceiling. An unbounded flood of dead-letters would therefore become sustained mint load.
  `RATE_LIMIT_DEADLETTER_MAX = 60` per repo per 5-minute window sits far above a legitimate
  CI matrix and far below a flood; past it, jobs are dropped as before but **LOUDLY**
  (`rate_limit_deadletter_capped` at ERROR + a metric), because "we shed load" and "we lost
  work" must never look the same in the logs.

  ⚠️ Recorded while reasoning about this: the limiter is **not** an abuse control against a
  party holding the webhook HMAC secret. Its key is `spawn:<repository.full_name>` — a value
  that party controls — so varying the repo string sidesteps it by construction. The real
  containment for a forged repo is server-side (the mint derives the tenant and 403s a repo
  not allowlisted to it) plus the fleet-wide slot DO. The new cap keeps the RECOVERY path
  proportionate; it does not replace either.

- **A COLD refusal (no installation id) still records nothing** — it cannot be re-driven WARM
  without bypassing per-job authz. Same deliberate gap as `cell12-deadletter-cold`, now
  pinned by its own test and logged (`rate_limit_deadletter_skipped_cold`) rather than left
  to be inferred from an absence.

- **Regression-locked, and the ordering change caught its own bug.** Moving the limiter below
  the `!repo` 400 initially let a repo-less flood bypass it entirely — caught immediately by
  the existing I1 pin ("a payload with NO repository is STILL rate-limited"), so the 400 was
  deferred to run after the limiter instead. The main pin was proven RED against the pre-fix
  behaviour (`expected undefined to be defined`). 334 tests pass; `tsc --noEmit` clean.

### 2026-08-02 — every fabric job longer than ~15 minutes was being SIGTERMed

- **fix(cloudflare): `sleepAfter` was a hard cap on job DURATION, not an idle timeout.**
  In `@cloudflare/containers` 0.3.x, `sleepAfterMs` only moves forward via
  `renewActivityTimeout()`, and `isActivityExpired()` renews only while
  `inflightRequests > 0` — a counter incremented **solely** inside `containerFetch`.
  `RunnerContainer` has no `defaultPort` and is never `containerFetch`ed (the GH Actions
  agent is the image entrypoint and dials OUT; nothing dials in). So the counter stayed 0
  forever, the deadline froze at container-start + 900 s, and `alarm()` →
  `onActivityExpired()` → `stop()` SIGTERMed the box mid-job.

  The in-code note asserting *"A running job keeps the container active, so this never cuts
  a live job"* was false. Consistent with the longest fabric job that ever succeeded: 864 s
  (14.4 min). `coverage.yml` (45 m), `perf-nightly.yml` (60 m) and `dr-drill-monthly.yml`
  (60 m) already target `runs-on: corelink` and would all have been killed.

  The fix supplies the activity signal the SDK cannot observe, which is what its own docs
  ask for (*"Call this method whenever there is activity on the container"*): the 1-minute
  cron calls a new `RunnerContainer.keepAlive()` on exactly those boxes that still hold an
  `rhandle:` binding — i.e. those whose completion webhook has not arrived. On completion,
  teardown drops the binding, renewals stop, and the box idles out through the normal path.

  **Deliberately not just a bigger number.** Raising `sleepAfter` would have turned every
  stuck box into a multi-hour hold on `max_instances`, trading a job-killer for a
  fleet-starver — and starving spawns is what pushes jobs into the ceiling-refusal path
  fixed earlier the same day. A lost completion webhook stays bounded by the binding's own
  KV TTL (`JOB_PAT_TTL_S`), after which renewals stop on their own.

### 2026-08-02 — teardown no longer SIGKILLs a box that is running someone else's job

- **fix(cloudflare): correlate container teardown on `runner_name`, not the spawn-request
  job id.** `generate-jitconfig` binds a runner to a repo + label set and to **nothing
  else** — not to the job whose webhook prompted it. GitHub then assigns queued jobs to
  idle ephemeral runners by **label match**, so with N identical jobs and N identical
  runners in flight the mapping is a **permutation**: the box minted for job A routinely
  runs job B. Teardown keyed on `jhandle:<jobId>`, so job A completing destroyed the box
  minted for A — which was still executing B.

  Observed in production: five jobs killed mid-step (one inside `cargo clippy`, one during
  `Complete job` with its work already finished), each surfacing ~600 s later as GitHub's
  *"The self-hosted runner lost communication with the server."* **0 deaths across 59 SOLO
  jobs, 5 across 27 that had at least one other box alive** (Fisher p≈0.002), with **no
  concurrency threshold** — deaths at 2, 6, 7, 8 and 9 concurrent boxes with survivors
  interleaved at the same counts. A correlation bug, not a capacity limit.

  ⚠️ The 600 s figure is **not** a timeout — those workflows set no `timeout-minutes`. It
  is GitHub's reaper for a runner that went silent, and the "hung step" in the UI is
  merely the last thing GitHub heard before the SIGKILL. Read the **annotation**, not the
  duration.

  The runner name we mint at `generate-jitconfig` is echoed back by GitHub as
  `workflow_job.runner_name`, so it identifies the box that **actually ran** the job
  whatever permutation GitHub chose. It is now stashed as `rhandle:<runnerName>` at spawn
  and is the primary teardown key; `jhandle:<jobId>` remains only as a fallback for a
  completion carrying no `runner_name` (a job cancelled before assignment) and for records
  written before this shipped. Both bindings are dropped on teardown so no stale pointer
  survives a redelivered completion. A `runner_minted` log line now records
  `jobId → runnerName`, without which the permutation is invisible.

### 2026-08-02 — a spawn refused at the ceiling no longer loses the customer's job

- **fix(cloudflare): a ceiling refusal is BACKPRESSURE, not a dropped job.** Found by moving
  `corelink-server`'s own CI to `runs-on: corelink` and pushing ~24 jobs at once: **12 got
  boxes and went green; 12 sat `queued` forever** with zero containers running and nothing
  reported as failed. Two defects compounded:

  1. `driveSpawn` **returned** at the concurrency ceiling. `driveSpawnGuarded` writes the
     dead-letter only from its `catch`, so a refused spawn recorded nothing — and GitHub
     sends `workflow_job.queued` exactly once and never redelivers it. No record ⇒ no
     recovery, ever.
  2. `retryOrphanedSpawns` drives the **throwing** `driveSpawn` and reads a normal return as
     recovery. So even once a dead-letter existed, the first refused retry tick would have
     **deleted** it — the recovery path destroying its own evidence.

  The ceiling now throws a typed `SpawnRefusedError`, which routes the job to the dead-letter
  while staying distinguishable from a genuine failure. A refusal does **not** consume the
  `MAX_ORPHAN_ATTEMPTS` (3) budget — that budget bounds real errors, and spending it on
  backpressure would still lose every job behind a burst lasting more than three cron ticks.
  Refusals are instead bounded by an **absolute** `ORPHAN_TTL_S` (30 min) window stamped at
  first record (`OrphanRecord.firstRecordedMs`), so re-putting the record each tick can never
  extend its deadline; exhausting the window gives up **loudly** (`orphan_refusal_giveup`) as
  the real capacity fault it is. A busy fleet keeps counting `spawn_at_ceiling`, never
  `spawn_failed`, so healthy burst load cannot bury real failures.

  Neither the cap nor the refusal itself changed — only whether the job survives one.
  Unchanged and pinned: a COLD refusal (no `installation_id`) still records nothing, since it
  cannot be re-driven WARM without bypassing per-job authz/mint.

### 2026-08-01 — GitHub org migration `HumanGuardrail` → `HuGR-Labs` (repo slug + App installation)

- **fix(cloudflare): repin the spawn-Worker to the migrated org + installation.** The CoreLink
  repos moved to the `HuGR-Labs` org on 2026-08-01 and the `corelink-runners` GitHub App
  (id 4222041) was transferred + reinstalled, changing the installation id **`144561227` →
  `150584374`**. `REPO_INSTALLATION_MAP` (`{"HuGR-Labs/corelink-runners":"150584374"}`) and
  `RECONCILER_REPOS` (`HuGR-Labs/corelink-runners`) in `deploy/cloudflare/wrangler.jsonc` are
  matched by EXACT string against the webhook payload's `repository.full_name` — GitHub's
  transfer redirect does NOT apply to a string compare — so both stale values would have
  produced the silent failure mode `runs-on: corelink` picks up no job (403 / COLD spawn with
  no tenant derivation). Requires a Worker redeploy to take effect (owner action).
- **fix(runner-image/check-host): repin the `clw-releases` download origin** to
  `HuGR-Labs/clw-releases` (the release repo migrated too); minisign key unchanged.
- **test(fabric): the org-rename regression (G5) now guards BOTH dead slugs.**
  `stale_org_slugs_denied_against_hugr_labs_allowlist` asserts that `humangr-labs` **and**
  `HumanGuardrail` acquires are denied `400` against the live `HuGR-Labs` allowlist, reserving
  no slot and minting nothing. Fixtures across the Rust + Worker suites repinned to the new
  org/installation.
- **docs:** runbooks (`incident-playbook`, `cloudflare-go-live`, `dogfood-go-live`,
  `secret-inventory`), the fabric env reference, the product catalogs, the SDK/integration
  package metadata, and the agent skills repinned. `docs/handoff/**` and `docs/review/**`
  are left as-is (dated historical record).
- **fix(option-c): repin the cold-organic proof repo — it migrated too.**
  `corelink-cold-organic-e2e` is now `HuGR-Labs/corelink-cold-organic-e2e` (verified
  2026-08-01 via `gh api repos/HuGR-Labs/corelink-cold-organic-e2e`; an earlier revision of
  this entry wrongly claimed it had stayed on `HumanGuardrail`). This one is NOT cosmetic:
  `REPO_TENANT_PAT_MAP` is keyed by the exact `owner/repo` string and consumed by
  `tenantPatSecretForRepo` (`deploy/cloudflare/src/lib.ts`) against the webhook's
  `repository.full_name`, so a stale key silently misses → no Option-C dispatch → the
  cold-organic box mints under the **installation-derived dogfood tenant `d863fafb` instead
  of `3c7d77b1`**, attributing cache + billing to the wrong tenant with no error. All 10
  live references repinned (the three agent skills, the `Env` doc-comment in
  `deploy/cloudflare/src/index.ts`, and the Option-C vitest fixture).
- **fix(cloudflare): `REPO_TENANT_PAT_MAP` is now pinned to its live value in
  `wrangler.jsonc`** (`{"HuGR-Labs/corelink-cold-organic-e2e":"COLD_ORGANIC_TENANT_PAT"}`)
  instead of `"{}"`. The var held `"{}"` while prod ran a non-empty map, so ANY
  `wrangler deploy` silently DISARMED Option-C — including the redeploy this change
  requires. The map contains only a secret NAME (the PAT itself stays the
  `COLD_ORGANIC_TENANT_PAT` secret binding), and a mapped-but-unbound secret still falls
  back to the default installation-derived mint, so config-as-code here is fail-safe.
- **`conformance/AcquireRequest.json` repinned** to `"owner": "HuGR-Labs"` (+ its byte-exact
  golden in `conformance_lease_dtos.rs` and its `conformance/manifest.sha256` digest). This
  vector is not illustrative: the server TL seeded the production D1 `runner_repo_allowlist`
  row from it (`docs/handoff/2026-07-08-REPO-SLUG-DEFINED-*`), which is what produced the
  2026-07-08 slug incident — and the previous org rename repinned it for exactly that reason
  (`b76f2f3`). The "frozen from hugit's side" constraint is dead: hugit is discontinued.

### 2026-06-17 — Cold-start S-class hardening + Phase-2 cache-moat relays (#87)

- **fix(runner): admit-time box-backend guard (S2).** A runner-mode acquire under the no-op
  `NoBoxProvisioner` (default-off, no cloud backend) is now rejected `400` AT ADMIT — symmetric
  with the broker guard — instead of admitting to a `Held` box that never binds and hangs the
  GitHub job. Adds `BoxProvisioner::binds_boxes()` (default `true`; `NoBoxProvisioner` → `false`);
  the guard precedes the cap reserve, the JIT mint, and the rate-window push, on the synchronous,
  queued, and webhook-autoscaler acquire paths.
- **fix(cloud-engine): runner-box ephemeral-disk floor (S3).** A RUNNER box (`allow_egress`)
  resolving below `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB` (4096 MiB) now fails closed at `spawn` with
  an actionable error, instead of silently inheriting the 1 GiB CHECK default and ENOSPC-ing
  mid-build (cold-start north star: cache absent ⇒ slow, never broken). Does not change the #82
  Northflank-allowance posture.
- **fix(runner-image): base-pin hygiene (S1).** The ubuntu base digest was already pinned (#75);
  removed the stale `<PIN-AT-BUILD>` comments and rewrote the `build-and-push.sh` guard to validate
  the `FROM` lines (it had been false-positive-warning on its own comments).
- **docs(cache-moat): Phase-2 cross-TL relay asks + resume state.** The CoreLink Cache TL
  warm-boot seam ask (CT-Q1 overlay-vs-per-job-fetch, CT-Q2 protocol — gates P3) and the clw TL
  confirms.
- **Quality:** two independent adversarial cold reviews (APPROVE, no correctness defects); full
  local CI-equivalent gate green (`fmt`/`clippy --workspace -D warnings`/`test --workspace` =
  682 passed + `cargo deny`); zero dependency changes.

### 2026-06-15 — Direct-CI runner-lease lifecycle wired (ADR-0007 Stage A)

- **feat(runner-lease): acquire-time runner-fleet wiring — `AcquireRequest.runner`,
  broker-gated JIT mint, egress fork, `/exec` refusal.** A runner-mode acquire
  (`runner: Some`) forces the lease's `net_policy` to `"egress-runner"` server-side,
  builds the box through `ContainerSpec::from_runner_lease` (the C2 egress floor, #69),
  mints an ephemeral GitHub Actions JIT registration config via the (default-off)
  `RunnerRegistrationBroker`, and injects it into the box env as
  `CORELINK_RUNNER_JITCONFIG`. `/exec` is refused on a runner lease (it runs its own
  ephemeral agent). The mint runs outside the ledger lock and fails closed — a mint
  failure frees the reserved slot and never provisions a config-less egress box. Both
  the immediate and the queued (`FABRIC_ADMISSION_MODE=queue`) admission paths converge
  on the shared `finalize_admitted_lease`, so the JIT mint covers both.
- **Default-off, byte-unchanged check path.** With no broker wired (`with_runner_broker`),
  a runner acquire is rejected `400` before any slot is reserved, and the classic hugit
  check-exec lease is byte-for-byte unchanged (still hermetic `no_network`, §13.2 ingest
  token injected, no JIT config). Egress is granted ONLY via the runner constructor,
  never inferred from a caller `net_policy` string (proven end-to-end in
  `acceptance_runner_lease`).
- **Adversarial-review fix:** `forget_lease` now runs on the normal `/close` path, GC'ing
  the runner-lease marker (and closing a latent `images` side-table leak) — the reaper
  only sweeps `Held` leases, so a closed lease was never reclaimed.
- **Deferred (creds-gated):** the production `GitHubAppBroker`-from-env composition wiring
  (App private key + `ureq` transport) — the lifecycle is fully exercised today via
  `MockBroker`.

### 2026-06-14 — M1 last-mile: billing exporter, tenant onboarding, registry GC, CLI, contract v1.4.0

- **feat(fabric): Wave-6 M1 last-mile — durable billing exporter + runtime tenant onboarding +
  BoxRegistry orphan GC (#51).** `SlotMeter` drains into Postgres `billing_events` table with
  exactly-once upsert by PK; export cadence driven by `FABRIC_BILLING_EXPORT_INTERVAL_SECS`.
  `POST /internal/v1/admin/tenants` enables runtime tenant provisioning without a redeploy,
  guarded by `FABRIC_ADMIN_KEY`. BoxRegistry periodic GC reaps orphaned box entries.
- **docs(plans): rate_ceiling_per_min ratified as an abuse rail, not a price (#53).** Owner-
  confirmed: the cap is a DoS/runaway-cost guardrail only; it does not appear on the pricing
  sheet or in any billing calculation. Closes the last owner-gated M1 flag.
- **feat(conformance): result_binding_v2 cross-repo conformance vector + contract v1.4.0
  RATIFIED (#52).** Signed drift tripwire committed byte-identical in both repos; hugit-side
  v2 verifier PR merged. §7.1 amendment (v1.4.0) closes the P0 attestation-forgery fix
  (binding `CheckResult.exit`/`.artifacts` into `result_binding_sig_v2`). Seams §7/§13 now
  fully closed on both sides.
- **feat(cli): `corelink` client/ops CLI — adoption last-mile smoke + verify (#54).** Thin
  CLI binary in the workspace covering the core operator workflows; smoke tests and a verify
  suite confirm the happy path end-to-end.

### 2026-06-14 — multi-instance, durable state, exhaustive audit

- **feat(fabric): persistent Postgres ledger DEPLOYED + multi-instance proven
  live.** `PgLedger` cross-instance cap-safe (`pg_advisory_xact_lock` + atomic
  count-and-insert); deployed on the Northflank `corelink-ledger` addon;
  `instances=2` proven cap-safe (25 acquires → cap held at 20, advisory-lock
  serialized). Live-only `lease_id` collision fixed via UUID minting (#39).
  Opt-in PG TLS `FABRIC_PG_TLS=disable|require` (#40, default unchanged).
- **feat(fabric): ADR-0004 durable-reap-state.** Phase 1 durable lease deadline
  in the `leases` row → the reaper is a true cross-instance backstop (#43, closed
  the cap-slot leak on instance death). Phase 2a durable envelope checkpoint +
  3-tier abnormal flush (local hook → durable checkpoint → `no_capture` marker)
  → an abnormal reap on any instance always emits a forensic record, never
  silently dropped (#45, closes hugit §13 Item-3 SLA). Owner-ratified Decision-3
  (per-turn cadence, `no_capture` marker).
- **fix(security): comprehensive adversarial audit — P0 attestation forgery +
  27 more, all closed (#46/#47).** 16-dimension workflow (96 agents, each finding
  double-verified): 40 raw → 28 confirmed (1 P0, 10 P1, 11 P2, 6 INFO). **P0:
  `result_binding_sig` did not bind `CheckResult.exit`/`.artifacts`** → a
  forgeable pass/fail verdict on an otherwise-valid attestation under untrusted
  compute → fixed with **`result_binding_sig_v2`** binding the full outcome
  (backward-compat, no flag-day; hugit must add the v2 verifier — §7.1 amendment
  v1.4.0). Plus: memo_key validation before attest, ed25519 `verify_strict`,
  cloud-engine `classify_run_status` fail-closed + injective container names,
  FileLedger `fsync` + torn-journal tolerance, forensic re-scan fail-closed,
  batch-teardown leak surfacing, stale-`Pending` cap-slot sweep, close
  ack-window + global concurrency-limit/load-shed, saturating token sum,
  introspect-vector `deny_unknown_fields` tripwire, X4 oracle single-sourced to
  the production path, real fence red-team escape vectors. Lead cold-verify
  caught a committed-disabled supply-chain gate + a spawn-in-acquire invariant
  break before they shipped.
- **docs: ADR-0004 (durable-reap-state) · ADR-0005 (queued fair admission,
  proposed) · the Northflank+Postgres multi-instance RUNBOOK · the comprehensive
  audit findings tracker · SECURITY + turn-feed handoffs to hugit.**

## [0.1.0-seed] — 2026-06-12

The seed milestone: the proven ephemeral-runner execution core, shipped to
`main` through the repo's first real CI run on `corelink-runners-builder-01`.

- feat(envelope): **S13 wave — §13 contract obligations as mechanism**
  (audit 2026-06-11 → P0). `IntentMetrics`/`TokenCounts`/`ToolCount`
  transcribed @ hugit-contracts 443ff1b with in-crate golden fixture +
  `CONTEXT_ENVELOPE_SCHEMA_VERSION` pin; conformance tripwire hardened
  (real SHA-256 per vector, manifest membership, tamper proof); envelope
  mechanism — derivation collector (saturating meters, exact-integer
  micro-USD), CaptureHook (two bounded in-memory surfaces, bearer seam
  both directions), JobClose ack state machine (fail-closed timeout,
  in-window drain, exactly-once incl. abnormal paths). 41-item acceptance
  suite, cold-reviewed (FIX-FIRST findings closed in-PR).
- docs: ROADMAP (P0 closed · P1 ship-the-seed · M1 fabric · M2 GA);
  contract title v1.2.0; CLAUDE.md refresh; transplant prose fixes.
- ci: default branch `main`; install-action v2.81.10 + pinned tool
  versions (cargo-audit@0.22.2, cargo-deny@0.19.8).

- docs(spec): **contract 1.2.0 — `cost_usd_micros|u64` (E-DOCS, 2026-06-11)**.
  §13.1 money field renamed: `cost_usd|f64` → `cost_usd_micros|u64` (integer
  micro-USD, 1 USD = 1,000,000 units; exact-integer, no f64 epsilon; owner-
  ratified 2026-06-11 as hugit WA4). §13.4 schema version updated to 1.2.0
  SHIPPED. §12 amendment-log entry added. Additive — all other §0–§12 and
  §13.2/13.3/13.4 content unchanged. Conformance-vector drift tripwire: new
  vectors must be committed byte-identical in both repos (tracked).

- docs: **WP-R5 — runner-transfer campaign records** (2026-06-10). CLAUDE.md
  advanced from spec-phase → CODE: workspace status, gate commands, wire-contract
  law, CI labels, and "seeded ≠ shipped" scope statement documented. Handoff note
  `docs/handoff/2026-06-10-runner-seed.md` authored for campaign-#1 sessions:
  what arrived (execution core + fence enforcement + X4 oracle + suites, all gate
  green @ b6319a3; contracts + vectors @ 78702d6; integration contract v1.1 @
  9796aa8), what it proves (lease loop · fence enforcement real · supply-chain
  proven over live spawn surface · wire seam tripwired · envelope emission
  contracted), what the PRODUCT still needs (multi-tenant control plane · public
  API · billing · Firecracker · C5b broker stays hugit-side), and the v1.1
  obligations (per-job metric emission + trajectory blob hook points). hugit side:
  supersession appendix in `docs/plan/decomposition.md`, absorption-map touchup,
  and hugit CHANGELOG entry — all in the same campaign. (runner-transfer-campaign)

- feat(runner): **WP-R4④ — receive fence enforcement (materialize/enforce) +
  X4 oracle (runner-transfer campaign)**. The fence's runner-side half arrives
  from hugit-fence: `materialize` (sparse hydrate by path-set — sparse
  materialization IS the fence; SHA-256 content digests, new `sha2 =0.10.9`
  workspace pin) and `enforce` (in/out classifier + box-backed ENOENT probe),
  WITH the container-escape red-team harness (`redteam`: six vectors incl.
  the load-bearing fence-materialized-escape that would go RED under a no-op
  classifier, plus its hermetic FakeFsBox twin in the bare gate) and its
  box-gated acceptance (`tests/acceptance_redteam.rs`, c5b item ⑤) — moved
  rather than re-pointed so every red-team assertion keeps driving the REAL
  classifier in-process (relocated, never weakened). The WP-X4 supply-chain
  oracle arrives too (`x4::pin`: content-pinning + verify-before-spawn
  fail-closed ordering over the LIVE spawn surface, `tests/acceptance_x4.rs`
  with the hermetic ordering proof in the bare gate; item ② retargeted to
  THIS workspace's pinned lockfile/CI — same invariant, honest home). All
  env-gated lanes preserved exactly (`HUGIT_RUNNER_HOST`: FAIL-not-skip when
  set, short-circuit when unset); `tests/acceptance_c5a.rs` moved with its
  deterministic allow-all-rejection lane. Every file carries a provenance
  header citing hugit @ 69e28e5 (removed hugit-side by WP-R4②). The secrets
  broker (C5b items ②③④⑥) stays hugit-side with the forge; the seam is the
  wire contract. Full gate green: fmt · clippy `-D warnings --locked` · test
  `--locked` · `cargo deny check` · `cargo audit --deny warnings`.

- feat(runner): **WP-R2 — transplant the execution core (runner-transfer
  campaign)**. Moved `hugit/crates/hugit-runner` → `crates/corelink-runner`
  with **zero behavioral change**: every module transplanted intact (`lease`,
  `isolation`, `teardown`, `pin`, `boot/`, `concurrency/`, `expiry/`,
  `recovery/`, `shim/{broker,executor,parser,report,subset}`, `ws/`, `lib`) —
  **minus** the F2 envelope-capture files (`src/envelope/`,
  `tests/acceptance_f2.rs`), which stay in hugit (relocate to
  `hugit-ledger::envelope` in R4 per R0). Every moved file carries a provenance
  header citing hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1.
  Imports retargeted `hugit_contracts::{RunnerLease, RunnerState,
  FenceManifest}` → `corelink_runners_contracts::…` (the R0-frozen triplet, the
  runner's whole hugit-contracts surface — nothing else needed) and the crate
  self-path `hugit_runner::` → `corelink_runner::`. Acceptance suites
  C2a/C2b/C3/C9/E4 + `hermetic_supply_chain` moved unmodified except imports +
  header; C9's box-lane behavior preserved exactly (FAIL on unreachable box
  when `HUGIT_RUNNER_HOST` set; SKIP-return when unset). Crate deps trimmed to
  the scout budget: `corelink-runners-contracts` + `anyhow =1.0.102` (new
  workspace pin) + dev `serde_json =1.0.150`; no `hugit-ledger`/`sha2`/`hex`
  (those left with the envelope). Full gate green: fmt · clippy `-D warnings
  --locked` · test `--locked` · `cargo deny check` · `cargo audit --deny
  warnings`. Plan: hugit `docs/plan/2026-06-10-runner-transfer-campaign.md` §3
  (WP-R2) + R0 FREEZE.

- feat(contracts): **WP-R1b — wire-contract types + conformance vectors
  (runner-transfer campaign)**. Transcribed `RunnerLease`, `RunnerState`,
  `FenceManifest`, and closure type `MaterializedEntry` into
  `corelink-runners-contracts` from hugit-contracts @
  7c2f1e64bc1ba46d4941dc3e5b4a6247c21b0ec0 — same fields, same serde
  attributes (`deny_unknown_fields` etc.), same doc comments, plus a
  provenance note per type.  Conformance vectors (`RunnerLease.json`,
  `FenceManifest.json`) committed byte-identical to hugit under
  `conformance/`, with `conformance/manifest.sha256` tying both repos to the
  same digests (A3 criterion). Golden round-trip tests pin every vector
  byte-exact (4 golden + 1 manifest-coverage = 5 total).  Added
  workspace-level exact-pinned deps: `serde =1.0.228`, `serde_json =1.0.150`,
  `schemars =1.2.1` (all matching hugit's versions). Full gate green: fmt ·
  clippy `-D warnings --locked` · test `--locked` · `cargo deny check` ·
  `cargo audit --deny warnings`.

- feat(workspace): **WP-R1a — workspace foundation (runner-transfer campaign,
  skeleton half)**. Cargo workspace with one member crate,
  `corelink-runners-contracts` — a placeholder lib (real wire-contract types
  arrive in R1b after the R0 transcription freeze) with a trivial test so the
  gate exercises something from day 1. Toolchain pinned to 1.96.0
  (`rust-toolchain.toml`, copied verbatim from hugit), `deny.toml` with the
  HuGR house policy (crates.io only · multiple-versions deny · no skips — the
  empty workspace needs none), CI + DCO workflows on the self-hosted fleet
  (`[self-hosted, mac, corelink-builder]`, never GitHub-hosted) running the
  full gate: fmt · clippy `--workspace --all-targets --locked -D warnings` ·
  test `--workspace --locked` · `cargo deny check` · `cargo audit --deny
  warnings`. Plan: hugit `docs/plan/2026-06-10-runner-transfer-campaign.md` §3.
