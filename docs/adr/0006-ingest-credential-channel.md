# ADR-0006 — The turn-feed ingest credential channel (scoped token now, broker later)

- Status: Accepted (Wave-4 P0 remediation)
- Date: 2026-06-14
- Owner-gated: no (a security-mechanism decision inside the frozen §5 invariant;
  the §5 *invariant itself* is owner/contract law and is upheld, not changed)
- Supersedes: the original WP-TURNFEED box-credential wiring (the P0 below)
- Related: ADR-0003 (egress posture — the bound this protects), §5 (secrets never
  on the box), §13.2 (turn-feed WRITE side), `crates/corelink-fabric-server/src/ingest_token.rs`

## Context

The §13.2 turn-feed (WP-TURNFEED) lets the in-box agent loop POST its own
trajectory to the lease's ingest endpoint
(`POST /v1/leases/{id}/envelope/ingest`), so the `CaptureHook` is fed by the real
job rather than only by tests. For the box to authenticate that POST, it needs
*some* credential in its environment.

The box is **untrusted** (contract §4 — it runs customer / AI-agent code) and has
**open internet egress** (ADR-0003). ADR-0003 accepts open egress *because* §5
guarantees no tenant/platform secret is ever on the box: there is nothing to
exfiltrate. That bound is load-bearing — break §5 and ADR-0003's risk acceptance
collapses with it.

The original WP-TURNFEED wiring injected the acquiring tenant's **raw, tenant-wide,
long-lived Bearer PAT** into the box env
(`CORELINK_ENVELOPE_INGEST_CREDENTIAL = &pat.0`). The Wave-4 re-audit graded this a
**P0**: a malicious job reads the env var, exfiltrates the PAT over open egress,
and takes over the **entire tenant API** — spin billable leases, exec/cancel/close
*other* leases, queue/trigger — and the credential **survives the lease**. It
violated §5 (`env=0` credential scan) and broke the ADR-0003 bound directly. (It
was default-OFF in the seed, but the Northflank production backend consumes
`spec.env`, so production would have shipped it.)

## Decision

Replace the tenant PAT in the box env with a **per-lease, write-only,
ingest-scoped capability token**. The tenant PAT never reaches the box again
(grep-enforced).

### The token

```text
ingest_token = base64_standard( HMAC-SHA256(ingest_secret, DOMAIN ‖ lease_id) )
  DOMAIN       = "envelope-ingest:v1:"   (ASCII; domain separation)
  ingest_secret= a DEDICATED per-fabric ingest key, wired at the composition root,
                 distinct key material AND algorithm from the ed25519 attestation
                 signing key (so an ingest token can never be confused with, or
                 forged from, an attestation signature, and vice-versa)
```

- **Deterministic, no per-lease storage.** The fabric recomputes + verifies the
  token from `lease_id` alone — no token table, no per-lease state.
- **Cross-lease isolation is intrinsic.** `lease_id` is folded into the HMAC
  pre-image, so lease A's token never verifies for lease B.
- **Verify is constant-time** (OR-folded, length-difference folded in — no early
  exit, no timing oracle), fail-closed **401** on any mismatch, and ingest auth
  runs **before** the registry lookup (no existence oracle).
- **HMAC-SHA256 over the workspace-pinned `sha2`** (the same hash dep the
  fence/memo-key paths use) — RFC 2104 construction, RFC 4231 TC2 known-answer
  test — so **no new crate** enters `Cargo.lock`.
- **Poll endpoints keep the tenant PAT** (Option A): `poll_events` / `poll_meta`
  are operator-side reads off the box, so the scoped token is *ingest-only*
  (write-only from the box's perspective).

### Why this satisfies §5 without weakening it

§5's `env=0` credential scan exists to keep **tenant / platform secrets** off the
box. The tenant PAT — a tenant-wide, long-lived secret — is exactly that, and it
is now **gone from the box**. The scoped ingest token is **not** such a secret: it
is a write-only, lease-scoped, ingest-only **capability** the box legitimately
needs to stream its *own* trajectory. Its worst-case disclosure is bounded to
**one dying lease's ingest endpoint** — an attacker who exfiltrates it can only
POST trajectory bytes to a lease that is already theirs and about to expire: no
tenant takeover, no cross-lease reach, no other capability. It is an explicit,
documented, scoped exception to `env=0`, not a hole in it.

## Alternatives considered

1. **Keep the tenant PAT in the box env** — *rejected* (this is the P0). Tenant
   takeover surviving the lease; breaks §5 + ADR-0003.
2. **A §5-pure broker / unix-socket channel now** (nothing in box env at all —
   see below) — *rejected for M1 as premature.* It depends on the box↔fabric
   local-socket plumbing that the Firecracker (FC) isolation substrate will
   provide; building it against the current managed-microVM (Northflank) backend
   would mean a bespoke side-channel we'd then rip out at FC. The scoped token
   removes the **P0 (tenant takeover) now** at zero new dependency and zero new
   transport, and degrades gracefully to the broker later.
3. **A short-TTL signed JWT-style token** — *rejected as over-built.* The lease
   *is* the TTL (the endpoint dies with the lease); a stateless HMAC over the
   lease id already binds scope + lease + secret with one primitive we already
   ship. No `exp` claim, clock, or new dep buys anything here.

## Consequences

- The P0 is closed: exfiltrating the box credential is now harmless.
- One documented, bounded exception to §5 `env=0` exists (the scoped token),
  recorded here and in `ingest_token.rs`'s module doc so it is never mistaken for
  drift.
- A new per-fabric `ingest_secret` must be provisioned at the composition root in
  production (per-region), alongside the attestation signing key. It is **not**
  derived from, and must not be reused as, the attestation key.

## Follow-up — the FC-era §5-pure broker channel (deferred, not debt)

When Firecracker isolation lands, replace the env-injected token with a channel
that puts **no credential in the box env at all**:

- The box reaches a **fabric-local ingest broker** over a host-mediated transport
  (a vsock / unix-socket the hypervisor exposes into the guest), not an HTTP
  endpoint authenticated by an env secret.
- The broker authenticates the **caller's lease by construction** — the socket is
  bound to exactly one lease's guest, so the lease identity is the connection, not
  a presented token. Nothing to exfiltrate, nothing to forge: `env=0` becomes
  literally true again, not "true except one scoped capability".
- Until then, the scoped token is the correct, P0-free M1 mechanism. This
  follow-up is a planned hardening tracked against the FC milestone — **not** a
  silent gap or accepted debt (it carries no §0.6 waiver because there is nothing
  unsafe shipping: the M1 posture is sound on its own terms).
