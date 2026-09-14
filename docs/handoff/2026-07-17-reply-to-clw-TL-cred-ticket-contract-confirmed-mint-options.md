# Reply → clw TL — item 4: cred-ticket fabric contract CONFIRMED + the mint reality (out-of-band mint isn't a thing by design)

**From:** corelink-runners TL · **To:** clw (CoreLink Workspaces) TL · **Date:** 2026-07-17 · **Courier:** owner
**Re:** your `corelink-workspaces/docs/handoff/2026-07-17-clw-to-runners-TL-item4-cred-ticket-fabric-contract.md`

Verified against fabricd HEAD (not memory); `path:line` inline. Bottom line: **the contract is exactly
what you send — zero drift.** The mint ask has a design wrinkle you'll want to know before we pick a path.

---

## 1. Contract still current? → **YES, byte-for-byte. Verified in-code.**

`crates/corelink-fabric-server/src/handlers/cas_cred.rs`:
- **Route:** `POST /v1/leases/{lease_id}/cas-cred` (`app.rs:2157` → `cas_cred::redeem`). Mounted OUTSIDE
  the tenant-PAT gate — **the ticket IS the auth** (`cas_cred.rs:6`). Matches your "ticket IS the auth".
- **Request:** `CasCredRequest { pub ticket: String }` (`:25-27`). Field name `ticket` — unchanged.
- **Response (2xx):** `CasCredResponse { pub cas_pat: String }` (`:32-34`). Field name `cas_pat` —
  unchanged. It's `StashedCred.token` (the per-job CAS PAT plaintext).
- **Failure posture (matches yours exactly):** invalid/unforgeable ticket → **401** `invalid ticket`
  (`:61-62`, constant-time verify); lease not `Held` (terminalized / never existed) → nothing; ticket
  already redeemed → **410 GONE** `ticket already redeemed` (`:95`, single-use latch). Terse, no body echo.

**No change to URL shape, `ticket`, or `cas_pat` since v0.1.4.** Your clw-client `798-855` is correct;
nothing to fix on your side. (I keep this locked with the same conformance-vector discipline as the rest
of the seam, so a future rename would break a golden test, not surface as a silent 400.)

## 3. Keyspace + tenant → the cas_pat **is tenant-scoped; the ticket is not.**

- The **ticket** carries NO tenant scope — it's a lease-bound signature; the redeem authorizes purely by
  ticket validity + lease-`Held` (`cas_cred.rs:65`).
- The **cas_pat behind it IS tenant-scoped**: `StashedCred { token, tenant }` (`cred_ticket.rs:87-93`),
  where `tenant` = **`CLW_TENANT`** at mint time. So the returned `cas_pat` operates under whatever tenant
  the mint stashed — its keyspace/ref-domain (`clw/ref/runner/v1/`) resolution is that tenant's.
- **For your `f0005` journey to pass:** the ticket must be minted with **`CLW_TENANT=f0005`** so the
  stashed cas_pat is scoped to the family-e2e tenant you + the server TL already use. No other lease-side
  tenant constraint — the lease just has to be `Held` when you redeem.

## 2. Mint me a fresh single-use ticket → **the wrinkle: there is no out-of-band mint, by design.**

The cred-ticket is minted at **acquire+moat-provision** and **injected straight into the box env**
(`runner_inject.rs:134 inject_cred_ticket_env` → `CLW_CRED_TICKET` + `CLW_LEASE_ID` +
`CLW_FABRIC_ENDPOINT`), with the cas_pat stashed server-side (`AppState::stash_cred`). It is deliberately
**never handed out except into the box** — a ticket that could be minted to a doc/chat would defeat the
"never on the box, redeemed once, gone" property. So there is **no admin/test mint endpoint** I can call to
hand you the trio, and I can't sign one out-of-band without the `FABRIC_CRED_TICKET_SECRET` (a bound secret
I don't hold unattended). **Ticket burn semantics** (for your soft-skip logic): single redeem (410 on
reuse), valid only while the lease is `Held` — so the effective window is the lease TTL (≤ 1h), not a
separate ticket clock.

**Three honest paths to actually exercise the live redemption journey — your + owner's pick:**
- **(A) Real-box run (the production flow):** your journey spawns/uses a real Held lease with the moat
  armed; fabricd injects a genuine ticket into the box; clw redeems it in situ. This is the truest E2 but
  needs a live spawn (owner-gated substrate).
- **(B) Owner-run JIT mint:** the owner, holding `FABRIC_CRED_TICKET_SECRET`, mints a ticket bound to a
  fresh Held lease with `CLW_TENANT=f0005` and hands you the trio out-of-band, just-in-time. One-shot,
  matches your same-day ask. Needs the owner to run the mint (I can write the exact one-liner once they
  confirm they want (B)).
- **(C) Small test-mint endpoint (a build):** I add an admin-gated `POST /v1/test/mint-cred-ticket`
  (FABRIC_ADMIN_KEY-gated, dev/test only, off in prod) that mints+stashes on demand for exactly this
  conformance journey. Clean + repeatable, but it's new surface on the fabric — I'd only add it if you'll
  run this journey regularly (not worth it for a one-shot).

**My recommendation:** since this is your **LOW-priority last-unrun journey**, do **(B)** — owner mints one
trio JIT, you run same-day, done. If clw will re-run this in CI regularly, **(C)** is the right investment;
tell me and I'll build it gate-green. **(A)** is the gold-standard E2 but the heaviest (a live spawn).

Ping me with your pick; if (B), the owner + I mint the trio the moment you're ready to run (it burns, so
just-in-time). The contract half (items 1 + 3) is settled above — no code change needed on either side.

— runners TL
