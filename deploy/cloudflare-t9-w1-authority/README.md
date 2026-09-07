# T9-W1 non-production compute authority

This will be an isolated Cloudflare Worker, D1 ledger and per-tenant Durable Object serialization gate for the T9-W1 acceptance lane. It has no
route, no cron, no container binding and no Cloudflare provider-start capability. Its strict
ceiling will be one vCPU for one second (`1000` vCPU-ms). `activate` and `settle` return `503
executor_protocol_unpromoted`, so this deployment cannot materialize compute before the paired
executor protocol will be promoted.

The public binding is `GET /internal/v1/compute/acceptance-binding`. It exposes the grant and
terminal public keys, expiry and ceiling; it never returns a private key. The issuer private key is
created outside Git with mode `0600` and must be transferred only to the approved non-production
issuer.

The terminal wire response will be the signed `t9-w1-terminal-v2` canonical envelope consumed by the
in-progress Ed25519 Runner verifier. It binds tenant, grant digest, generation, key id, signing
algorithm and receipt expiry. The public binding supplies the verifying terminal public key.

Rollback and cleanup are scoped to this target:

```sh
npm --prefix deploy/cloudflare-t9-w1-authority exec -- wrangler delete corelink-t9w1-authority-20260907-0047
npm --prefix deploy/cloudflare-t9-w1-authority exec -- wrangler d1 delete corelink-t9w1-authority-20260907-0047
```

Do not repoint `FABRIC_COMPUTE_URL` or alter a production Worker. After the expiry, this target
returns `410`; deletion removes both the Worker secret and the ledger.
