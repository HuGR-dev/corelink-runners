# Evidence audit — adversarial red-team of the e2e suite's own cells (2026-07-19)

**Trigger:** owner — *"você tem que auditar a evidência também."* A green cell is only worth what
its assertion **discriminates**. An independent adversarial auditor (a separate agent, told to
DISTRUST) red-teamed every cell + captured artifact for: false-positives (no discriminating
control), overclaim (assertion > artifact), tautology, weak/truncated capture, and READ-vs-ENFORCED
scope creep. **12 findings. All addressed** — strengthened, honestly downgraded, or tracked. This is
the skeptic mandate applied to my own harness: test-green ≠ live-proven.

## Findings & disposition

| # | Cell | Attack angle | Disposition |
|---|---|---|---|
| **F1** | `TS2-door-a-spawn` | overclaim: `conclusion=success` ⇒ "cache-warm spawn+teardown" | **FIXED** — now pulls the real runner from the job log, asserts `cf-runner-*` on `cloudchamber` (a genuine CF-ephemeral box); **drops the cache-warm claim** (records `cacheWarmHitObserved` — was `false`, no `[clw]` line) and **drops teardown** (`teardownCaptured:false`). Proves spawn+execute, nothing more. |
| **F2** | `TS5-cross-tenant-no-oracle` | tautology: random UUID → 404 is trivially true | **DOWNGRADED** → `TS5-unknown-lease-404`. Real cross-tenant no-oracle needs a **live foreign lease** (spawn batch); recorded as `crossTenantOracle: not-yet-proven`. |
| **F3** | `TS6-tenant-scoping` | overclaim: distinct ids ⇒ "no cross-tenant read" | **DOWNGRADED** → `TS6-tenant-identity-distinct`. Honest: distinct identity + the read API exposes **no cross-tenant selector** (structural). Foreign-resource read-isolation = spawn batch. |
| **F4** | `TS2-attestation-key` | weak: `200 + any 20 chars` | **FIXED** — asserts `keys[0].pubkey_b64` base64-decodes to **exactly 32 bytes** (ed25519) under a stable `key_id`. |
| **F5** | `TS5-internal-gate-fail-closed` | no positive control | **DOWNGRADED** — scoped to "header-less caller → 401"; records `positiveControl: absent` (the internal-auth key isn't in this harness). Not claimed "gated". |
| **F6** | `TS6-entitlement-by-tier` | READ vs ENFORCED | **DOWNGRADED** → `TS6-entitlement-value-by-tier`. Proves the cap **VALUE** is surfaced/monotonic; **enforcement** is a separate TS-3 stress cell. |
| **F7** | `TS2-usage-api-reads` | tautology + empty sets | **DOWNGRADED** — same PAT→same tenant is by construction; scoped to "3 routes 200, echo the caller's own tenant". Mis-scoping needs live data (spawn batch). |
| **F8** | `*-nonleak-sweep` | weak regex + weak capture | **FIXED** — the real PATs are `corelink_…` (96 chars); the regex now includes `corelink_` (**it would previously have MISSED a leaked PAT**). Added a non-vacuous floor (`bodiesSwept >= N`). Credential-bearing SUCCESS bodies swept in the spawn batch. |
| **F9** | `TS2-authed-acquire-validation` | overclaim: "image validation" from `400 "invalid"` | **DOWNGRADED** — a generic 400 doesn't name the field; scoped to "authenticated (400 not 401) + rejected at server-side validation before spawn". |
| **F10** | `TS2-substrate-health` | edge vs container | **DOWNGRADED** — `/health` may answer at the proxy Worker edge; scoped to "the HTTP **front** answers 200". Container-liveness proven by the authed introspect+ledger cells. |
| **F11** | `TS5-error-vocab-malformed` | uncaught info-leak | **FIXED + PRODUCT FINDING** — scoped to "no 5xx"; **records** that the DTO error echoes parser internals (`"Failed to parse … line/column"`) → a tracked info-disclosure to harden server-side (opaque error code). |
| **F12** | `TS5-v1-namespace-auth-front` | single-route, no comparison | **FIXED** — now hits a **known-real** route (`/v1/usage`) unauthenticated too and asserts the two 401 bodies are **byte-identical** — the actual no-route-oracle proof. |

## What was actually SOUND (auditor-confirmed)

- `TS5-auth-fail-closed-nopat` / `-badpat` — the "dead-deploy 401-everything" false-positive is
  genuinely defeated: the sibling `TS2-authed-acquire-validation` posts a **valid** PAT to the SAME
  route and gets `400` (not `401`), and the rejection bodies differ (`"missing Bearer PAT"` vs
  `"unknown PAT"`) — the introspect layer runs and discriminates.
- `TS5-cred-cred-bad-ticket` — asserts the **specific** body `"invalid ticket"`, not a bare 401 —
  proves the ticket validator executed (a blanket auth-front would say `"missing Bearer PAT"`).

## Product findings surfaced by the audit (tracked, not swept)

1. **DTO parser echo (F11)** — malformed-body errors leak the JSON-parser wording + line/column. Low
   severity (framework disclosure), but should be an opaque `{"code":"bad_request"}`. → server hardening.
2. **Cross-tenant no-oracle, cache-warm hit, entitlement enforcement, cross-route mis-scoping** — all
   require a **live spawned resource** to prove; they are **explicitly deferred to the box-spawn batch**
   and recorded as GAPs in the artifacts, never claimed green.

## Net

The two headline value claims — **spawn** and **cross-tenant isolation** — were the weakest cells,
each green on evidence consistent with the property being absent. They are now either honestly proven
(spawn: a real `cf-runner` on `cloudchamber`) or honestly downgraded with the exact stronger test named
(cross-tenant: needs a live foreign lease). Every remaining cell asserts something a broken system
would fail. Re-run live after hardening: **15/15 green** on the stronger assertions; leak-sweep clean
against the corrected `corelink_` regex.
