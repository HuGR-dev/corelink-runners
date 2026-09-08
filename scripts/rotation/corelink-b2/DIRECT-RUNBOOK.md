# Direct production rotation runbook

`bin/direct-rotate-production.sh` is the approved direct path for the exposed
Corelink secrets. It has no Hugit, Githugr, GitHub-action, controller, nonce, or
external-verifier dependency. GitHub remains a provider used by the existing
Corelink fleet-busy implementation.

It defaults to an inert plan. A live invocation must supply the two exact
acknowledgements, the current clean B2 source commit, exact SHA-256 pins for
both current configs, current provider deployment-version pins and expected
image digests. It reads the fleet key and the `corelink-canary-tenant-pat` only
from mode-0600 files under `~/.corelink/rotation-b2-20260908`.

The procedure has a fixed sequence: prove the exact baseline and empty fleet;
freeze Spawn then Fabricd using existing `FABRIC_ADMISSION_PAUSED` and
`--keep-vars`; generate mode-0600 independent primary and recovery pairs;
deploy Spawn and Fabricd with only new values; prove health, exactly one active
attestation key, Corelink v1/v2 contract, and new-token 503 / old-token 401;
release Spawn then Fabricd; run one tenant acquire/close canary; refreeze
Fabricd then Spawn and leave both frozen. On any failure after release, the EXIT
trap attempts the same refreeze order. Neither normal nor recovery mode writes
an exposed value.

The existing fleet endpoint exposes its active runner count as `busy`; it does
not expose a second `active_count` field. The harness requires `busy=0` and
`unverifiable=0`, and records `active_count=0_from_busy` rather than inventing
an extra control-plane API.

`run_wrangle` obtains `npx --no-install wrangler auth token --json` for every
Wrangler command, keeps that OAuth value out of arguments and evidence, and
uses it only in the child process environment. Evidence records public IDs,
digests and status facts only.

Run the finite local checks:

```sh
bash scripts/rotation/corelink-b2/tests/test-direct-rotate.sh
shellcheck scripts/rotation/corelink-b2/bin/direct-rotate-production.sh
```
