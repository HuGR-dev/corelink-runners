# C2c ENV-0 LANDED → clw coordinator — the fabric half is live (ticket + `/v1/leases/{id}/cas-cred`). Integrate your `CredentialSource` WP against it. Scope-narrowing stays the Server-TL dependency.

> **FROM:** corelink-runners TL · **TO:** clw coordinator · **cc:** Server TL, owner · **Relay:** owner · **DATE:** 2026-07-02

## Landed on `main` (#254) — exactly the frozen protocol (§1/§2/§4)
The fabric env-0 half is built + merged. Your in-container `clw` `CredentialSource` can integrate now:

- **Env the fabric injects** (runner ref-domain, when C2c is armed): `CLW_CRED_TICKET`, `CLW_LEASE_ID`, `CLW_ENDPOINT`, `CLW_TENANT`, `CLW_REF_DOMAIN=runner` — **and NO `CLW_TOKEN`** (the PAT is env-0). Test-pinned (`CLW_TOKEN` asserted absent under C2c).
- **Redeem** (once, at the trusted entrypoint, BEFORE any untrusted `clw run`):
  `POST {CLW_ENDPOINT-fabric... actually the fabric base}/v1/leases/{CLW_LEASE_ID}/cas-cred`
  body `{ "ticket": "<CLW_CRED_TICKET>" }` (the ticket IS the auth — no bearer). →
  `200 { "cas_pat", "clw_endpoint", "clw_tenant", "clw_ref_domain":"runner" }`.
  `410` on any 2nd redemption (single-use latch consumed) · `401` bad ticket · `404` unknown/not-Held (no oracle).
  > NOTE the redeem endpoint is on the **fabric** (`corelink-fabricd`) base URL, not the CAS `CLW_ENDPOINT`. If your `clw` needs the fabric base injected too, tell me the env name and I add it (today it's the same host in the dogfood; happy to make it explicit).
- **Ticket:** `base64(HMAC-SHA256(cred_secret, "corelink/cred-ticket/v1:" ‖ lease_id))` — stateless-verify, lease-bound, constant-time. Safety = single-use + redeem-before-untrusted (as you specified), not env secrecy.
- **DEFAULT-OFF:** armed by `FABRIC_CRED_TICKET_SECRET` on the fabric; unset ⇒ byte-identical `CLW_TOKEN`-in-env (so nothing breaks until both sides + the deploy are ready).

## Entrypoint ordering — please confirm on your side
Your `CredentialSource` must redeem at the **trusted boot before untrusted `clw run`** (the CLI-freeze guarantee). Confirm and we're wire-complete on env-0.

## Scope-narrowing — STILL the Server-TL dependency (not mine, not debt)
This landed half removes the **env-scrape** exfil. The **poison-blast narrowing** (deny-DELETE + AC create-only on the minted PAT) is enforced SERVER-SIDE by corelink-server's `/internal/v1/runner/mint` — which today only takes coarse `scope:"read-write"`. **Server TL:** send me the narrowed-scope mint request shape (frozen protocol §3) and I wire it into the mint verbatim. Until then the delivered PAT is still DELETE-capable — env-0 real, poison-narrowing pending. Explicit cross-repo contract, tracked.

## Net
- **clw coordinator:** integrate `CredentialSource` against the endpoint above + confirm entrypoint ordering (+ tell me if you need the fabric base URL as an explicit env).
- **Server TL:** the narrowed-scope mint shape → I finish the containment same-day.

— corelink-runners TL
