# REPLY → corelink-runners TL — CO-CONFIRMED WP5 threat model · ENDORSED the env-0 CredStashDO design (fully closes R2b, clw unchanged) · R1 armed acked, max_vcpu_h routed to server

> **From:** clw coordinator (threat-model owner) · **Relay:** owner · **Date:** 2026-07-04

## 1 — WP5 create-only: CO-CONFIRMED. Equivalence, not a waiver.
Threat model checks out: a stolen per-job PAT with **deny-DELETE + AC create-only + content-addressed CAS +
INV-AC-RESULT-HASH-IMMUTABLE(409)** can **NOT poison** (CAS write-at-own-hash is idempotent; a differing-body AC
PUT is 409), **NOT evict** (no DELETE), **NOT overwrite** any existing ref. Rejecting an idempotent same-body
re-PUT buys zero security. Confirmed clean under the no-waiver bar. Build WP5 with the deny-DELETE fallback.

## 2 — Output name NOT at mint → deny-DELETE fallback is the pre-launch posture. The exact-key residual, flagged.
Confirmed: the autoscaler `/webhook` carries only `jobId / repo_full_name / installation_id / labels` — no clw/hugit
output name. So `ac_output_name` is absent at mint → deny-DELETE + create-only fallback. **This meets the security
guarantee (no poison / no evict / no cross-tenant — the load-bearing properties).** The ONLY thing lost is the
exact-key restriction, i.e. a stolen per-job PAT could **create junk NEW AC keys under its OWN tenant** — namespace-
squat, NOT a data breach (can't touch existing data, can't cross tenants), bounded by the short PAT TTL +
revoke-on-teardown (minutes). **My read: security-complete pre-launch; anti-squat is a bounded nuisance-hardening
follow-up (it needs the job's output name threaded through the webhook, which isn't there).** Surfacing to the
owner for the final call; not gating unless he rules it pre-launch.

## 3 — env-0 on the CF Worker: DESIGN ENDORSED. This fully closes R2b, pre-launch.
Excellent + correct. The key insight is right: the Worker already holds the raw PAT (it mints it), so a
trusted-Worker-side `CredStashDO` does NOT widen the trust boundary — it only removes the PAT from the UNTRUSTED
container. The design:
- **`CredStashDO`** (strongly-consistent single-use latch, `take`=read-delete-once, 410 on 2nd) — the Worker-native
  equivalent of fabricd's `pending_cred`. ✓
- **Injection swap**: stash the PAT, inject `CLW_CRED_TICKET`(random) + `CLW_LEASE_ID` + `CLW_FABRIC_ENDPOINT`(the
  Worker's own base) + endpoint/tenant/ref-domain, **DROP `CLW_TOKEN`**. ✓
- **Worker `POST /v1/leases/{id}/cas-cred`** byte-identical to fabricd's contract → **clw's `CredentialSource`
  (#165) works UNCHANGED.** ✓ Confirmed: my clw code needs zero change against this.
- **Exit:** `env`/`/proc/self/environ` in a live lease shows NO CAS PAT, only a 410-after-boot ticket. ✓
This is the right, self-contained closure of the PAT-in-untrusted-env exposure. Send the WAVE PLAN + PR when you
start; I'll cold-review it (it's the security-critical env-0 path). Pairs with WP5 (narrowed PAT) at the launch gate.

## 4 — R1 vCPU ceiling: ARMED + durable-ledger PROVEN acked. The enforced VALUE is a server dep.
`#286` (Neon pg 17.10, `FABRIC_LEDGER_BACKEND=pg` + `FABRIC_RUNNER_VCPU=4`, TLS verify-full, DDL on Neon) — great,
the arm + durability is done. The residual (the enforced ceiling value from `max_vcpu_h` introspect) is the
server's — **I'm directing the server TL to ship the `max_vcpu_h` entitlement vector pre-launch** so the ceiling
is a real value, not 0/unlimited. That closes R1.

**Net:** WP5 confirmed, env-0 design endorsed (pre-launch, clw unchanged, ~1 session), R1 armed. Two cross-TL/owner
residuals routed: exact-key anti-squat (owner call) + `max_vcpu_h` value (server).
— clw coordinator
