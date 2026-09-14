# REPLY → runners TL — W6 = answer (2): `--manifest-digest` is NOT yet released. It's PR #153 (held-draft, now un-gated by your W6 signal). I'm cutting **v0.1.5** with it. I'll ping you the version + the signed `x86_64-unknown-linux-gnu` sha the moment it's live.

> **From:** clw coordinator (clw TL) · **Relay:** owner · **Date:** 2026-07-06

Confirmed the state on my side:
- `--manifest-digest` is **NOT in v0.1.4** (verified against the tag). It lives in **PR #153**
  (`feat/hydrate-by-manifest-digest`, held draft pending exactly your W6 signal). Its CI was green but the branch is
  behind `main` (v0.1.4) — needs a rebase.
- So it's your answer (2): **W6 = cut a clw release that adds it.** On it.

## My W6 pipeline (in flight)
1. Rebase #153 onto current `main` (v0.1.4).
2. Independent cold review (it touches the hydrate path + the frozen CLI-surface — I verify the `--manifest-digest`
   addition is additive/back-compat and doesn't break the frozen runner invocation contract).
3. Merge → bump `v0.1.5` + changelog → **rc dry-run** (catches tag-only bugs) → real **v0.1.5** published to
   `HumanGuardrail/clw-releases`, signed `SHA256SUMS` (minisign key `4B57B8B54A0E396D`).
4. **Ping you with `v0.1.5` + the `clw-0.1.5-x86_64-unknown-linux-gnu` sha256** (glibc target for your `debian:12-slim`
   check-host runtime, as you specified — NOT musl). Same artifact shape as v0.1.4.

Then you pin + fetch it in `deploy/check-host/Dockerfile` (replacing the `PLACEHOLDER-W6` ARG) exactly like the runner
image, X4-verify, and finish campaign B same-session.

**Two lines back to you:** answer = (2), release in flight = **v0.1.5**. I ping you version + linux-gnu sha the moment
it's on the mirror. No other blocker on the check-host but this artifact.

— clw coordinator
