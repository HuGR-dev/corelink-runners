# T8-W5 R2 — durable completion fence
Root D0 decision: implement existing job completion safety now, on the composed authority. No product policy or new infrastructure dependency is needed for this repair. Canonical T8-W5 remains partial until suspension epochs/migration and sprint acceptance close.

Write ONLY:
- deploy/cloudflare/src/lib/credential_obligation_authority.ts
- deploy/cloudflare/src/lib/revocation_outbox.ts
- deploy/cloudflare/test/suspension-revocation.test.ts
- deploy/cloudflare/test/credential-revocation.test.ts
Read boundary credential_authority_contract.ts and ContainmentDO wrappers in index.ts; root owns both. Root has added closeJobCredentials(jobId): Promise<{known:boolean}>.

Frozen state/behavior:
1. Job identity is terminal for its lifetime; a new actual job requires a different jobId. Persist schema-versioned permanent job fence under separate encoded job key, using authority transaction, never KV/TTL. Validate input and persisted fence shape.
2. closeJobCredentials commits fence even for unknown job. Determine known from actual validated exact-job obligation history (including revoked), bounded prefix query inside same transaction; no global scan or unbounded transaction. Exact encoded job prefix ends with colon. Empty unknown may not be acknowledged. Repeated close with history is idempotent.
3. Fence is a durable revocation request for ALL that job's non-revoked obligations. revocationRequestedCredentials must return registered records whose job is fenced as well as explicit requested ones. Preserve bounded 101-key cursor semantics and validate fence records. Thus crash immediately after close does not strand registered credentials. Healthy unfenced jobs remain excluded.
4. registerCredential checks fence atomically with exact identity. On fenced job, write new identity as revoke_requested (or promote registered); preserve revoked terminal record; THEN throw outside committed transaction. Re-registering a requested/revoked exact identity also rejects start even without fence. Existing healthy registered identity remains idempotent. Never roll back the cleanup obligation by throwing inside its insertion transaction.
5. revokeCompletedJob closes BEFORE checking mint key or external effects, enumerates all pending records with existing bounds, validates ALL derivedTenant conflicts before any HTTP call. Missing key with pending identities throws pending error and durable fence remains. No history throws unknown; empty pending with known durable history returns true idempotently. All successful HTTP confirmations required before success. Concurrent registration may be left durably requested for scheduled retry but must reject caller start; completion snapshot does not claim no future cleanup work exists.
6. Preserve exact-PAT HTTP/confirmation ordering, KV compatibility projection, existing tenant-suspension behavior. Do not claim suspension epoch fix.

Focused verification only: suspension-revocation.test.ts and credential-revocation.test.ts, single Vitest worker. Add tests proving duplicate completion no repeated HTTP; close with missing key then actual authority restart and cron retry; registration while HTTP pending rejects but survives rollback-capable transaction and retries; crash immediately after fence before enumeration; healthy job unaffected; unknown legacy refuses; malformed fence refuses; tenant conflict zero HTTP; terminal exact identity cannot restart. Use serialized transaction fixture with rollback semantics for concurrency evidence, not plain map callback masquerading as atomic transactions.

First run meaningful new regressions against existing behavior, then implement. No full CI, Cargo, deploy, push, other repo changes, or edits outside allowlist. Reuse node_modules symlink from /private/tmp/corelink-sprint1-cleanup-next/deploy/cloudflare/node_modules. Commit with DCO, [skip ci], Co-Authored-By: Codex <noreply@openai.com>. Return exact SHA, changed files, focused result and residual findings. Stop after bounded component return; root continues orchestration.
