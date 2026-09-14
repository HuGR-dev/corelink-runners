# Owner runbook — arm the two fabricd ops keys (obs + admin)

**Date:** 2026-07-09 · **For:** owner (you) · **From:** runners TL
**Why:** a live-verify of the fabricd Worker found **two operator secrets were never set**.
Both gate default-OFF surfaces (fail-closed 404/disabled until armed), so nothing is broken —
but two operability powers are dark until you set a value. This is a 2-command action.

---

## The finding (verified live, `wrangler secret list`)

| Secret | What it unlocks | State today |
|---|---|---|
| `FABRIC_OBSERVABILITY_KEY` | reads on `GET /internal/v1/status` (version, uptime, ledger durability, shards, **the new golden-signal counters**) + `GET /internal/v1/occupancy` (slot occupancy) | I armed a throwaway value to PROVE it works (status went `404 → 401` live). You should set **your own** value so you can actually read the surface. |
| `FABRIC_ADMIN_KEY` | operator `POST /internal/v1/admin/tenants/{t}/suspend` + `/unsuspend` — the **AUP1 abuse response** (kill an abusive tenant's live leases + block new acquires) | **NOT set** — the suspend power is unusable live until you set it. Arm before wide go-live. |

Neither is a customer credential. They are operator read/act keys for a non-tenant surface.
Values are yours to pick and keep (a long random string; a password manager entry).

---

## The 2 commands (run each with the `!` prefix in this session)

Type these in the Claude Code prompt — the `!` runs them in this session so I see the result.
Replace `<PASTE-A-LONG-RANDOM-STRING>` with a value **you keep** (e.g. `openssl rand -hex 32`):

```
! cd ~/Documents/HuGR/corelink-runners/deploy/cloudflare-fabricd && printf '%s' '<PASTE-A-LONG-RANDOM-STRING>' | npx wrangler secret put FABRIC_OBSERVABILITY_KEY
```
```
! cd ~/Documents/HuGR/corelink-runners/deploy/cloudflare-fabricd && printf '%s' '<PASTE-A-DIFFERENT-LONG-RANDOM-STRING>' | npx wrangler secret put FABRIC_ADMIN_KEY
```

Keep the two values in your password manager. Do **not** paste them into a normal chat message
(only the `!`-prefixed command, which is treated as a local command).

---

## ⚠️ The one gotcha: secrets take effect on the NEXT container rollout

The fabricd control plane runs as **one long-lived singleton container** that reads its env
**only at boot**. A `wrangler secret put` updates the Worker but does **not** restart the
container — and a plain `wrangler deploy` only restarts it when the **image digest changes**.

So after you set the secrets, they are staged but inert until the container next re-boots.
**Two ways to activate:**
1. **Free ride (recommended):** they activate automatically on my **next fabricd image
   deploy** (the next code wave that ships a new binary). Just tell me you've set them and
   I'll confirm they're live on that rollout.
2. **Immediate:** tell me "activate the keys now" and I'll trigger a no-op image rollout so
   the container re-boots with them today.

Once active: `FABRIC_OBSERVABILITY_KEY` makes `GET /internal/v1/status` return `200` (with
the counters JSON) for a request carrying `X-Corelink-Internal-Auth: <your value>`, and the
suspend/unsuspend endpoints accept `<your admin value>`.

---

## What this is NOT

- Not a blocker for the runner code (that's shipped + green).
- Not the first-external-customer gate — that's **console/onboarding** (corelink-server,
  ADR-0002) + the **public GitHub App install page**. See the server-TL relay
  (`docs/handoff/2026-07-09-RELAY-to-server-tl-console-onboarding-golive-gate.md`).
