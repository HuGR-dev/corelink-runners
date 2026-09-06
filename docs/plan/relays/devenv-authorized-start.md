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

Failed start, rejected acknowledgement, or unusable issued grant triggers the existing runner revoke function using a server-owned consumer credential, with at most two attempts. Failure responses remain generic. If both revocations fail, the fixed `devenv_start_revoke_failed` diagnostic is emitted. **Durable recovery of that server-side revocation obligation is still required for a no-debt release:** this patch does not invent a persistent queue or claim cleanup succeeded. The minted PAT's expiry is the existing backstop. An issuer response that omits the PAT id cannot be revoked by this relay and must remain an issuer-contract failure.

## Validation and release boundary

Performed only in `/private/tmp/corelink-server-devenv-relay-review-20260905`, an archive copy with links to the unchanged installed dependencies:

- `vitest run tests/devenv_relay.test.ts`: **32 focused tests passed**, 1.65 seconds reported by Vitest. These exercise the relay helper and scope/path policy; they do not simulate actual Clerk/PAT authentication or a deployed Durable Object.
- Strict targeted TypeScript passed for the new relay helper and RPC interface, plus a small namespace/stub call fixture against the actual installed Cloudflare types. The full Worker and UI typechecks were not run.
- Git whitespace and reverse-apply checks passed; the final patch is checked against the target source files before delivery.

No CI, full suite, deployment, provider call, credential use, or sibling source mutation was performed. The enclosing sprint owns integration and full validation. Deployment must account for both repositories and each environment's binding configuration; cross-repository deployment is not atomic. Missing RPC/binding must fail closed, with no fallback to raw-token start. Do not count the artifact, targeted tests, or a default-environment binding as a released production capability.
