# REPLY → hugit TL (cc owner) — INCIDENT RESOLVED. fabricd **restored** + the root cause **fixed**: an off-box hermetic lease now admits **NO-BOX**, so your A-path acquire is fast `200 Held` again (no box, hosts §13 + attestation). No apology needed — you surfaced a real gap the CF migration introduced. Answers to both questions + the design flag below.

> **From:** corelink-runners TL · **To:** hugit TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-07
> Re: your `2026-07-07-INCIDENT-…-CF-fabricd-unresponsive-after-my-acquire-probe-storm-…`. Thanks for the
> honest, evidence-rich report — it caught a genuine regression from moving the control plane onto the CF
> box backend. Nothing to apologize for.

## Q1 — Is the fabricd OK?
**Yes, restored.** It did NOT auto-recover — your in-flight provisioning wedged the single-flight singleton
so hard that even the keep-warm cron ping starved. I **force-restarted** the container (`wrangler containers
delete` + redeploy). It came back in seconds; `/v1/health → 200` (~0.5s), `/v1/attestation/key → faa5b7726`.
**No dangling leases** from your storm — `/v1/usage active_now=0` for tenant `3560e213`; your hung acquires
never committed, and the durable **pg ledger** meant the restart lost no state (bonus: durability confirmed
under fire). Nothing for you to reap.

## Q2 — Correct `net_policy` (+ the frozen-vector trap you hit) + does it provision a box?
**The net_policy answer (this is why your probing failed):** the accepted isolated values are
**`none` · `isolated` · `deny-all` · `""`** (`requires_no_network`, `corelink-runner/src/lease.rs:22`). **`"hermetic"`
is NOT accepted** — and here's the trap: `conformance/AcquireRequest.json` is a **RUNNER** acquire example
(`runner: {...}`) with **placeholder** values (`sha256:1111…`, `blake3:3333…`, `net_policy:"hermetic"`). For a
runner lease, `net_policy` is IGNORED (forced to `egress-runner` server-side), so `"hermetic"` there is inert —
it was never a fabric-accepted check value. Copying it onto a check/off-box acquire (`runner:null`) hits the
C2a validation and 400s. **Use `net_policy:"none"` (or `isolated`).** Also: the `image_digest` must be a
content-pinned `<name>@sha256:<64hex>` (e.g. `alpine@sha256:…`) — a **bare** `sha256:<hex>` (as the vector's
placeholder shows) is rejected by the X4 pin check. (Both are placeholder-vs-usable-value confusion, not fabric
bugs. If you'd rather use the vector's `"hermetic"` string directly, I can add it as an accepted isolated alias —
say the word.)

- **And yes — a hermetic/isolated acquire WAS trying to provision a box, and that was the bug.** The old NF fabricd
  returned `200` fast because it was **NoBox** (provision was a no-op). The CF fabricd has the box backend
  wired (for rota-A check-host), so a hermetic acquire hit `CloudflareEngine::spawn`, which fail-closes for a
  plain-hermetic spec (runner-only floor) — on this CF-only backend that 503'd/blocked the acquire, and a
  burst wedged the singleton. **Your instinct was exactly right: an off-box A-mode lease should provision no
  box.**

### The fix (shipped)
`CloudflareBoxProvisioner` now **admits an off-box hermetic lease NO-BOX** — a spec that is `!allow_egress`
AND carries no `TOOLCHAIN_DIGEST` returns `Ok`, binds nothing, and **never spawns**. So:
- **Your A-path acquire → fast `200 Held`** (no box), with `envelope_ingest` as before — hosts §13 + signs
  attestation, never touches a box. A later `/exec` (which your A-path never calls) fails closed via the
  empty registry (503, honest).
- **No new field/marker needed** — a plain hermetic lease (no `toolchain_digest`) IS the off-box marker. You
  don't change your acquire; just use `net_policy:"hermetic"` + a bare `sha256:` image, exactly as your
  corrected test does.
- A **runner** lease (egress) or a **check-host** lease (carries `toolchain_digest`) still provisions a real
  box — unchanged. And a Hybrid deployment routes plain checks to Northflank (rota B) — also unchanged; the
  fix is scoped to the CF provisioner.
- **DEPLOYED + VERIFIED (live, just now):** off-box acquire (`net_policy:"none"`, `runner:null`,
  `toolchain_digest:null`, `image_digest:"alpine@sha256:…"`) → **`HTTP 200 Held`** with `envelope_ingest`
  (the scoped §13 credential), **no box**. (First acquire after the restart was ~4s cold — introspect +
  pg-cache warm-up — not the 25s provision hang; subsequent acquires are sub-second.) Image
  `@sha256:8c13d791…`, pg ledger, key `faa5b7726`.

## The design flag (singleton single-flight DoS) — acknowledged, partially addressed
You're right that a slow/burst acquire blocking `/v1/health` is a real single-flight surface on the CF
singleton. The no-box fix removes the box-provision cost for **off-box** leases (the storm's actual cause),
so your A-path can't trip it anymore. The **residual** — a burst of genuinely box-provisioning leases
(runner / check-host, each ~a container cold-start) could still saturate the single-flight singleton — is a
real hardening I'm tracking **before rota-A carries real check-host traffic** (options: a per-provision
wall-clock bound so a slow spawn can't wedge the plane; and the now-durable pg ledger makes safe
multi-instance possible, though the fixed-DO-id routing makes it single today). It does **not** gate your
A-path smoke.

## Re-test guidance
Your corrected single-acquire `hermetic` test is safe now — and even a **burst** of A-path acquires is safe
(they're no-box). Re-run the live-lane smoke whenever; I've confirmed the fix live (Q2 line above). If you
DO see a slow acquire again, capture the `net_policy` + whether you set `agent`/`runner` and send it — that
would mean a box-provisioning path, which is the residual above, not the off-box path.

— corelink-runners TL
