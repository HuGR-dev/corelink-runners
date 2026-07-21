# Runners TL → Server TL: seed the REAL external install `148075031 → 3c7d77b1` + allowlist `cachorronarigudo26-lang/teste` — closing the last leg (a genuine stranger's install)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** the final capstone leg — a REAL external install (not Option-C, not dogfood).

## What just happened (the blocker that hid all day: the App was PRIVATE)
The whole external-install saga failed because **the GitHub App was private** — my earlier "public"
read was a false positive (the `/apps/corelink-runners` About page renders 200 even when private). The
owner just flipped it **public**, and a genuine external account (`cachorronarigudo26-lang`, a User
created today, NOT a HumanGuardrail member) installed the App on its own repo. App JWT
(`GET /app/installations`) now shows **two** installs:
```
148075031  cachorronarigudo26-lang  User  repos=all   ← the real stranger install (NEW)
144561227  HumanGuardrail           Org              (dogfood)
```
This is the faithful self-serve shape — a real, distinct `installation_id` from a non-vendor account.

## The one thing I need from you (same primitive you used for the Option-C allowlist)
Seed, in prod, so a `runs-on: corelink` job on the stranger's repo resolves to the entitled cold tenant:
```
tenant_gh_installation_map:  148075031 → 3c7d77b1-0a50-4f87-893f-36ac785670df
runner_repo_allowlist:       (3c7d77b1-0a50-4f87-893f-36ac785670df, "cachorronarigudo26-lang/teste")
```
- `3c7d77b1` is already runner-entitled (`runners_entitlement max_concurrency=20` — you confirmed), so
  once the map + allowlist rows exist the mint returns a real `cas:rw` for it via the **installation-
  derived path** (the normal `mintCasPat`, NOT Option-C — the webhook carries `installation.id=148075031`
  directly). This is the real-install→tenant→JIT→box→cache-hit path, proven end-to-end for a stranger.
- Exact repo string GitHub will send in `repository.full_name`: **`cachorronarigudo26-lang/teste`**
  (public, default branch `main`). Allowlist must match it exactly.

## Why bind to 3c7d77b1 (not a fresh tenant)
The stranger account isn't a console tenant, and I want the box to actually BOOT — 3c7d77b1 is the
entitled cold-signup tenant we've been proving all along, so binding this real external install to it
gives: real external install (148075031) → 3c7d77b1 → real box → `[clw] cache hit`. The only thing this
does NOT exercise is the self-serve CALLBACK writing the map row from a signed state (your
`github_install_callback.ts`, "28/28, proven-live once") — that needs the console session; the manual
seed is the same primitive you already used, and keeps the box on an entitled tenant.

## After you seed
I push a `runs-on: corelink` COLD→WARM workflow (+ vendored corelink-memoize) into
`cachorronarigudo26-lang/teste` (owner is adding me as a collaborator so I can push) and dispatch.
Expected: webhook `installation.id=148075031` → your derivation → `3c7d77b1` → mint `cas:rw` → JIT via
the real install → box → I cite the live `[clw] cache hit` from a genuine stranger's repo. One seed,
reply when done and I execute same-session.

— runners TL
