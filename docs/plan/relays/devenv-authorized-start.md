# DevEnv authorized-start relay artifact

Target: `corelink-server` commit `c0a4466d930073d05ebedc3ac8b8d9cc8474b68f`, verified 2026-09-05. The companion file is a plain Git patch, not a purported upstream commit or published release. It was prepared against an archive copy; the sibling checkout was read only. This artifact does not deliver T9-W1 by itself.

The existing server authenticates PAT or Clerk, derives the tenant, and forwards the browser's `clw_token` body to `RunnerDevEnvDO`. The patch removes that UI field and body type, adds the missing cross-worker binding, and replaces every normalized start request with a typed `startAuthorizedDevenv` RPC. HTTP headers cannot grant access to that RPC. Non-start requests continue over the existing trusted binding.

## Authorization and issuance

- PAT identity uses the actual `parsePat` field `tokenId`, after signature and persisted-row verification. Clerk identity uses the verified `clerkUserId`. These stable identities determine mint throttling; a fresh session UUID cannot bypass it.
- Only explicit `admin` or `read-write` PAT scopes permit writes. Runner-narrowed PATs cannot derive broader DevEnv credentials. Clerk owner/admin/member roles permit writes; viewer permits status/list only; unknown roles/scopes fail closed. WebSocket upgrades and GET execution paths require write access.
- The route matcher checks a complete prefix boundary. Starts include both API aliases and trailing slashes, matching the existing DO normalization. All other request fields beyond `workspace_name`, `profile_name`, and `tier` are rejected before mint, including credential aliases and nested grants. Null, arrays, malformed JSON, invalid names, and unsupported tiers fail closed.
- The existing entitlement check gates starts. Authenticated cleanup does not depend on quota availability. Failed quota reads produce a generic failure.
- `MintGrant.fromDevenvSession` uses the existing mint authority to issue `cas:rw` for the verified tenant. Nonempty PAT data, non-nil UUID identities, matching tenant, integer future expiry, and an eight-hour upper bound are checked before RPC. The grant deadline is also clamped to eight hours from relay entry.

## Paired runners contract

The cross-worker type extends `Rpc.DurableObjectBranded`, as required by the installed Cloudflare types. Its sole start method is:

```ts
startAuthorizedDevenv({
  config: { workspaceName, profileName, tier },
  grant: { tenantId, sessionUuid, casPat, patId, expiresAtMs }
}): Promise<{ sessionUuid, status: "starting" | "running" }>
```

The server verifies the acknowledgement's session and returns only those two fields. Arbitrary RPC fields, errors, and the minted PAT are never reflected in the start response. The paired runners implementation must validate the grant, issue its own stash ticket, prohibit the old HTTP/raw-token start, and own stop/error/restart cleanup. That implementation and its acceptance evidence remain separate, pending work; this artifact does not claim them complete.

## Durable credential handoff (F-20260905-004)

The container mint endpoint is not idempotent, but only computes the plaintext and persistable hash. `mintScopedPat` activates the credential by inserting its D1 `pat` row. The patch adds migration `0107_devenv_credential_obligation.sql` and makes DevEnv activation insert that PAT row and record its exact operation/PAT/token IDs in **one D1 batch transaction**. Other mint consumers retain their existing persistence path.

Before invoking mint, the relay generates the session UUID and calls the existing `_system` CoreLinkServer DO through its internal binding. Its new cleanup preparation route requires the existing `runner_mint` consumer authority and verifies the addressed DO is `_system`. It checks the migration, durably records a tenant-bound operation marker and alarm in a storage transaction, then creates a deadline-fenced D1 intent. Missing schema, unavailable storage/alarm, unavailable internal authority, or the 64-marker capacity limit refuses issuance before mint. This route arms cleanup only; it cannot start a DevEnv and is not an alternative public grant endpoint.

The existing CoreLinkServer alarm drains at most four due markers per invocation, using persisted backoff capped at five minutes. It keeps pending credential work armed even when the container is dead or idle, without booting it. Newly prepared work arms its own wakeup; the drain never deletes an alarm after observing an empty list. Pending obligations survive adapter/isolate restart and D1/KV failure.

The D1 operation state is monotonic for this protocol: prepared → issued → adopted, or prepared/issued → revoking → revoked. A validated matching RPC acknowledgement is required to mark adoption. Revocation atomically fences the operation and revokes its PAT; cache invalidation must complete before the operation becomes revoked. Failed immediate cleanup leaves the existing durable marker for the alarm. Raw PAT plaintext is absent from both the obligation table and DO marker.

Closed `adopted`/`revoked` rows are retained as idempotency tombstones. In particular, cleanup of an absent intent creates a revoked tombstone, so an unusually late prepare cannot recreate untracked work after its alarm was retired. A late mint cannot activate against an expired, revoked, revoking, or adopted operation. The code does not assume a timed-out remote mutation was cancelled. Tombstones are protocol history, not pending retries; deleting them requires a separately proven safe retention boundary.

Every added remote wait has an explicit deadline (normally five seconds; mint HTTP twenty seconds, the whole mint promise twenty-five, and the runners start RPC twenty seconds). The prepare activation window is ninety seconds, with an alarm grace of two seconds. The paired runners author confirmed that the RPC budget accommodates its bounded stash/start steps. Late RPC completion can lose availability after revocation, but cannot erase the cleanup obligation or reactivate the credential. Only confirmed adoption or revocation retires a DO marker.

This repairs the **authored server artifact** for F-20260905-004. Review, upstream application, migration, paired deployment, and real D1/DO acceptance remain required before closing the finding as delivered.

## Validation and release boundary

Performed only in `/private/tmp/corelink-server-devenv-relay-review-20260905`, an archive copy with links to the unchanged installed dependencies:

- `vitest run tests/devenv_cleanup.test.ts tests/devenv_relay.test.ts`: **45 focused tests passed**, 7.55 seconds reported by Vitest. These cover helper authorization, body validation, mint/RPC timeouts, late acknowledgements, durable prepare failure, bounded drain/capacity, and restart/KV-failure recovery. They do not simulate actual Clerk/PAT authentication or a deployed Durable Object.
- One Vitest case invokes `python3 worker/tests/devenv_cleanup_sql.py`: **nine SQLite cases** execute the actual production SQL extracted from the helper against the checked-in PAT and obligation migrations. They cover transaction rollback, exact activation/obligation binding, expired/aborted/duplicate issuance, missing-intent tombstones, idempotent revoke, and adoption races. Python 3 is required for this focused test.
- Strict targeted TypeScript passed for both new helpers and the RPC interface, plus a namespace/stub call fixture against the actual installed Cloudflare types. The full Worker and UI typechecks were not run.
- Git whitespace and reverse-apply checks passed. The final patch is also checked against fresh exact target source files before delivery.

No CI, full suite, deployment, provider call, credential use, or sibling source mutation was performed. The enclosing sprint owns integration and full validation. Deployment must account for both repositories and each environment's binding configuration; cross-repository deployment is not atomic. Missing RPC/binding must fail closed, with no fallback to raw-token start. Do not count the artifact, targeted tests, or a default-environment binding as a released production capability.
