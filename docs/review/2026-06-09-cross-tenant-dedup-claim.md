# Review — "cross-tenant dedup, live in production" overstates the cache's GA state

- **Date:** 2026-06-09 · found during a full-stack read (hugit-anchored session)
- **Status:** **RESOLVED 2026-06-09** — the corrected language landed in the
  whitepaper (§2 intro · §2 lever 3 · §5.1 diagram · §6 theorem ×2 · §11 M1)
  and the product brief (§2.3 · §6 · §7 · §8), and in hugit's whitepaper §5.2,
  all in the same revision pass; **no vision or principle change**. This note
  stays as the record + the rule below.
- **Severity:** docs-only today; becomes a real liability the moment the claim is
  quoted in public material (pricing page, pitch, sales deck) or read by a diligent
  design partner / investor / competitor, because it is falsifiable against
  CoreLink's own GA release notes.

## The inconsistency

This repo states the **cross-tenant dedup** lever as live. The cache's own GA
release notes state it as **post-GA roadmap**.

**What this repo claims** (all present tense / "live"):

| Doc | Claim |
|---|---|
| `docs/whitepaper/corelink-runners-v1.md` §2 (intro) | "CoreLink already operates the hard, defensible asset — a **content-addressed CAS + Action Cache with cross-tenant dedup**, live in production." |
| whitepaper §2 (lever 3) | "One tenant warming `tokio` or `node_modules` warms it for all. **The cache gets cheaper per job as more customers join**" |
| whitepaper §5.1 (diagram) | "CoreLink Cache — … (content-addressed, cross-tenant dedup) **✅ LIVE**" |
| whitepaper §6 (the theorem) | "…both of which *require* a content-addressed CAS/AC with cross-tenant dedup at scale. … **We already have it, live.**" |
| `docs/product/product.md` §2 (bet 3) | "a content-addressed CAS + Action Cache with cross-tenant dedup — **already exists and is live**" |
| product.md §6 (COGS) | "The warm working set is small and shared (**cross-tenant dedup of public deps**)." |
| product.md §7 (defensibility) | "cannot boot warm or memoize without a content-addressed CAS/AC **with cross-tenant dedup** at scale" |

**What corelink-server actually ships at GA**
(`../corelink-server/RELEASE-NOTES-v1.0.0-GA.md`, verified 2026-06-09):

> line 76–77: "**Intra-tenant deduplication** out of the box; cross-tenant dedup
> backlogged for post-GA."

> line 397 (known-limitations table, item 4): "Intra-tenant dedup is on by default
> at GA; cross-tenant dedup is a **privacy-sensitive feature gated on
> `CAP-DEDUP-CROSS-TENANT` design**"

## What is true today (so the correction does not overcorrect)

- The **CAS + Action Cache is live in production** (data plane deployed wave 32,
  2026-05-22). The asset exists; that part of the claim stands.
- **Intra-tenant dedup is on by default.** R2 zero-egress is real.
- **Levers 1 and 2 of the moat — cache-warm boot and memoized execution — do not
  depend on cross-tenant dedup.** They hold per tenant, today. The worked example's
  core (warm boot cuts minutes; memoized jobs never run) is unaffected.
- What is **not yet true** is lever 3's cross-tenant half: "one tenant warming a
  public dep warms it for all" and "COGS falls as customers join." That is
  **designed** (`CAP-DEDUP-CROSS-TENANT`), not **on**.

## Why it matters

1. The **moat theorem (§6)** closes on "We already have it, live" — and its
   strongest clause is the one not yet enabled. As written, the theorem is
   falsifiable by anyone who reads the cache's GA notes.
2. The **economics narrative** ("the moat deepens with scale", "network effect on
   the COGS itself") is a *future* property until the capability ships; stating it
   in the present tense invites a credibility hit precisely where the product is
   strongest.
3. **Downstream, same bug:** hugit's whitepaper §5.2 makes the identical bet
   explicit ("every tenant's public-deterministic artifacts … warm the cache for
   all tenants. Customer #500 arrives to a workspace that is already ~40–70% hot").
   That fix belongs in `../hugit` — raise with the owner / hugit techlead; do not
   edit from here.

## Proposed corrected language (one canonical sentence, reuse everywhere)

> CoreLink already operates the hard, defensible asset — a content-addressed
> CAS + Action Cache, live in production. Dedup is intra-tenant today;
> **cross-tenant dedup of public-deterministic artifacts is designed in
> (`CAP-DEDUP-CROSS-TENANT`) and turns on post-GA** — from that point the cache
> gets cheaper per job as customers join.

And the theorem restated **without the overclaim** — it survives intact:

> A new entrant lacks even the live CAS/AC, the tenancy boundary, and the privacy
> machinery that cross-tenant dedup requires. We have the asset live and the lever
> staged. ("Live asset + staged lever", never "all live.")

## Where to fix (next doc revision — do not hotfix piecemeal)

| Doc | Spots |
|---|---|
| `docs/whitepaper/corelink-runners-v1.md` | §2 intro sentence · §2 lever 3 (tense) · §5.1 diagram caption ("✅ LIVE") · §6 theorem ("We already have it, live") |
| `docs/product/product.md` | §2 bet 3 · §6 cache-I/O bullet · §7 defensibility line |
| `../hugit/docs/whitepaper/hugit-v1.md` §5.2 | the "~40–70% hot" network-effect paragraph (**fix in the hugit repo**, owner/hugit-techlead) |

Unaffected (checked, no change needed): `CLAUDE.md` ("Cache — content-addressed
CAS + Action Cache (launch, live)" — true as written) · whitepaper Abstract ("no
vendor without a content-addressed cache at scale" — does not claim cross-tenant).

## Rule going forward

Any claim in this repo about the **production state** of CoreLink cites the
cache's GA release notes / live configuration at the time of writing — never
memory of the design. The vision is unchanged (the moat *does* deepen with scale
once the lever ships); only the tense was wrong.
