# Fabricd secret rotation (AU1.8)

This is an owner-approved change procedure for `corelink-fabricd`. It documents
the repository harness and does not authorize a live change by itself. A fresh
operator who did not write this procedure must hold the approved change window.
The harness is the sole procedure for secret, deploy, delete, and network
operations; do not substitute manual Wrangler or alternative secret-rotation
commands. No live PASS is implied by this document or by source review.

## Preconditions and exclusive ownership

Record the owner, `CHANGE_ID`, and the provider-resolved UUID for the exact
Cloudflare Containers application named
`corelink-fabricd-fabricdcontainer`. The UUID is not a logical container class,
Worker name, or GitHub App id. Confirm the account from
`deploy/cloudflare-fabricd/wrangler.jsonc` (`6a1fc1c626fc2628823e60b9db01f5cd`)
with the pinned `whoami` check.

Before any provider read, acquire an owner-only, exclusive resource lock for
the `corelink-fabricd` application, Worker, `FABRIC_CRED_TICKET_SECRET`, and
temporary test tenant. Acquire it atomically, for example with
`mkdir "$HOME/.corelink/locks/corelink-fabricd-au1.8.lock"`; record the owner
and `CHANGE_ID` inside the lock using an owner-only file. If the directory
already exists, or if the owner cannot retain it for the complete run and all
recovery, stop with `RED`; never operate concurrently. Remove the lock only
after final evidence or escalation is recorded.

The operator must supply these OOB paths, each regular, non-symlink, non-empty,
owner-readable, and mode `0600`: `AU18_OLD_SECRET_FILE` (rollback value),
`AU18_TEST_MINT_KEY_FILE`, `AU18_PAT_FILE`,
`AU18_INTROSPECT_KEY_FILE`, `AU18_FLEET_BUSY_KEY_FILE`, and
`AU18_OBSERVABILITY_KEY_FILE`. `AU18_NEW_SECRET_FILE` is an absent destination
that the harness creates atomically with nofollow destination checks and mode
`0600`; never pre-create it or replace it with a symlink. Set `AU18_APP_ID` to
the exact provider UUID and `AU18_HEALTH_URL` to the owner/provider health URL
in the private operator shell. Keep all values out of Git, shell history,
terminal output, logs, and evidence.

## Pinned execution

Run only from a clean checkout at the reviewed source commit. The executable
gate is `scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh`; it requires both
acknowledgements and resolves
`deploy/cloudflare-fabricd/node_modules/.bin/wrangler` at the repository-pinned
version. It refuses `npx`, global or network-resolved Wrangler, dirty trees,
ambiguous application identity, existing temporary key/binding, and unsafe OOB
files.

```sh
export AU18_APP_ID AU18_HEALTH_URL AU18_OLD_SECRET_FILE AU18_TEST_MINT_KEY_FILE AU18_PAT_FILE \
  AU18_NEW_SECRET_FILE AU18_INTROSPECT_KEY_FILE AU18_FLEET_BUSY_KEY_FILE \
  AU18_OBSERVABILITY_KEY_FILE
scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh --execute --ack-destructive
```

Do not run provider commands by hand in place of the harness. Its read-only
baseline captures the exact Worker version, Containers application version,
and configured image digest, and confirms the application name, singleton
capacity, and remote binding baseline before mutation. The harness uses
`--keep-vars --strict --containers-rollout=immediate` for every recreation;
these exact flags preserve unrelated bindings, reject drift, and recreate the
application from the pinned image digest.

## Gates and proof

The harness pauses intake and redrive and checks authoritative usage,
occupancy, fleet-busy, ledger, admission-pause, and PAT state. When the ledger
is in-memory, the provider creation timestamp must show application age
`>=3900` seconds. After every delete/recreate, re-read the provider creation
timestamp and enforce the same 3900-second age gate before rotation evidence or
any subsequent mutation; a missing, stale, or unavailable timestamp is `RED`.

Immediately before the first permanent credential mutation, the harness arms
the fixed temporary test tenant and performs the fixed canary mint preflight at
`POST /v1/test/mint-cred-ticket`, then the fixed redeem route
`POST /v1/leases/{lease_id}/cas-cred`. It classifies the canary using the
allowlisted diagnostics in the harness (`transport`, HTTP status, contract
code, or `shape`). HTTP `503`, transport failure, malformed/unknown response,
or an unknown cause is `RED`; stop before mutating
`FABRIC_CRED_TICKET_SECRET` and do not claim AU1.8. The canary is bound to the
fixed tenant, repository, installation, and acquiring PAT tuple.

The harness generates the replacement into the absent nofollow-checked OOB
destination, streams secrets only on standard input with tracing disabled, and
keeps the old file for rollback. It performs the forward put and boot reload
under the exclusive lock. Any failure invokes recovery: restore the old secret,
recreate, prove the old signer, revoke/delete the temporary test key, blank the
temporary tenant binding, recreate again, and verify final liveness. Cleanup and
disarm are mandatory on success and every recovery path; a failed revoke,
rollback, disarm, recreation, or final capture is `RED` and requires escalation.

The proof uses only the fixed routes. The old HMAC must be rejected (`401`),
the new HMAC must be accepted (`200`), its returned CLW endpoint must accept
the redeemed PAT probe, and replay must be rejected (`410`). Record only
statuses, allowlisted contract results, timestamps, and correlation handles;
scrub request bodies, headers, tickets, PATs, and secret values.

The harness takes two provider stability samples around each relevant state.
Evidence binds every probe and cleanup state to the sampled Worker version,
container version, application identity, image digest, source commit, and
baseline binding snapshot. A sample mismatch, digest change, unbound version,
or 503 with unknown cause is `RED`; never convert an indeterminate result into
live PASS. The operational window is the harness limit (2400 seconds), and
the evidence records its exact elapsed time and full 40-character source SHA.

## PASS and evidence

PASS requires the harness to report `AU1.8 PASS` after final provider capture,
with old rejection, new acceptance, replay rejection, CLW probe success,
matching version/digest-bound stability samples, singleton/quiescence gates,
temporary key absence, blank temporary tenant binding, and no secret material
in retained output. A source review, selftest, or generated evidence file does
not establish live PASS; this procedure has no production result by itself.

The harness writes only
`docs/plan/evidence/au1.8-fabricd-secret-rotation.json` using `evidence/v1`.
Keep the old rollback file until the owner accepts the evidence. If any gate or
cleanup fails, preserve the scrubbed status log and evidence as `FAILED`, keep
the lock while escalating, and do not delete custody material.

The historical AU503 mint failures are context only. They are not current
proof, a recovery authorization, or a substitute for the current harness
canary classification and version/digest-bound evidence.

## Static references

- [AU1.8 rotation harness](../../scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh)
- [AU1.8 harness selftest](../../scripts/ops/au1.8-fabricd-cred-ticket-rotation.selftest.sh)
- [AU1.8 operational-window selftest](../../scripts/ops/au1.8-fabricd-cred-ticket-rotation-window.selftest.sh)
- [Fabricd Wrangler configuration](../../deploy/cloudflare-fabricd/wrangler.jsonc)
