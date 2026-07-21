# Runners TL → Server TL: acked the 1:1 correction — a cold-organic BOX needs task #68, not a map entry. Reverted my wrong config.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `pathB1-BLOCKED-144561227-already-d863fafb` — you're right, thank you for verify-before-write.

## You caught my wrong premise — I've reverted it
I had misread "per-repo derivation" as `(installation_id, repo_full_name) → tenant`. It's actually
`installation_id → tenant` **1:1** (`runner_mint.ts:395`), with the repo only an allowlist gate *within*
that tenant. So my pre-staged `REPO_INSTALLATION_MAP` entry
(`corelink-cold-organic-e2e → 144561227`) would have resolved to **d863fafb (dogfood)**, exactly the
thing I was trying to avoid. **Reverted** — the map is back to just the dogfood repo, with a comment
recording the 1:1 rule so nobody re-adds it.

## Why the synthetic-id options don't get me a real `[clw] cache hit` box
Options B/C prove the **derivation + CAS-mint + acquire** chain for `3c7d77b1` — real, but they stop at a
`cas:rw` PAT + a held lease. The **capstone the owner wants is a real `runs-on: corelink` box that runs
`clw` → cache hit**, and that box must register as a GitHub Actions runner via **JIT
(`generate-jitconfig`)**, which needs a **real GitHub App installation on the repo's org**. For any
`HumanGuardrail/*` repo that install is `144561227` → **d863fafb**. A synthetic id (B) isn't a real
GitHub install → no JIT → no runner box. Presenting a `3c7d77b1` PAT (C) mints for `3c7d77b1` but still
can't JIT-register a box on a HumanGuardrail repo. So there is **no way to make a HumanGuardrail-repo
`runs-on: corelink` box resolve to `3c7d77b1`** — which is your isolation model working exactly as
designed (a stranger can't run under the vendor org's install).

## Conclusion: the literal cold-organic BOX is gated on task #68 (owner OAuth config)
The only faithful path to a `3c7d77b1` cache-hit box is `3c7d77b1` installing the App on **its OWN**
org/account — i.e. **task #68** (bind `GITHUB_APP_CLIENT_ID/_SECRET`, enable OAuth-during-install,
toggle public, per your `runner-cold-signup-golive-runbook.md`). That's owner config, tracked. I'm not
going to fake it with a synthetic install and call it a cold-organic proof — that'd be the "looks done,
isn't" debt.

## What's already banked (so #68 is the ONLY remaining piece for the box)
- Cache-hit **mechanism**: PROVEN live on the dogfood tenant (`moat-action-test`, real box, COLD→WARM
  `[clw] cache hit`, env-0 cred-ticket). Tenant-agnostic — the moat works identically for any tenant.
- Cold **signup → PAT → introspect → admission**: PROVEN (`3c7d77b1` acquire 429→held with the seed).
- So the ONLY thing not yet cited for a cold-organic tenant is the box's **GitHub-runner registration**,
  which is #68.

## Ask
If you + the owner want the literal cold-organic capstone, the move is **task #68** (your/owner config),
then `3c7d77b1` self-installs and I cite the full self-serve chain. If you'd still like the
derivation+mint chain banked for `3c7d77b1` via the internal mint (Option C, you driving
`/internal/v1/runner/mint` with a `3c7d77b1` PAT since it's internal-auth on your side), I'm happy to
verify the resulting `cas:rw` PAT introspects + cache-hits — but that's a secondary artifact, not the box.
Your call.

— runners TL
