# Journey authoring guide (FROZEN CONTRACT — author against this exactly)

You are authoring **story-driven journeys** for the CoreLink Runners e2e suite. A journey is a
NAMED user story, run by a persona (a real tenant PAT), as ordered steps that thread state and
assert the DEFINED behavior at each step, capturing the narrative. Author against the frozen API
and the live-verified contract below. **Do not invent endpoints, fields, or status codes.**

## The Journey framework (import and use exactly)

```js
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro ? false : 'PATs absent (source e2e-prod-env.sh)');

test('JOURNEY · <plain-English user story title>', { skip }, async () => {
  await new Journey('<same title>', { sid: ['S1.2.4'], persona: 'P1 Pro dev', atoms: ['F-4.1','F-5.1'] })
    .step('<what the user does, plain English>', async (ctx) => {
      const a = await acquire(P.pro, { expiryMs: 45000, tmpRoot: '/tmp/e2e-<unique>' });
      ctx.lease = a.leaseId;                                  // thread state
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    // ... more steps, each returning check(condition, 'assertion sentence', {artifact})
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(P.pro, ctx.lease); })  // ALWAYS close what you open
    .run();
});
```

`check(cond, assertion, artifact)` → `{ok, assertion, artifact}`. `ctx` threads state across steps.
The FIRST failing step fails the journey. `onCleanup` ALWAYS runs — **every lease you acquire MUST
be closed in cleanup** (real boxes; never leak them). Use a UNIQUE `tmpRoot` per lease.

## Frozen fabric API (the ONLY verbs — all live-proven)

- `acquire(pat, {image=VALID_IMAGE, netPolicy='deny-all', tmpRoot, expiryMs=60000})` → `{status, leaseId, state, json, text}`. 200 ⇒ a real HELD lease (provisions a box). `state==='held'`.
- `getLease(pat, id)` → `{status, state, json, text}`. 200 for own held lease; **404** for unknown/foreign.
- `listLeases(pat)` → `{status, tenant, leases[], json}`. Tenant-scoped.
- `closeLease(pat, id, status='succeeded')` → `{status, json, text}`. 2xx on close of own held lease (slow — teardown; the verb already uses a 90s timeout). `status` ∈ 'succeeded'|'failed'.
- `usage(pat)` → `{status, tenant, cap, activeNow, json}`. `cap` = plan concurrency cap.
- `req(method, path, {pat, body, headers, timeoutMs})` → `{status, text, json, headers}` for any other read/probe (e.g. `/v1/usage/history`, `/v1/metrics/tenant`, `/v1/attestation/key`).

## Persona → PAT → tenant → cap (live-verified)

| `P.` key | tenant | plan cap | role |
|---|---|---|---|
| `P.free` | f0001 | **1** | Free |
| `P.tenantB` | f0002 | **2** | second tenant |
| `P.pro` | f0004 | **10** | Pro |
| `P.enterprise` | f0006 | **100** | Enterprise |
| `P.solo` | (read it) | (read it) | Solo |
| `P.ro` / `P.rw` / `P.admin` | (read it) | — | role variants |
| `P.tenantBAdmin` | f0002 | 2 | tenant-B admin |

If unsure of a cap/tenant, **read it live** (`usage(pat)`) in step 1 and assert on the read value —
don't hardcode a guess. Never print or embed a PAT value.

## Live-verified endpoint contract (author edge/failure directions to THESE codes)

| Action | Result |
|---|---|
| acquire, valid pinned image (`VALID_IMAGE`) | **200**, lease held |
| acquire, unpinned image (`sha256:…` no repo) | **400** invalid |
| acquire, unsafe `tmp_root` (`"/tmp/x; rm"`) | **400** invalid |
| acquire, relative `tmp_root` | **400** invalid |
| acquire, missing `image_digest` | **422** (schema) |
| acquire, unknown extra field | **422** (deny_unknown) |
| acquire, `expiry_ms: 0` | **200** (accepted) |
| acquire over the tenant cap | **429** (no lease) |
| acquire, no PAT / bad PAT | **401** |
| close own held lease `{status:'succeeded'}` | **2xx** |
| close, no content-type | **415**; close, body missing `status` | **422** |
| close nonexistent lease | **404** |
| get / close a FOREIGN tenant's lease | **404** (no cross-tenant oracle) |
| get nonexistent lease | **404** |
| `/v1/usage`,`/v1/leases`,`/v1/metrics/tenant`,`/v1/usage/history` (authed) | **200**, tenant-scoped |
| any `/v1/*` or `/internal/*` with no PAT | **401** (identical body `{"code":"unauthorized","message":"missing Bearer PAT"}`) |

## HONESTY RULES (non-negotiable — the audit lesson)

1. **A journey asserts BEHAVIOR, never a bare status code.** State transitions, counter/usage deltas, presence/absence, isolation.
2. **No discriminating-control-free claims.** If asserting a rejection (401/404/429), the story should also show the positive case somewhere (e.g. the owner CAN read what a foreigner cannot).
3. **Owner-gated / X4 / planned features:** author a journey that asserts the OBSERVABLE CURRENT reality and RECORDS the residual as a GAP in the artifact — NEVER fake a green for a feature that isn't live. Example: a billing-push story asserts the usage numbers are surfaced and records `pushExporter: armed-off (owner)`.
4. **Every acquired lease is closed in `onCleanup`.** Short `expiryMs` (30–45s) as a backstop.
5. **Do NOT run live** (`E2E_LIVE=1`) — you'd collide with other authors on the shared tenant caps. Verify only that the file PARSES and DRY-skips clean: `node --test <yourfile>` with no `E2E_LIVE` must show all-skipped, 0 fail. The tech lead runs every journey live, serially, and audits it before merge.
6. Multiple journeys per file; one file per your assigned cluster. Map each journey to its `sid`(s).

## Direction coverage per story

For each story in your cluster, author the directions its nature admits: **happy** (it works),
**edge** (boundary: at-cap, zero-expiry, empty list), **failure** (it breaks: over-cap, bad input,
nonexistent), **adversarial** (someone attacks: cross-tenant, no-PAT, forged). Aim for multiple
journeys per story where the directions are distinct — this is how coverage scales 40×.
