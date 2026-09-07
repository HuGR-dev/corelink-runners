# Sprint 1 T1-W5 acceptance — startup/readiness mint gate

**Decision:** ACCEPT

**Version binding:** integration source tip `486b573`. The acceptance suite is
`crates/corelink-fabric-server/tests/mint_readiness_routes.rs`.

## Focused smoke

The focused route suite covers the finite startup/readiness matrix:

- unarmed mint (`None`) keeps `/readyz` at `200`;
- valid authenticated dispatcher self-check (`400` with the exact typed
  `BAD_REQUEST/job_id required/request_id` body) reaches `200`;
- absent/incorrect dispatcher key responses (`401`, `403`) fail closed with
  `503` and enter a terminal state without retrying;
- transport failures retry at most three times and then fail closed;
- readiness gates acquire before action-cache lookup, reservation, provider
  provisioning, or JIT exchange;
- a blocked probe does not block `/health`, and cancelling a waiter cannot
  cancel the single-flight probe or start a duplicate;
- the probe sends `{}` and no bearer credential.

The production path creates the gate only with both mint environment variables
configured; a missing half-configured arm fails at boot. Secret-bearing config
used redacted `Debug` implementations and the readiness probe carried only the
internal auth header, never a bearer token or request body secret. The source
SHA above binds these claims to the reviewed implementation version.

## Evidence command

```text
CARGO_BUILD_JOBS=1 cargo test -p corelink-fabric-server --test mint_readiness_routes -- --nocapture
```

No full CI, deployment, push, or merge is part of this acceptance.
