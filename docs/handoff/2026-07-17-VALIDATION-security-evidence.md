# Security validation — evidence matrix (2026-07-17)

Adversarial, read-only security validation of the whole live system: every gate
fail-closed, no secret leak. Each atom below carries a CITED artifact — a code
`file:line`, an existing test name, or a **live HTTP status captured on
2026-07-17** against the deployed Workers. This is the *security atom* of the
"every molecule validated" campaign.

- **Scope:** `deploy/cloudflare/` (spawn-worker), `deploy/cloudflare-fabricd/`
  (fabricd proxy) + `crates/corelink-fabric-server/` (the Rust control plane it
  fronts). READ-ONLY on code; live probes were all read-only (bad/absent auth →
  expect a refusal; never a mutating request).
- **Live endpoints probed:**
  `https://corelink-fabricd.gmhelmold.workers.dev` (`FAB`) ·
  `https://corelink-spawn-worker.gmhelmold.workers.dev` (`SW`).

## Verdict summary

**22 atoms SOLID · 0 fail-open / secret-leak findings · 3 defense-in-depth-INERT
notes (not bugs) · 1 documented best-effort gap (G2/IMDS, pre-existing, tracked).**

No CRITICAL or HIGH finding. Every gate probed live fail-closed; every sensitive
value is redacted or never logged; the raw CAS PAT provably never reaches the
untrusted container env under the armed prod (env-0) path.

Notable: **every** fabricd internal route returned **401** (not 404) to a
wrong/absent key — meaning `FABRIC_OBSERVABILITY_KEY` **and** `FABRIC_ADMIN_KEY`
are BOTH armed in prod and fail-closed. The task allowed "404 (unset) or 401
(wrong)"; live posture is the stronger 401.

---

## Atom 1 — Auth fail-closed on EVERY gate (code + LIVE probe)

### fabricd (`FAB`) — live results captured 2026-07-17

| Route | No auth | Wrong key | Verdict | Evidence |
|---|---|---|---|---|
| `GET /v1/health` (auth-free) | **200** | n/a | SOLID | live; `deploy/cloudflare-fabricd/src/index.ts:739` (falls to shard-0 passthrough, no gate) |
| `GET /v1/attestation/key` (public key ONLY) | **200** serves `{keys:[{key_id, pubkey_b64, expires_ms}]}` | n/a | SOLID | live body = `key_id:faa5b7726…, pubkey_b64:Mo4w…` — **no private key**; `crates/corelink-fabric-server/src/attestation.rs:412` `key()` returns `KeyEntry{key_id, pubkey_b64}` only; doc'd UNAUTHENTICATED-by-design (att.rs:406-411) |
| `GET /internal/v1/status` | **401** | **401** | SOLID (fail-closed) | live; `crates/corelink-fabric-server/src/handlers/status.rs:64-76` (None⇒404, mismatch⇒401, constant-time `secret_matches`) |
| `GET /internal/v1/occupancy` | **401** | **401** | SOLID | live; `handlers/occupancy.rs:71-83` (same gate) |
| `POST /internal/v1/admin/tenants/{t}/suspend` | **401** | **401** | SOLID | live; `handlers/admin.rs:250-262` (None⇒404, mismatch⇒401, constant-time) |
| `GET /v1/leases` (tenant bearer) | **401** | **401** | SOLID | live; `crates/corelink-fabric-server/src/auth.rs:93-129` `require_tenant` |
| `POST /v1/leases/{id}/cas-cred` bad ticket | **401** `{"error":"invalid ticket"}` | — | SOLID (no cred leaked) | live; `handlers/cas_cred.rs:61-63` constant-time `verify` ⇒ 401 |

### spawn-worker (`SW`) — live results captured 2026-07-17

| Route | No auth | Wrong key | Verdict | Evidence |
|---|---|---|---|---|
| `GET /internal/v1/metrics` | **401** | **401** | SOLID | live (key armed in prod ⇒ 401); `deploy/cloudflare/src/index.ts:969-975` (unset⇒404, mismatch⇒401, `safeEqual`) |
| `POST /webhook` no sig / bad sig | **401** | **401** | SOLID — NEVER spawns | live; `index.ts:981-989` (missing secret⇒503; else HMAC-verify⇒401). Prod returned 401 ⇒ secret+mint armed and rejecting |
| `POST /v1/spawn` | **401** | **401** | SOLID | live; bearer gate `index.ts:1199` `authed()` before any spawn logic |
| `POST /v1/exec` | **401** | (behind bearer) | SOLID | live; same `authed()` gate at `index.ts:1199` |
| `POST /v1/leases/{id}/cas-cred` bad ticket | **404** `{"error":"no such lease"}` | — | SOLID (no cred leaked) | live; `index.ts:1170-1194` → DO `redeem` → `decideRedeem` (`lib.ts:389-399`): 404 never-stashed, 401 bad ticket, 410 expired |

**`authed()` fail-closed on empty secret** — `index.ts:431-436`: `tok.length===0
⇒ return false` (no secret configured ⇒ deny). Mirror in Rust `auth.rs:98-99`
(no Bearer ⇒ 401) and `auth.rs:124-128` (store unreachable ⇒ **503 FailClosed**,
never anonymous admission — exhaustive `match`, no fall-through arm).
Test: `token_store_down_fails_closed_503_never_open` (cited `auth.rs:92`).

**Note (defense-in-depth, not a hole):** `SW` unknown route `/nonexistent`
returned **401**, because the bearer gate (`index.ts:1199`) runs before the
`404` fallthrough (`index.ts:1429`). This is fail-closed and does not reveal
route existence — acceptable.

---

## Atom 2 — HMAC (constant-time, fail-closed) + live bad-sig probe

- **spawn-worker** `verifyGithubHmac` (`lib.ts:170-182`): rejects when
  `!sig.startsWith("sha256=")` (fail-closed on missing/short/mal-formed), then
  `safeEqual` — constant-time byte-loop, length-check first (`lib.ts:159-166`).
- **fabricd/Rust** webhook `signature_valid` (`handlers/webhook.rs:741-749`) uses
  the shared `constant_time_eq` (`ingest_token.rs:158-167`, OR-folded, no early
  exit, length OR-folded). One HMAC impl pinned to **RFC 4231 TC2** known-answer
  (`ingest_token.rs:179-186`).
- **Live bad-sig probe:** `SW POST /webhook` with `x-hub-signature-256:
  sha256=deadbeef` ⇒ **401**, and with a non-`sha256=` header ⇒ **401**. Never
  spawns.
- Tests: `bad_signature_is_401` (`handlers/webhook.rs:1071`); TS "a BAD signature
  ⇒ 401 and NEVER spawns" (`test/webhook-route.test.ts:231`);
  `prop_constant_time_verify_no_short_circuit` (`ingest_token.rs:466`).

**Verdict: SOLID.**

---

## Atom 3 — Secret non-leak (exhaustive audit)

**Grep across every `console`/`logEvent`/`eprintln`/`tracing` + error-body +
`Response` + container `envVars` for token/PAT/JWT/private-key/ticket/HMAC-secret/
attestation-key VALUES.**

- **Rust:** the only `eprintln!` hits near secret nouns are *state descriptions*,
  never values: `server.rs:370-372` (DEV-key WARNING banner, no key printed),
  `runner_broker.rs:987` (`private key invalid ({e})` — the PEM *parse error*,
  not key bytes), `handlers/webhook.rs:688-696` ("PAT is not a known tenant" /
  "unreachable" — no PAT value). Sensitive newtypes redact `Debug` by
  construction: `BearerPat` (`auth.rs:32-36`), `IngestSigner`
  (`ingest_token.rs:74-78`), `CredTicketSigner` + `StashedCred.token`
  (`cred_ticket.rs:49-53, 96-104`), `MintedPat.token` (`runner_cas_mint.rs:63,
  ~72-80`). No `{:?}` path can print a secret.
- **TS:** three log lines touch secret nouns; all safe — `lib.ts:444`
  ("cred-stash failed … (no token leaked): ${error.message}"), `lib.ts:483`
  ("injecting legacy CLW_TOKEN …" — names the action, not the value),
  `index.ts:1069` (error message only). No `${...token/ticket/cred/jit...}`
  interpolation into any log string (grep for that pattern returned empty).
- **Error bodies never leak:** `github_app.ts:167-179` installation-token error
  carries only status + GitHub's body, **never** the JWT or private key (I2);
  `mintCasPat` on a malformed 200 logs `Object.keys(j)` not values
  (`lib.ts:257-265`). fabricd `upstreamTimeout503` is a generic reason, no
  internals (`index.ts:522-528`); the top-level catch returns
  `{error:"internal error"}` with the message only in the log, not the body
  (`index.ts:906-911`).
- **The cred-ticket envelope (raw `CLW_TOKEN` off the untrusted box under
  env-0):** `buildContainerEnv` (`lib.ts:414-508`) — when env-0 deps
  (`stash`+`fabricEndpoint`) are present, the overlay injects
  `CLW_CRED_TICKET`/`CLW_LEASE_ID`/`CLW_FABRIC_ENDPOINT` and **never**
  `CLW_TOKEN` (`lib.ts:447-460`). A stash failure spawns COLD, not with the PAT
  (`lib.ts:441-446`). The legacy raw-PAT branch is gated behind
  `ALLOW_LEGACY_PAT_ENV==="1"` **and** refused whenever the prod marker
  `SPAWN_WORKER_PUBLIC_URL` is set (`lib.ts:473-481`), so a raw PAT can NEVER
  reach an untrusted container in a prod deploy. fabricd-side is enforced at boot
  by `validate_mint_arm` (see Atom 4).

**Verdict: SOLID — no secret-leak finding.**

---

## Atom 4 — Token scoping & lifetime

| Item | Evidence | Verdict |
|---|---|---|
| App JWT `exp − iat ≤ 10min` | `github_app.ts:43,89-90`: `APP_JWT_TTL_S=540`, `exp = iat + 540 ≤ 600` (iat back-dated 60s) | SOLID |
| Installation-token cache expiry safety | `github_app.ts:145` returns cache only when `nowMs < expMs − 5min` (`INSTALLATION_TOKEN_SAFETY_MS`); cache key EXACTLY `ghtok:<installationId>` — installation-scoped isolation (I3, `github_app.ts:124-137`) | SOLID |
| Per-job CAS PAT scope (`cas:rw`, per-job, tenant-scoped) | `runner_cas_mint.rs:4-24` — `scope:"read-write"`, per-job, tenant DERIVED server-side (caller never sends `owner_tenant`; single-tenant hole closed 2026-07-08) | SOLID |
| PAT lifetime ≤ lease | `runner_cas_mint.rs:41-42` `expires_ms ≤ lease.expiry`; near-expiry mint fails CLOSED without calling mint — test `mint_near_expired_lease_fails_closed_without_calling_the_mint` (`runner_cas_mint.rs:941`) | SOLID |
| Cred-ticket TTL bound | `lib.ts:359` `CRED_TICKET_TTL_S=7200`; DO alarm wipes at expiry (`index.ts:230-253`); + explicit `wipe()` at job completion (`index.ts:1067-1071`) | SOLID |
| Multi-use envelope is coordinator-ACKed (NOT a bug) | `lib.ts:316-333` + `decideRedeem` (`lib.ts:389-398`): served on every redeem until lease-TTL wipe — DELIBERATE (two clw processes: boot hydrate + `clw run`); cred is a per-job tenant-scoped `cas:rw` PAT with no escalation over the job's own cache access. Window bounded by completion-wipe. | SOLID (documented, not flagged) |
| Ingest token = write-only, lease-scoped capability (NOT the tenant PAT) | `ingest_token.rs:1-46` — closes the P0 tenant-takeover; worst-case disclosure bounded to one dying lease's ingest endpoint | SOLID |

---

## Atom 5 — X4 supply-chain (every Dockerfile FROM @sha256-pinned)

| Dockerfile | Base image(s) | Pinned? |
|---|---|---|
| `crates/corelink-fabric-server/Dockerfile` | `rust:1.96@sha256:6df234c1…` (builder), `debian:bookworm-slim@sha256:60eac759…` (runtime) | YES |
| `deploy/runner/Dockerfile` | `ubuntu:24.04@sha256:786a8b55…` (×3 stages) | YES |
| `deploy/check-host/Dockerfile` | `ubuntu:24.04@sha256:786a8b55…` (×2 stages) | YES |

All `FROM` lines are digest-pinned. `deny.toml` enforces crates.io-only (no
git/path deps). **Verdict: SOLID.**

- **PINNED_IMAGE_DIGEST guard state (spawn-worker):** INERT by design when unset
  (`index.ts:110-122, 1268-1273`). The container image is wrangler-bound
  regardless, so this is defense-in-depth, not the isolation floor; owner-gated
  with the runner fleet. **Note: defense-in-depth INERT, not a hole.**

---

## Atom 6 — Untrusted-env (egress / IMDS)

- `enableInternet = true` on `RunnerContainer` (`index.ts:314`) +
  `CheckHostContainer` (`index.ts:369`) — the runner needs egress (git clone, GH
  API, CAS hydrate); bounded by ADR-0003 (scoped short-TTL PAT + ephemeral box).
- **G2 IMDS/metadata denylist is BEST-EFFORT and explicitly documented as
  NOT-closing** (`index.ts:18-46`): `simpleGlobMatch` does no CIDR math, so only
  exact-host entries (`169.254.169.254`, `metadata.google.internal`) match, and
  RAW SOCKETS bypass the SDK proxy entirely. The CIDR ranges are left as an inert
  TODO, deliberately NOT re-added (they'd silently no-op). The class-property
  `deniedHosts` was REMOVED because on `@cloudflare/containers` 0.3.x it broke
  GitHub egress (`index.ts:316-322`). Tracked in `docs/adr/0009-untrusted-
  isolation-signoff.md` and `docs/adr/0003-egress-isolation-posture.md`.
- **Kill-switch:** operator `POST /v1/egress-cutoff` → `cutEgress()`
  (`setDeniedHosts([...METADATA_DENYLIST,"*"])`, `index.ts:346-348, 1404-1427`),
  admin/bearer-authed; caveat: SDK-proxy layer only (raw sockets bypass) —
  `teardown()`/`destroy()` is the hard fail-closed control.

**Verdict: documented best-effort gap (G2), pre-existing and owner-tracked — NOT
a new finding.** The real IMDS/link-local block needs platform-network-layer
filtering, honestly disclosed in-code.

---

## Atom 7 — Fuzz the shapes (malformed / oversized / wrong content-type)

| Probe (live 2026-07-17) | Result | Verdict |
|---|---|---|
| `SW cas-cred` malformed JSON body | **400** `{"error":"invalid JSON body: …"}` — clean, not 500 | SOLID (`index.ts:1174-1178`) |
| `SW cas-cred` missing ticket | 400 `{error:"ticket required"}` (code `index.ts:1179`) | SOLID |
| `SW /v1/spawn` malformed body + WRONG bearer | **401** (auth checked BEFORE body parse — no pre-auth parse) | SOLID |
| `SW /webhook` wrong content-type + bad sig | **401** (HMAC rejects non-`sha256=`) | SOLID |
| `FAB cas-cred` bad ticket | **401** `{"error":"invalid ticket"}` — no 500, no leak | SOLID |
| `SW` uncaught-throw backstop | top-level `try/catch` ⇒ structured `{error:"internal error"}` 500, message logged not bodied (`index.ts:906-911`) | SOLID |

Code discipline: `/v1/spawn` (`index.ts:1204-1215`), `/v1/exec`
(`index.ts:1292-1298`), `/v1/teardown` (`index.ts:1366-1370`) each guard
`request.json()` in try/catch ⇒ clean 400. `image_digest` type-guarded before
`.includes` (`index.ts:1213`) so an absent/non-string field is a 400, not a 500.
Container-fetch failures on `/v1/exec` fail-CLOSED 502/503, never a fabricated
`CmdOutput` (`index.ts:1323-1333`).
Tests: `cred-cred-route.test.ts:131` ("malformed JSON body ⇒ 400 (clean, not an
opaque 500)"); scatter-gather skips a shard returning a malformed body
(`deploy/cloudflare-fabricd/src/index.ts:337-341`).

**Verdict: SOLID.**

---

## Findings & inert-notes (ranked)

There are **no fail-open or secret-leak findings.** For completeness, the
defense-in-depth-INERT / documented items (NOT bugs, ranked by residual risk):

1. **[LOW / documented, pre-existing] G2 — IMDS/link-local egress block is
   best-effort.** An in-lease process on a substrate that exposes a metadata
   endpoint could reach it via raw socket; the SDK denylist does not stop it. The
   compensating controls are the scoped short-TTL PAT (nothing tenant-wide on the
   box, `ingest_token.rs`), env-0 (no CAS PAT in env), and ephemeral teardown.
   Tracked in ADR-0009/0003; needs platform-network filtering to close. Honestly
   disclosed in `index.ts:18-46`. *Not introduced by this validation; no
   regression.*
2. **[INERT, not a hole] `PINNED_IMAGE_DIGEST` unset** ⇒ the runner-mode digest
   assertion is dead. Image is wrangler-bound regardless (defense-in-depth).
3. **[INERT, not a hole] `EXEC_SERVER_AUTH_TOKEN` no-auth fallback** on
   `/v1/exec` (`index.ts:1315-1317`) is UNREACHABLE for any live check-host: a
   `mode:"check"` spawn now hard-requires the secret (fail-closed 503,
   `index.ts:1243-1249`), so no check container exists without its `/exec` gate.

---

## Live probes run (raw status log, 2026-07-17)

```
FAB /v1/health                              200   (auth-free OK)
FAB /v1/attestation/key                     200   public key only (key_id faa5b7726…, pubkey_b64) — no private key
FAB /internal/v1/status        no-auth      401
FAB /internal/v1/status        wrong-key    401
FAB /internal/v1/occupancy     no-auth      401
FAB /internal/v1/occupancy     wrong-key    401
FAB /internal/v1/admin/…/suspend no-auth    401
FAB /internal/v1/admin/…/suspend wrong-key  401
FAB /v1/leases                 no-bearer    401
FAB /v1/leases                 wrong-bearer 401
FAB /v1/leases/…/cas-cred      bad-ticket   401  {"error":"invalid ticket"}   (route mounted + validating externally)
SW  /internal/v1/metrics       no-auth      401
SW  /internal/v1/metrics       wrong-key    401
SW  /webhook                   no-sig       401  {"error":"unauthorized"}  (never spawns)
SW  /webhook                   bad-sig      401  {"error":"unauthorized"}
SW  /v1/spawn                  no-bearer    401
SW  /v1/spawn                  wrong-bearer 401
SW  /v1/exec                   no-bearer    401
SW  /v1/leases/…/cas-cred      bad-ticket   404  {"error":"no such lease"}    (no cred leaked)
SW  /v1/leases/…/cas-cred      malformed    400  {"error":"invalid JSON body…"} (clean, not 500)
SW  /v1/spawn  malformed+bad-bearer         401  (auth before body parse)
SW  /webhook   wrong-ctype+bad-sig          401
```

All probes were read-only (bad/absent auth or an invalid ticket); no mutating
request was issued. Every gate fail-closed; the attestation-key route serves the
public key only.
