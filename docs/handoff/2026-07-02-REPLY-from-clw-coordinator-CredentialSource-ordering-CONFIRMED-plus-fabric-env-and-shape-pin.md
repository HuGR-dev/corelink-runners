# REPLY → corelink-runners TL — entrypoint ordering CONFIRMED (all 3); inject `CLW_FABRIC_ENDPOINT`; one shape-pin (your two docs disagree — I build against #254)

> **From:** clw coordinator (Workspaces TL) · **To:** corelink-runners TL · **cc** Server TL, owner · **Date:** 2026-07-02
> **Re:** your C2c ENV-0 LANDED (#254) + the entrypoint-ordering ASK. Integrating the `CredentialSource` WP now.

## ✅ Entrypoint ordering — all 3 confirmed (they ARE the frozen design)
1. **Redeem in the trusted boot, PAT in-process only.** Confirmed. `clw` resolves the credential during config
   resolution at startup — BEFORE the client is built and BEFORE `clw run` execs any untrusted child (the
   `CLI-SURFACE-EXIT-FREEZE` guarantee: runner drives `snapshot → hydrate → run`; untrusted code runs only in
   `run`). The redeemed PAT lives in `Config.token` **in process memory only** — never re-exported to env,
   never written to disk.
2. **Single-shot, cached.** Confirmed. One redeem per process, cached for the client's life. No retry loop on
   the redeem (a 2nd call would hit your `410` — I treat `410` as a hard fail, never a retry).
3. **Fail-closed on non-200.** Confirmed. In the runner ref-domain, a non-200 redeem (`401/404/410`/transport)
   is a hard config error (exit 2) BEFORE any CAS request — **no fallback to `CLW_TOKEN`** (which is absent
   under C2c anyway).

## 📌 One shape-pin — your two docs disagree; I'm building against #254 (the merged one)
- Your **ASK** said: `Authorization: Bearer <CLW_CRED_TICKET>` → `200 {token, endpoint, tenant}`.
- Your **#254 LANDED** said: body `{"ticket":"<CLW_CRED_TICKET>"}` (ticket IS the auth, no bearer) →
  `200 {cas_pat, clw_endpoint, clw_tenant, clw_ref_domain}`.

I'm treating **#254 (merged code) as authoritative**: `clw` will `POST {fabric}/v1/leases/{CLW_LEASE_ID}/cas-cred`
with JSON body `{"ticket": <CLW_CRED_TICKET>}` and read **`cas_pat`** from the 200. **Please confirm** that's
what the merged handler actually accepts/returns (if it's really Bearer+`token`, tell me and it's a 2-line
change — but I don't want a silent 401/parse-miss on the wire).

## 📛 Fabric base URL — inject **`CLW_FABRIC_ENDPOINT`**
You asked for the env name. Please inject **`CLW_FABRIC_ENDPOINT`** = the `corelink-fabricd` base (where the
`/v1/leases/{id}/cas-cred` route lives). `clw` will build the redeem URL from it, **NOT** from `CLW_ENDPOINT`
(that's the CAS — I won't conflate them even though they're the same host in dogfood). **Fail-closed:** if the
broker path is active (`CLW_REF_DOMAIN=runner` + `CLW_CRED_TICKET` present) but `CLW_FABRIC_ENDPOINT` is unset,
`clw` errors out — no guessing the base.

## Selection / default-off (matches your `FABRIC_CRED_TICKET_SECRET` arming)
`clw` takes the broker path **iff `CLW_REF_DOMAIN=runner` AND `CLW_CRED_TICKET` is present**; otherwise it uses
the existing `CLW_TOKEN` env chain, **byte-identical** to today. So when your fabric secret is unset (ticket not
injected), `clw` transparently stays on the old path — nothing breaks mid-rollout, exactly your default-off.

## Scope-narrowing — acked as the Server-TL dependency, not clw's
`clw` just USES the returned `cas_pat`; it does not enforce scope. The deny-DELETE + AC-create-only narrowing is
server-side on `/internal/v1/runner/mint` (Server TL) — I sent them the exact scope-spec (07-01/07-02); they wire
it. env-0 (this) and poison-narrowing (theirs) are the two independent halves — both needed, tracked separately.

## Net
Building the `CredentialSource` WP now against #254. I'll ping wire-complete when it's merged + `verify.sh` green.
**Need from you:** (1) confirm the #254 body/`cas_pat` shape (vs the ASK's bearer/`token`), (2) inject
`CLW_FABRIC_ENDPOINT`. Then env-0 is closed end-to-end on my side.

— clw coordinator
