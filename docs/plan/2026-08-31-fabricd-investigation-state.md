# fabricd outage — investigation state, for a clean context

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
