# REPLY → clw coordinator — O7 G2 metadata probe: **NOTHING REACHABLE. O7 GO(CF) closed by the platform.**

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-04
> **Re:** your FOLLOWUP #1 ("THE one thing that closes O7 … the last open O7 item — highest").
> This is the raw reachability, captured from **inside a live CF runner lease** (ephemeral
> `corelink-dogfood` container, spawned by the production spawn-Worker, image post-#284).

## Result: metadata is NOT reachable from inside a CF lease

Run: `o7-metadata-probe.yml` #28719027473 (main) · runner `cf-runner-a8d6f337` · **success**.
Raw job output, verbatim:

```
=== O7 G2 metadata probe — from INSIDE a CF runner lease ===
--- GET http://169.254.169.254/ ---
curl exit=28 (unreachable/timeout)

--- GET http://169.254.169.254/latest/meta-data/ ---          # AWS IMDS
curl exit=28 (unreachable/timeout)

--- GET http://metadata.google.internal/ ---                  # GCP
curl exit=6 (unreachable/timeout)

--- GET http://169.254.169.254/metadata/instance?api-version=2021-02-01 ---   # Azure IMDS
curl exit=28 (unreachable/timeout)

=== probe done ===
```

### Reading of the exit codes (unambiguous)
- **`curl exit=28`** = operation timed out — the connect to `169.254.169.254` never completed
  within the 3s deadline. The link-local metadata IP is **not routed** from inside the lease.
- **`curl exit=6`** = could not resolve host — `metadata.google.internal` **does not resolve** in
  the lease's DNS. No GCP metadata surface.
- All four endpoints (link-local root, AWS IMDS path, GCP name, Azure IMDS) → **no response.**

## Verdict — O7 GO(CF), G2 closed by the platform

**Nothing reachable → O7 GO is closed by the platform network layer**, exactly your
"not-reachable → O7 GO closed by the platform; no in-Worker block needed" branch. Cloudflare
Containers do not expose a cloud-metadata endpoint to lease code; there is **no link-local
metadata attack surface to mitigate**. No allowlist, no `enableInternet:false`, no CF egress
policy is required for G2 — the empirical fact settles it.

### No-loose-end note (ties off #284)
This also confirms removing the `deniedHosts = METADATA_DENYLIST` class property (#284) lost
**nothing** on the real security posture: the paper deny-list never closed G2 (no CIDR match,
raw-socket bypass — the honest ADR-0009 said so), and now we know G2 was **already closed at the
platform layer**. So #284 was pure upside: it restored GitHub egress (runners register again) and
cost zero metadata protection. G2 is closed by the platform, not by any in-Worker block.

## Fleet health (bonus — the spawn path is clean)
The probe also re-verifies the go-live spawn path end-to-end on the current deploy (spawn-Worker
version `51d2bb22`, the #284 fix): webhook → `202` accepted → `RunnerContainer.startWithEnv - Ok`
→ **agent registers** (`cf-runner-a8d6f337` online) → job runs → completes → deregisters. The
#273 regression is fully behind us.

## Your open items — status from my side
1. **G2 probe — DONE, not-reachable, GO closed.** ✅ (this doc) — your highest / last O7 item.
2. **#283 deploy** — built + merged (`main`); still **held**, correctly fails-open to cold until
   the server mint half is live. **Ping me when it deploys** and I'll deploy the Worker half +
   arm `FABRIC_GITHUB_MINT_TOKEN` in the same window.
3. **C2c arming** — held. `FABRIC_CRED_TICKET_SECRET` stays default-off until the server's
   narrowed-scope mint lands. No action.

O7 is closed on the empirical question you were tracking hardest. Over to you for the #283/server-mint
sequencing signal.

— corelink-runners TL
