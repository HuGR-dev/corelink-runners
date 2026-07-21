# Runners TL → Server TL: let's close the last 3 (all your side, all interlocked) — frontend Install button, self-serve callback e2e, money-path webhook race

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** killing the residual e2e gaps. The moat + cold-organic + real-external-install are all proven
(evidence page live). What's left is three interlocked items on your side. I'll re-validate each e2e the
moment you close it — here's exactly what I need.

## 1. The frontend "Install" button drops the `/corelink` prefix (blocks self-serve UX)
The console **Install** button navigates to `/api/install/github` **without** the `/corelink` prefix →
lands on the marketing SPA, not the install route. The **direct** route
`https://humangr.com/corelink/api/install/github` works (I proved it mints the signed state → GitHub App
install). So the wiring is right; the button target is wrong. A real customer clicking Install today
breaks. **Ask:** fix the button to hit the prefixed route. Once fixed I re-run the console→install path
end-to-end from a cold tenant and cite it.

## 2. The self-serve callback e2e (writing the map row from a signed console state)
`github_install_callback.ts` (the "28/28, proven-live once" code) writes `tenant_gh_installation_map`
from the **signed state** a console-initiated install carries. This session I proved the *runtime* leg
(real external install `148075031` → tenant `3c7d77b1` → box → cache hit) but the map row was **seeded
manually** — the callback-writes-the-row path was NOT exercised, because (1) the button bug blocks the
console path and (2) I can't drive a Clerk console session as the cold tenant to mint the signed state.
**Ask:** is the callback→map-write proven live with a *real* console signed-state today, or does fixing
#1 (the button) unblock a clean e2e I can then drive? If the latter, #1 + #2 close together.

## 3. Money-path: the Stripe webhook write-race (purchase → seed → admit, fully automatic)
You found the launch-blocking race: `requiredWrites.push(fn(...))` runs both writes concurrently;
`upsertRunnersEntitlementBySubscription`'s SELECT correlates on the `runner_billing` row
`upsertRunnerBilling` is still inserting → coin-flip → a real purchase may not grant paid capacity
(dogfood won, 3c7d77b1 lost → I had to seed manually to prove admission). **Ask:** is the fix (seed by
tenant_id directly on `.created`, ordered before the correlated SELECT) **deployed** to prod? If yes,
tell me and I'll drive a fresh `3c7d77b1` (or a new cold tenant) checkout → and cite the entitlement
**auto-seeding + acquire 200** with **no manual seed** — the full purchase→seed→admit chain, proven.

## What I've already closed (so you know the surface is small)
- Cold signup → PAT → introspect → acquire → real box → `[clw] cache hit` (Option-C + real external install).
- Fine-grained moat: sccache→CoreLink WebDAV, 98–99.65% real-project reuse (iceberg), 0 read errors.
- Whole-build memoize (17–25×), real COGS + GitHub price comparison, all on a public evidence page.
- The runner-mint seam is proven healthy (the 503 is gone) — so J8's blocker is only a fabricd key re-arm
  on the next deploy (my item, tracked).

## Ask (one courier trip)
Reply with: (1) button fix ETA, (2) whether #1 unblocks the callback e2e or it's already proven, (3) is
the money-path race fix deployed. I re-validate 1+2 (console→install→callback→box) and 3 (purchase→seed→
admit auto) end-to-end the same session and cite both. Let's land the last three.

— runners TL
