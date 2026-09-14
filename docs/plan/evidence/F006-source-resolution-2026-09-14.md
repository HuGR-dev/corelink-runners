# F-20260905-006 source resolution

F-20260905-006 is source-resolved in the final B2 tree (`d733c440`, composed by `d50ac366`). The credential obligation authority persists exact `(jobId, tenant, patId)` identities in transactional Durable Object storage. A durable completed-job fence prevents stale KV absence or an old receipt from closing a newer credential, and registration after the fence commits a `revoke_requested` obligation before rejecting the caller. Confirmed records remain terminal, while healthy registered credentials remain excluded from retry.

Focused verification ran in a detached worktree from `d50ac366`:

```
deploy/cloudflare/node_modules/.bin/vitest run \
  test/credential-revocation.test.ts test/suspension-revocation.test.ts \
  --pool=threads --poolOptions.threads.singleThread=true --reporter=verbose
```

Result: **2 test files passed, 18 tests passed**.

The covered cases include independent A/B credentials for one job, stale KV and old receipt isolation, concurrent replacement, transactional rollback and serialization, durable fence/restart recovery, missing mint key, crash after fencing, malformed authority records, tenant attribution conflict before HTTP, bounded 101-key pagination, healthy-job isolation, legacy inventory refusal, and terminal exact-identity rejection. No live/provider qualification is claimed.

F-20260905-008 remains open and blocks overall T8-W5 delivery for suspension epochs, migration coverage and ordered unsuspend/resuspend lifecycle behavior. This evidence closes only the F-006 source component.
