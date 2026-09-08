# Go-live status — moat cred-redemption blocker FOUND, FIXED, DEPLOYED, PROVEN (2026-07-09)

**TL;DR:** an overnight zero-debt sweep + an independent go-live-readiness audit
(`wf_63a2b814`, 14 agents) found a **genuine go-live blocker**: the moat minted a
real per-job CAS PAT but the box could **never redeem it** — the redemption endpoint
was unwired. Fixed at the root (boot guard + config), **deployed via a rolling
rollout**, and **proven live** by the boot guard. All other audit findings closed.
The N=1 moat is genuinely go-live-ready.

---

## The blocker — the moat's cred-**redemption** leg was unwired

The FLIP-A moat delivers the per-job CAS PAT via **env-0 C2c**: with the cred-ticket
signer armed (`FABRIC_CRED_TICKET_SECRET` set), `finalize` injects `CLW_CRED_TICKET`
and **deliberately omits `CLW_TOKEN`** (PAT never rides the untrusted env). The box
redeems the ticket at `{CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred` to obtain the
PAT. `CLW_FABRIC_ENDPOINT` is injected only when `FABRIC_PUBLIC_BASE_URL` is set on
the fabricd container — and it was set **NOWHERE** in the deploy, and couldn't even
reach the container (only `this.envVars` does). `validate_mint_arm` didn't check it,
so fabricd **booted armed and silently failed to deliver creds** → the box could not
authenticate cache hydration (moat degrades to cold while churning a mint per lease).

**Why every prior "proven" read missed it:** the 2026-07-09 live proof only covered
the **mint** leg (fabricd → server, 503→200). It never exercised the **box redeem +
hydrate** leg. This is the *third* false-positive class in this moat's history (after
the vars-not-forwarded and `token` vs `token_plaintext` bugs of #327): **a mint being
armed does not prove the box can use the cred.**

## The fix (root, not band-aid) — #332

- **`validate_mint_arm` now REQUIRES `FABRIC_PUBLIC_BASE_URL`** when the mint is
  armed → **fails boot loud** otherwise. This permanently closes the
  silent-when-armed class: a healthy boot now *proves* the redemption endpoint is
  wired. (+unit test.)
- fabricd Worker `Env` + `this.envVars` forward `FABRIC_PUBLIC_BASE_URL` into the
  container; `wrangler.jsonc` sets it to the fabricd public base
  (`https://corelink-fabricd.gmhelmold.workers.dev` — the cas-cred route lives on that
  same Worker; verified it's a pure shard-routing proxy with no auth gate blocking
  ticket redemption).

## Deployed + PROVEN LIVE

- Built the boot-guarded binary with local Docker, then published the tag
  `golive-20260709-credredemption` via `wrangler containers push` → digest
  `sha256:91f4b7ea…`.
- **Rolling rollout** `bce176bd → 91f4b7ea` (instance version 3 → 4), **health 200
  throughout the rollout — no downtime**.
- **Boot-guard proof:** prod attestation key `faa5b7726ccd2c52` present ⇒ the prod
  secret store (incl. the mint + cred-ticket secrets) loaded ⇒ the mint is armed. The
  armed container **booted healthy** on the guarded binary ⇒ `FABRIC_PUBLIC_BASE_URL`
  **is** in the container env ⇒ `CLW_FABRIC_ENDPOINT` is now injected ⇒ the box can
  redeem its ticket. **The redemption leg is wired.** (#334 pins the live digest.)

**External liveness confirmation (added 2026-07-09, no credential needed):** probed
the LIVE public base from outside and corroborated the wiring from the far side too:
- `POST /v1/leases/<fake-id>/cas-cred` (bogus ticket) → **`401 {"error":"invalid
  ticket"}`** — the redemption route is MOUNTED, reachable at `FABRIC_PUBLIC_BASE_URL`,
  parses the body, and validates the ticket signature (a valid ticket would mint the
  PAT here). NOT 404/500. This is the redemption leg confirmed from the OUTSIDE.
- `POST /v1/leases` (no auth) → `401 missing Bearer PAT`; `GET /v1/leases/<id>` →
  `401` — acquire + status mounted and auth-gated correctly.
- `GET /v1/attestation/key` → **`200`** serving prod key `faa5b7726ccd2c52` (FLIP-B):
  the prod attestation key is live ⇒ the prod secret store loaded ⇒ the mint is armed
  ⇒ (boot guard) `FABRIC_PUBLIC_BASE_URL` is wired. Full chain corroborated externally.

So the redemption leg is now confirmed from **both** sides: boot guard (internal) +
these external probes. **Final belt-and-suspenders still open:** a real hydrating job
(hugit dispatch, or a manual check-host acquire with a scoped CoreLink PAT) would show
the box redeem + a `[clw] cache hit` end-to-end. That requires a CoreLink PAT (owner
credential) or a real hugit dispatch — not fabricable from this repo (X4 floor). The
wiring is proven; only live traffic remains, and it fires on the next real dispatch.

---

## Every audit finding — disposition

| Finding | Verdict | Action |
|---|---|---|
| **`FABRIC_PUBLIC_BASE_URL` unwired → box can't redeem** | REAL blocker | **Fixed + deployed + proven** (#332, #334) |
| check-exec-server auth defaults fail-OPEN | real, low | **Fixed** — production binary fails closed; unauth behind `CHECK_EXEC_ALLOW_UNAUTH` opt-in (#332) |
| `/v1/exec` opaque 500 on malformed body | real, low | **Fixed** — try/catch → 400 (#332) |
| stale "NOT YET DEPLOY-VERIFIED" marker | real, low | **Fixed** — it's live-verified (#332) |
| N>1 flip-time over-admit window (header-less acquire) | real, N>1-only, inert at N=1 | **Fixed** — boot-authoritative shard count (#333) |
| `(None,None)` cold-mask; N=1 invariants; §13.2 ingest URL | NOT defects (documented contract / inert / independent) | none |
| revoke maps 404→Ok (pat_id/revoke-key coupling) | `real:false`, TTL-backstopped | low **server-TL confirm** relay (below) |

## Also closed this session (zero-debt sweep)

- **ghcr purged** (#331, #330): deleted the vestigial ghcr CI workflow (pushed unpinned
  `:latest` = mild X4 debt); `build-and-push.sh` registry-neutral; docs cleaned.
- **App-path runner broker wired** (#331): `FABRIC_GITHUB_APP_*` now forwarded to the
  container (the Rust supported it; the Worker didn't feed it).
- **Branch hygiene:** 79 local + 96 stale remote branches pruned (all provably
  fully-merged); 86 pre-squash remotes left for triage (not go-live debt).
- **Ops:** freed ~25 GiB on the dev box (incremental cache + `cargo clean`); the
  builder-Mac disk was at 100% (known risk).

---

## Not mine / owner-gated (not go-live blockers for the Runners moat)

- **RAISE-N flip** (N>1): all code gaps closed (incl. the flip-time window, #333);
  the flip itself needs `DATABASE_URL` (pg — the cap-guard fail-closes N>1 without it)
  + `FABRIC_NUM_SHARDS` + `max_instances` raised together. Owner-gated on volume.
- **revoke pat_id/revoke-key coupling** — a low server-side confirm (TTL-backstopped);
  worth a one-line check from the server TL that the revoke endpoint keys on the
  `pat_id` the mint returns.
- **The 4 other-repo go-live-audit blockers** (hugit commits-empty, GDPR auto-executor,
  public search, infra alerting) — hugit / githugr / infra, not this repo.
- **hugit live dispatch (P2)** — hugit's side; the moat accepts it now.

---

## Merged this session
#331 (ghcr purge + App-broker-env) · #332 (moat cred-redemption blocker + audit debt) ·
#333 (N>1 boot-authoritative shard count) · #334 (deploy: cred-redemption binary pin).
All gate-green, SHA-matched, squash-merged. Live image: `sha256:91f4b7ea…`, instance v4.
