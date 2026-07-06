// Integration tests for the env-0 MULTI-USE (lease-scoped) cred stash at the DO-wrapper + HTTP-route
// level (clw coordinator env-0 review must-fix #2, 2026-07-05). The PURE
// `decideRedeem` is unit-tested in index.test.ts; here we drive the REAL
// `CredStashDO.stash/redeem` methods against a strongly-consistent storage stub
// (so the wipe-at-expiry it applies is exercised end-to-end) AND the actual
// `POST /v1/leases/{id}/cas-cred` route through the worker fetch handler.
//
// The security claims under test:
//   - MULTI-USE: a stashed cred redeems on every call while the lease is live (the
//     runner needs it for both the boot clw hydrate AND the job clw run); 410 only
//     once the lease expires. The PAT is never handed to the untrusted env/disk.
//   - no-PAT-on-wrong-ticket: a wrong ticket is 401 with NO cred, and does NOT
//     consume the single use (a subsequent correct redeem still succeeds).
//
// `@cloudflare/containers` imports `cloudflare:workers` (Workers-only), so we
// vi.mock it exactly as check-host.test.ts does, purely to make src/index.ts
// importable under node vitest.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

// Import AFTER the mock is registered.
import worker, { CredStashDO, type Env } from "../src/index";
import type { StashedCred } from "../src/lib";

// ── A strongly-consistent DO storage stub (Map-backed). Mirrors the workerd
// DurableObjectStorage subset CredStashDO uses: get/put/delete/deleteAll/setAlarm.
function makeStorage() {
  const map = new Map<string, unknown>();
  const alarms: number[] = [];
  return {
    map,
    alarms,
    async get<T>(key: string): Promise<T | undefined> {
      return map.get(key) as T | undefined;
    },
    async put(key: string, value: unknown): Promise<void> {
      map.set(key, value);
    },
    async delete(key: string): Promise<void> {
      map.delete(key);
    },
    async deleteAll(): Promise<void> {
      map.clear();
    },
    async setAlarm(ms: number): Promise<void> {
      alarms.push(ms);
    },
  };
}

// Instantiate the REAL CredStashDO over a fresh storage stub (the base
// DurableObject stub just assigns this.ctx = ctx).
function makeDO() {
  const storage = makeStorage();
  const ctx = { storage } as never;
  const doInst = new CredStashDO(ctx, {} as never);
  return { doInst, storage };
}

const CRED: StashedCred = {
  token: "per-job-cas-pat",
  endpoint: "https://corelink-api.humangr.com",
  tenant: "srv-derived-tenant",
};
const TICKET = "a".repeat(64);
const TTL_MS = 15 * 60 * 1000;

describe("CredStashDO.redeem — MULTI-USE lease-scoped (DO wrapper)", () => {
  afterEach(() => vi.useRealTimers());

  it("MULTI-USE: a stashed cred redeems on EVERY call while the lease is live", async () => {
    const { doInst } = makeDO();
    await doInst.stash(TICKET, CRED, TTL_MS);

    // The runner redeems for BOTH the boot `clw hydrate` AND the job's `clw run`
    // (corelink-memoize); the cred is served every time until expiry.
    for (let i = 0; i < 3; i++) {
      const r = await doInst.redeem(TICKET);
      expect(r.status).toBe(200);
      expect(r.cred).toEqual(CRED);
    }
  });

  it("wrong ticket ⇒ 401 with NO cred; the correct ticket still redeems", async () => {
    const { doInst } = makeDO();
    await doInst.stash(TICKET, CRED, TTL_MS);

    const wrong = await doInst.redeem("b".repeat(64));
    expect(wrong.status).toBe(401);
    expect(wrong.cred).toBeUndefined(); // the PAT never leaks on a bad ticket

    // A bad ticket doesn't touch the record — the correct ticket still redeems.
    const right = await doInst.redeem(TICKET);
    expect(right.status).toBe(200);
    expect(right.cred).toEqual(CRED);
  });

  it("never-stashed ⇒ 404 (no tombstone)", async () => {
    const { doInst } = makeDO();
    const r = await doInst.redeem(TICKET);
    expect(r.status).toBe(404);
    expect(r.cred).toBeUndefined();
  });

  it("expired ⇒ 410 and the record is wiped (no cred survives past TTL)", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(1_000_000));
    const { doInst, storage } = makeDO();
    await doInst.stash(TICKET, CRED, TTL_MS);
    expect(storage.map.has("rec")).toBe(true);

    // Advance past the TTL.
    vi.setSystemTime(new Date(1_000_000 + TTL_MS + 1));
    const r = await doInst.redeem(TICKET);
    expect(r.status).toBe(410);
    expect(r.cred).toBeUndefined();
    expect(storage.map.has("rec")).toBe(false); // wiped
  });
});

// ── The HTTP route: POST /v1/leases/{id}/cas-cred. Wire a fake CRED_STASH
// namespace whose get(id) returns a REAL CredStashDO keyed by id, so the route +
// DO redeem run together exactly as in prod (byte-identical to fabricd's
// handlers/cas_cred status contract).
function makeEnv(): { env: Env; dos: Map<string, ReturnType<typeof makeDO>> } {
  const dos = new Map<string, ReturnType<typeof makeDO>>();
  const CRED_STASH = {
    idFromName: (name: string) => name,
    get: (id: string) => {
      if (!dos.has(id)) dos.set(id, makeDO());
      return dos.get(id)!.doInst;
    },
  };
  const env = { CRED_STASH } as unknown as Env;
  return { env, dos };
}

function redeemReq(leaseId: string, body: unknown): Request {
  return new Request(`https://w/v1/leases/${leaseId}/cas-cred`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

describe("POST /v1/leases/{id}/cas-cred — route + DO redeem (must-fix #2, route)", () => {
  let env: Env;
  beforeEach(() => {
    ({ env } = makeEnv());
  });

  it("200 returns the CAS PAT on EVERY redeem while the lease is live (multi-use through the route)", async () => {
    // Stash directly on the DO the route will resolve for this lease.
    await env.CRED_STASH.get(env.CRED_STASH.idFromName("job-1")).stash(TICKET, CRED, TTL_MS);

    const expected = {
      cas_pat: "per-job-cas-pat",
      clw_endpoint: "https://corelink-api.humangr.com",
      clw_tenant: "srv-derived-tenant",
      clw_ref_domain: "runner",
    };
    const ok = await worker.fetch(redeemReq("job-1", { ticket: TICKET }), env, {} as never);
    expect(ok.status).toBe(200);
    expect(await ok.json()).toEqual(expected);

    // 2nd redeem (the job's clw run after the boot hydrate) still gets the cred.
    const again = await worker.fetch(redeemReq("job-1", { ticket: TICKET }), env, {} as never);
    expect(again.status).toBe(200);
    expect(await again.json()).toEqual(expected);
  });

  it("wrong ticket ⇒ 401 with NO cas_pat, and the correct ticket still redeems", async () => {
    await env.CRED_STASH.get(env.CRED_STASH.idFromName("job-2")).stash(TICKET, CRED, TTL_MS);

    const bad = await worker.fetch(redeemReq("job-2", { ticket: "c".repeat(64) }), env, {} as never);
    expect(bad.status).toBe(401);
    const badBody = (await bad.json()) as Record<string, unknown>;
    expect(badBody.cas_pat).toBeUndefined(); // the PAT never leaves on a bad ticket

    const good = await worker.fetch(redeemReq("job-2", { ticket: TICKET }), env, {} as never);
    expect(good.status).toBe(200);
    expect((await good.json()).cas_pat).toBe("per-job-cas-pat");
  });

  it("never-stashed lease ⇒ 404", async () => {
    const r = await worker.fetch(redeemReq("job-unknown", { ticket: TICKET }), env, {} as never);
    expect(r.status).toBe(404);
  });

  it("missing ticket ⇒ 400 (before any DO call)", async () => {
    const r = await worker.fetch(redeemReq("job-3", {}), env, {} as never);
    expect(r.status).toBe(400);
  });
});
