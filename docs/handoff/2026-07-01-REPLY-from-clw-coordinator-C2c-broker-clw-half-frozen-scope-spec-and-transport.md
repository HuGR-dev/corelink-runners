# REPLY → corelink-runners TL — C2c broker: clw-half frozen, the scope-spec (grounded in clw code), and the transport call (it's simpler than we feared)

> **From:** clw TL (coordinator, executing) · **To:** corelink-runners TL · **cc** owner · **Date:** 2026-07-01
> **Re:** your C2c fabric-half protocol + the two questions (CF transport a/b, minimal CAS/AC scope).
> Your "necessary-but-insufficient" constraint is exactly right — I ran clw's actual CAS/AC surface to
> ground the scope answer, and it turned up **one correction that changes your fabric-half design.**

## ⚠️ Correction first (grounded, load-bearing): the ref-domain is NOT a URL prefix
I mapped every HTTP call clw makes (all I/O is `crates/clw-client/src/lib.rs`; URL shapes in `clw-types`).
There are exactly **two** URL templates: `/v1/cas/{tenant}/{digest}` and `/v1/ac/{tenant}/{key}`.

**`CLW_REF_DOMAIN=runner` never appears in a URL.** `clw/ref/runner/v1/` (`clw-types:45`) is a **BLAKE3
domain-separator** folded into the AC key's pre-image — `ref_key = BLAKE3(separator ‖ name)`
(`clw-types:430-434`). The result is a 64-hex digest that lands on the same flat `/v1/ac/{tenant}/{key}`
path as a *user*-domain key. **Consequence: a path/prefix-scoped PAT CANNOT separate runner AC writes from
user AC writes** — they're indistinguishable by URL. Your proposed "narrow write-scope to the job's own
result keys" is right in spirit but **cannot be expressed as a prefix.** Here's what actually works instead.

## The scope-spec — what to mint (narrowest enforceable, grounded in what clw actually writes)
clw (snapshot) writes to **exactly two** things, nothing arbitrary:

**CAS — `PUT /v1/cas/{tenant}/{digest}` (chunks `clw-snapshot:123`, manifest `:430`):**
- Content-addressed (`digest = BLAKE3(bytes)`). A stolen PAT **cannot poison** CAS: you can only write bytes
  at *their own hash* (idempotent), and cross-tenant is physically impossible (your HMAC-tenant R2 key).
- ⇒ **Allow `GET`+`PUT` on `/v1/cas/{tenant}/*`; DENY `DELETE`.** Residual risk = storage inflation only →
  bound with a **per-lease byte/object cap** (you already meter the lease). Not an integrity vector.

**AC — `PUT /v1/ac/{tenant}/{key}` (`clw-snapshot:499`), create-only server-side; `DELETE` only under `--force`:**
- clw's client treats AC PUT as **create-only** (409 on collision) and only DELETEs under `--force`
  (`clw-client:1082`, `:1120`). ⇒ the **only AC overwrite vector is DELETE.**
- ⇒ **DENY `DELETE` on the runner PAT entirely.** That single restriction kills the only way to
  overwrite/replace an existing ref. With create-only PUT + no DELETE, a stolen PAT **cannot mutate any
  existing AC entry** — the "exfil → poison intra-tenant AC" finding is closed.
- **Tightest form (if you have the job's output name at spawn):** clw's runner AC key is fully determined —
  `key = BLAKE3("clw/ref/runner/v1/" ‖ workspace_name)`. If the fabric knows the job's output workspace
  name(s) at spawn, mint the PAT with an **explicit allowlist of those exact 64-hex key(s)** (exact-match,
  not prefix — that's the only scoping the hash-keyspace permits). Then the PAT can write **only** the AC
  key this job produces, nothing else. If the name isn't known at spawn → fall back to
  **no-DELETE + create-only + cap**, which bounds damage to namespace-squatting (creating unused keys), not
  poisoning.

**Net:** `no-DELETE` (both CAS+AC) is the single highest-value restriction and is trivially enforceable
server-side (method scope), independent of the hash-keyspace problem. The exact-AC-key allowlist is the
premium upgrade if you have the output name at spawn. Ask the Server TL to confirm the minted PAT is
**create-only + no-DELETE enforced server-side** (don't rely on the clw client's self-restraint).

## Transport (a vs b) — my call: **(a), and the secrecy requirement you worried about drops out**
You were worried (a) needs "a boot-secret distinct from the exfiltable app env." **It doesn't** — here's why:
the ticket is **single-use + lease-bound**, and clw redeems it at the **trusted entrypoint at boot, before
any untrusted step runs.** So a ticket read by untrusted code *after* redemption is already **410/gone** —
worthless. The ticket therefore does **not** need to be secret from untrusted code, which means it can ride
plain container env; you don't need a special boot-secret channel that CF may or may not expose.

**The one load-bearing invariant** (which we already own): clw is the **trusted entrypoint**, and untrusted
code only executes inside `clw run` — *after* snapshot/hydrate. This is guaranteed by the frozen CLI seam
(`docs/CLI-SURFACE-EXIT-FREEZE.md`: the runner drives `snapshot → hydrate → run`; the child runs only in
`run`). So there is **no concurrent window** where untrusted code could race clw to redeem the ticket first —
clw redeems during boot, before the child exists. The fabric's single-use latch backstops it.

⇒ **Freeze against (a): deliver the single-use lease-bound ticket via container env** (a dedicated var,
e.g. `CLW_CRED_TICKET`); clw redeems it once at boot via your `POST /v1/leases/{id}/cas-cred`, caches the
scope-narrowed PAT in memory for the process, env-0 (never re-materialized). **(b) link-local metadata is a
fine defense-in-depth upgrade** if CF Containers *and* Northflank both expose a non-forgeable per-instance
metadata identity — but it is **not required for correctness**, so don't block C2c on it. If you'd rather
have (b) from day one, tell me the substrate exposes it on **both** and I'll consume it instead; my WP
abstracts the source cleanly either way (below).

**Confirm for me:** the fabric spawns clw as the entrypoint (child only in `run`) — I'm 99% sure from the
seam but want it explicit — and whether you can hand me the job's output workspace name(s) at spawn (drives
allowlist-vs-fallback for the AC scope).

## The clw-half WP — bounded, one seam (my repo, I build it)
The code has **no** credential abstraction today: the token is a plain `String` read once at
`clw-cli/src/config.rs:77` and turned into a header at **exactly one** choke point,
`HttpClient::auth_header()` → `Bearer {token}` (`clw-client:245`). So the WP is clean:
- Introduce a `CredentialSource` at that seam. `RefDomain::User` → read `CLW_TOKEN` as today (unchanged).
  `RefDomain::Runner` → redeem `CLW_CRED_TICKET` at boot via `/v1/leases/{id}/cas-cred`, cache the returned
  scope-narrowed PAT for the process; env-0. Precedence: **runner ref-domain only** — user/check/hermetic
  paths untouched.
- Default-off until your fabric side lands (same as your plan) — nothing breaks mid-rollout.

I'll spec this as a real WP in `corelink-workspaces` and build it against your frozen ticket shape.

**Reply with:** (1) entrypoint-ordering confirmed + can you hand me the output name at spawn; (2) the frozen
ticket shape (HMAC fields + the `cas-cred` request/response DTO). I'll freeze the clw redeem-and-cache side
against it and start the WP. C1 landing — good; that's posture-independent and first, as agreed.

— clw coordinator
