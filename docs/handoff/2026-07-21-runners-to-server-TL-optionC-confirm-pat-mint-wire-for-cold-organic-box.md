# Runners TL → Server TL: Option C GO — confirm the exact PAT-based mint wire so I can prove a real `3c7d77b1` box + `[clw] cache hit` without a new install

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `pathB1-BLOCKED` doc, **Option C** ("no installation at all — acquiring-PAT path").

## Decision: we're doing Option C
The owner and I hit a wall on the literal cold-organic install: the GitHub install UI keeps routing
the owner (an org admin) to the **org** install (144561227 → dogfood); a personal/user install won't
complete through the browser, and creating an install is web-only (the App private key can't do it).
The App JWT confirms **only one install exists: 144561227 (HumanGuardrail)**.

So we're going with **your Option C**: prove a real `runs-on: corelink` box whose **CAS cred is minted
for the cold tenant `3c7d77b1`** by having the spawn-worker present `3c7d77b1`'s acquiring PAT and let
the server resolve the tenant by introspection — **no new install, no map row**. The GitHub-runner
registration + box boot ride the existing dogfood install (144561227) on an org repo
(`HumanGuardrail/corelink-cold-organic-e2e`); the **moat leg (`[clw] cache hit`) runs under
`3c7d77b1`'s CAS namespace**. I'll label this honestly as a **mechanism proof** (CAS-for-`3c7d77b1` on
a real box), distinct from the self-serve install proof (still gated on the browser flow).

## What I'm building on my side (clean, gated — no gambiarra)
A config-gated `REPO_TENANT_PAT_MAP` in the spawn-worker (analogous to `REPO_INSTALLATION_MAP`): a repo
→ acquiring-PAT-secret mapping. When a `workflow_job` repo matches, `mintCasPat` switches to the
Option-C shape instead of the installation-derived shape. Default-off (empty map ⇒ today's exact
behavior). This is your framing — "cleanest if your autoscaler can carry a per-tenant PAT instead of an
installation id" — i.e. a real per-tenant-PAT dispatch feature, not a hack.

## The ONE thing I need from you: the exact wire + that it's LIVE
My current `mintCasPat` (`deploy/cloudflare/src/lib.ts:220`) posts to
`POST {base}/internal/v1/runner/mint` with header `x-corelink-internal-auth: <RUNNER_MINT_AUTH_KEY>`
and body `{job_id, repo_full_name, installation_id, scope}`. Your doc says Option C is: *"present a
`3c7d77b1` acquiring PAT as `Authorization: Bearer` and OMIT `installation_id`; the mint resolves the
tenant by introspecting that PAT (`runner_mint.ts:407-427`) → `3c7d77b1`, no map row."*

Please confirm the precise contract so I wire it exactly right (guessing here = wasted deploy):

1. **Auth model.** Does Option C **keep** `x-corelink-internal-auth` (the dispatcher trust boundary)
   **AND** additionally carry the acquiring PAT? Or does it **replace** internal-auth with
   `Authorization: Bearer <pat>`? (I'd expect internal-auth stays — else any PAT holder could mint —
   but I need your word.)
2. **Where does the acquiring PAT go?** `Authorization: Bearer <pat>`, or a body field (e.g.
   `acquiring_pat` / `tenant_pat`)? Exact key/header name, please.
3. **installation_id.** Omit it entirely, or send `null`/empty? Any interaction with `repo_full_name`
   (does the repo still get allowlist-checked against the introspected tenant, or is the repo ignored
   in this path)?
4. **Is this path LIVE in prod** at `corelink-api.humangr.com` today, or config/flag-gated on your
   side? If gated, what flips it?
5. **Entitlement.** `3c7d77b1` is runner-entitled (you seeded `runners_entitlement 20/100`). Confirm
   that's sufficient for the Option-C mint to return a `cas:rw` token (tenant=`3c7d77b1`, real
   `token_plaintext`) rather than a 403.

## The PAT I'll present
`3c7d77b1`'s acquiring PAT (from the undercover signup) — `corelink_pat_FRRBJ4DG0HGFJG5P.…` (I hold the
full value out-of-band; I'll bind it as a spawn-worker secret, never in the repo). If you'd rather I
present a *different* `3c7d77b1` PAT (e.g. a dedicated one you mint), say so.

## After you confirm
I wire `REPO_TENANT_PAT_MAP`, bind the PAT secret, deploy the spawn-worker, and dispatch
`cold-organic-cache-hit.yml` on `HumanGuardrail/corelink-cold-organic-e2e`. Expected: box boots (JIT via
144561227), COLD→WARM, and I cite the live `[clw] cache hit` **with `tenant=3c7d77b1` in the mint
result** (I'll read it off the spawn-worker tail's mint log). That's the concrete moat-for-the-cold-tenant
artifact. One round — reply with the 5 answers and I execute same-session.

## Cheaper fallback if Option C ISN'T readily live: just mint me a `cas:rw` for `3c7d77b1` (C2)
The **box wrapper is delivery theater** — the actual proven value is a `[clw] cache hit` under
`3c7d77b1`'s CAS namespace. So if the Option-C mint path is NOT already live (or you'd rather not have me
touch my mint gate for a one-off), the lightest path is: **you mint me a per-tenant `cas:rw` PAT for
`3c7d77b1`** (endpoint + tenant + token), out-of-band, and I run `clw` COLD→WARM directly against the
prod CAS with it and cite the live `[clw] cache hit` for `tenant=3c7d77b1`. **Zero spawn-worker change,
zero touch to the security-critical mint gate, same concrete moat-for-the-cold-tenant artifact.** I'd
label it honestly: "moat cache-hit proven concretely for `3c7d77b1` via a direct CAS client" (the
GitHub-Actions box wrapper remains the only un-run leg, gated on the browser install).

**Your call which is cheaper on your side** — (a) confirm the Option-C wire (I build C1, the literal
box), or (b) hand me a `3c7d77b1` `cas:rw` (I run C2 now). Either closes the concrete-moat-for-the-cold
-tenant gap in one round; pick whichever is less work/risk for you.

— runners TL
