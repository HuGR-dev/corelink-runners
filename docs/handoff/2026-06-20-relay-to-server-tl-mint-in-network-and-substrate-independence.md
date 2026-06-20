# Relay → CoreLink Server TL — D-9 mint in-network reachability + substrate-independence

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL · **Relay:** owner (gustavo@humangr.com)
> **Date:** 2026-06-20 · **Status:** Confirmations + one deploy nudge.
> **Context:** ADR-0008 (this repo) — Cloudflare Containers is now the **default** compute substrate
> (Northflank fallback). Compute moves onto Cloudflare; the control plane (D1 entitlement, D-9 mint) is
> already Cloudflare. I need two confirmations so the substrate swap stays a no-op for your seams.

## (1) D-9 mint — in-network reachable from the Cloudflare runner?

D-9 (per-job CAS-PAT mint) is already a Cloudflare Worker. Once the runner runs in a Cloudflare
Container in the same account/network:
- Can the runner / spawn-Worker reach D-9 **in-network** (worker-to-worker / service binding), instead
  of over the public internet? That would make the mint hop zero-egress + low-latency too — same win as
  the R2 co-location.
- Does the D-9 internal-auth contract (`x-corelink-internal-auth`, the `CORELINK_PAT_MINT_AUTH_KEY` the
  runner holds) stay identical, or is there a CF-native service-binding auth you'd prefer on the
  in-network path? (My `HttpCasPatMint` client is transport-generic, so either is a small adapter.)
- **Deploy nudge:** the D-9 **prod-Worker deploy** was still pending (shipped #305/#307). With compute
  moving to Cloudflare, this deploy is now on the critical path for the live moat — flag the ETA.

## (2) Substrate-independence of entitlement / auth — confirm it's a no-op

The entitlement lookup (`runners_entitlement` via introspect, `FABRIC_AUTH_BACKEND=corelink`) and the
dogfood tenant row (`ee30f7ba`, 80/600, LIVE) are **substrate-independent** — they gate admit by tenant,
not by where the box runs. Please confirm: moving the compute backend Northflank→Cloudflare changes
**nothing** on your side (same introspect, same entitlement, same `max_vcpu_h` sequencing). I believe
it's a clean no-op, but confirming so we don't discover a coupling at flip.

## What's ready on my side

- Mint env-wiring (WP-8a) is in `main`, default-off; the runner-side `HttpCasPatMint` is built + tested.
- Cloudflare backend (`CloudflareEngine`) + spawn-Worker contract + Worker skeleton built, default-off.
- The flip stays config-only on the runner side once (1) the mint endpoint (in-network or public) and
  (2) the substrate are wired.

Just need the two confirmations + the D-9 deploy ETA. Ping via owner. — CoreLink Runners TL
