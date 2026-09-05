import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn(() => ({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) })) }));

import { ContainmentDO, REDRIVE_RESERVATION_TTL_MS, redriveOrphanedJobs, retryOrphanedSpawns, runContainmentDrain } from "../src/index";
import { authorityProxy, bootstrap, ctx, digest, env, envWithAuthority, event, makeDO, kv, providerReceipt, recoveryFixture, reserveKey, settle, T0, writeDeliveredProof } from "./containment-redrive-test-helpers";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T0); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
describe("T3-W17 redrive gate and proof failure boundaries", () => {
  it("keeps the head CLAIMED for each divergent or nonterminal recovery witness", async () => {
    const corruptions: Array<(records: Record<string, unknown>[]) => void> = [
      (records) => { records[0].source_value = "different claim"; },
      (records) => { records[1].permit_id = "wrong-permit"; },
      (records) => { records[2].event_id = "wrong-event"; },
      (records) => { records[3].source_sha256 = "0".repeat(64); },
      (records) => { records[4].terminal = "PENDING"; },
      (records) => { records[4].attempt_count = 2; },
    ];
    for (const corrupt of corruptions) {
      const { instance, effect, records } = await recoveryFixture(corrupt);
      // This is the digest of the altered canonical witness bytes, not a stale
      // digest from the valid fixture. Rejection therefore proves content
      // validation, rather than merely a caller-digest mismatch.
      const alteredDigest = await digest(JSON.stringify(records.map((record) => JSON.stringify(record))));
      expect(await instance.recoverEffectCommitted(effect, "new", 2, alteredDigest)).toBe(false);
      expect(await instance.snapshot()).toMatchObject({ drain_cursor: 0, backlog_count: 1, lease_epoch: 2 });
      expect((await instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", claim: { owner: "new", lease_epoch: 2 } });
      expect((await instance.claimNext("new", 2, T0 + 120_000))?.event.event_id).toBe("evt-1");
    }
  });

  it("does not recover an unresolved permit with no immutable proof", async () => {
    const d = makeDO(); await bootstrap(d, "1"); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: kv() } as never);
    await instance.append(event(1)); await instance.acquireLease("old", T0); await instance.claimNext("old", 1, T0);
    expect(await instance.beginEffect("evt-1", "old", 1, T0)).not.toBeNull();
    await instance.acquireLease("new", T0 + 120_000); await instance.claimNext("new", 2, T0 + 120_000);
    expect(await instance.recoverEffectCommitted("containment:v1:evt-1", "new", 2, "0".repeat(64))).toBe(false);
    expect((await instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", claim: { owner: "new", lease_epoch: 2 } });
  });

  it("suppresses first-party GitHub listing under paused and invalid redrive states", async () => {
    for (const value of ["1", "bogus", " "]) {
      const d = makeDO();
      const fetchSpy = vi.fn(async () => new Response(JSON.stringify({ workflow_runs: [] }), { status: 200 }));
      vi.stubGlobal("fetch", fetchSpy);
      await redriveOrphanedJobs(env(d, kv(), { AUTOSCALER_REDRIVE_PAUSED: value, RECONCILER_REPOS: "acme/repo" }), ctx() as never, undefined, T0);
      expect(fetchSpy).not.toHaveBeenCalled();
    }
  });

  it("rejects malformed or divergent orphan authorization before reservation or mutation", async () => {
    const valid = { repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 };
    const badRecords: Array<{ key: string; record: unknown; map?: string }> = [
      { key: "orphan:123", record: {} },
      { key: "orphan:0", record: valid }, { key: "orphan:01", record: valid }, { key: "orphan:9007199254740992", record: valid }, { key: "orphan:12x", record: valid },
      { key: "orphan:123", record: { ...valid, repo: "acme" } }, { key: "orphan:123", record: { ...valid, repo: "acme/repo/extra" } },
      { key: "orphan:123", record: { ...valid, repo: "acme/other", installationId: "42" }, map: JSON.stringify({ "acme/other": "43" }) },
      { key: "orphan:123", record: { ...valid, labels: ["other"] } }, { key: "orphan:123", record: { ...valid, labels: ["corelink", "other"] } }, { key: "orphan:123", record: { ...valid, labels: ["corelink", "corelink"] } },
      { key: "orphan:123", record: { ...valid, labels: [null] } }, { key: "orphan:123", record: { ...valid, labels: [42] } }, { key: "orphan:123", record: { ...valid, labels: [{}] } },
      { key: "orphan:123", record: { ...valid, installationId: "999" } }, { key: "orphan:123", record: { ...valid, installationId: "" } }, { key: "orphan:123", record: { ...valid, labels: "corelink" } },
    ];
    for (const { key, record, map } of badRecords) {
      const d = makeDO();
      const store = kv({ [key]: JSON.stringify(record) });
      const drive = vi.fn(async () => {});
      await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", ...(map ? { REPO_INSTALLATION_MAP: map } : {}) }), ctx() as never, T0, drive, vi.fn(async () => null));
      expect(drive).not.toHaveBeenCalled();
      expect(store.put).not.toHaveBeenCalled();
      expect(store.delete).not.toHaveBeenCalled();
      expect(d.storage.map.has(reserveKey())).toBe(false);
    }
  });

  it("does not retry a post-eligibility reservation after a redrive failure", async () => {
    const d = makeDO(); await bootstrap(d, "123");
    const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1 }) });
    const failingDrive = vi.fn(async () => { throw new Error("crash after eligibility"); });
    await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), ctx() as never, T0, failingDrive, vi.fn(async () => null));
    expect(failingDrive).toHaveBeenCalledTimes(1);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
    await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), ctx() as never, T0 + 10 * REDRIVE_RESERVATION_TTL_MS, failingDrive, vi.fn(async () => null));
    expect(failingDrive).toHaveBeenCalledTimes(1);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
  });
});

describe("T3-W17 deterministic continuation crash seams", () => {
  async function queuedDrain() {
    const store = kv(); const d = makeDO({ RUNNER_JOB_PATS: store });
    await bootstrap(d, "1");
    await d.instance.append(event(1));
    return { d, store };
  }

  it("leaves durable state at every named drain crash boundary", async () => {
    {
      const { d, store } = await queuedDrain();
      const authority = authorityProxy(d.instance, { claimNext: async () => { throw new Error("before claim"); } });
      await expect(runContainmentDrain(envWithAuthority(d, store, authority))).rejects.toThrow("before claim");
      expect((await d.instance.getEvent("evt-1"))?.state).toBe("QUEUED");
    }
    {
      const { d, store } = await queuedDrain();
      const drive = vi.fn(async () => providerReceipt({ jobId: "1", repo: "acme/repo" }));
      const authority = authorityProxy(d.instance, { beginEffect: async () => { throw new Error("after claim before permit"); } });
      await runContainmentDrain(envWithAuthority(d, store, authority), { driveSpawn: drive });
      expect((await d.instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", effect_permit: null });
      expect([...d.storage.map.values()]).toContainEqual(expect.objectContaining({ path: "drain", state: "UNKNOWN", permit_id: null, effect_started: false }));
      expect(drive).not.toHaveBeenCalled();
    }
    {
      const { d, store } = await queuedDrain();
      await runContainmentDrain(env(d, store), { claimSpawn: async () => { throw new Error("after permit"); } });
      expect((await d.instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", effect_permit: null });
      expect(store.map.has("spawn:1")).toBe(false);
    }
    {
      const { d, store } = await queuedDrain();
      const claim = vi.fn(async () => { await store.put("spawn:1", "123"); return true; });
      const bind = vi.fn(async () => { throw new Error("after claim KV"); });
      await runContainmentDrain(env(d, store), { claimSpawn: claim, bindContainmentSpawnClaim: bind });
      expect(claim).toHaveBeenCalledTimes(1); expect(bind).toHaveBeenCalledTimes(1); expect(store.map.get("spawn:1")).toBe("123");
      expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
      expect([...d.storage.map.values()]).toContainEqual(expect.objectContaining({ path: "drain", state: "DRIVING" }));
    }
    {
      const { d, store } = await queuedDrain();
      const bind = vi.fn(async () => {}); const drive = vi.fn(async () => { throw new Error("after bind"); });
      await runContainmentDrain(env(d, store), { claimSpawn: async () => true, bindContainmentSpawnClaim: bind, driveSpawn: drive });
      expect(bind).toHaveBeenCalledTimes(1); expect(drive).toHaveBeenCalledTimes(1);
      expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
      expect([...d.storage.map.values()]).toContainEqual(expect.objectContaining({ path: "drain", state: "DRIVING" }));
    }
    {
      const { d, store } = await queuedDrain();
      const authority = authorityProxy(d.instance, { markEffectCommitted: async () => { throw new Error("after drive before commit"); } });
      const drive = vi.fn(async (_env: unknown, opts: { jobId: string; repo: string }) => providerReceipt(opts));
      await runContainmentDrain(envWithAuthority(d, store, authority), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: drive });
      expect(drive).toHaveBeenCalledTimes(1); expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
      expect([...d.storage.map.values()]).toContainEqual(expect.objectContaining({ path: "drain", state: "COMMITTED" }));
    }
    {
      const { d, store } = await queuedDrain();
      const authority = authorityProxy(d.instance, { acknowledge: async () => { throw new Error("after commit before ack"); } });
      const drive = vi.fn(async (_env: unknown, opts: { jobId: string; effect_id?: string; containment_event_id?: string; effect_permit_id?: string }) => {
        await writeDeliveredProof(store, { jobId: opts.jobId, effect_id: opts.effect_id!, containment_event_id: opts.containment_event_id!, effect_permit_id: opts.effect_permit_id! });
        return providerReceipt({ jobId: opts.jobId, repo: "acme/repo" });
      });
      await runContainmentDrain(envWithAuthority(d, store, authority), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: drive as never });
      expect(drive).toHaveBeenCalledTimes(1); expect((await d.instance.getEvent("evt-1"))?.state).toBe("EFFECT_COMMITTED");
      expect(await d.instance.snapshot()).toMatchObject({ drain_cursor: 0, backlog_count: 1 });
    }
  });

  it("allows only the deferred old permit holder into the continuation after lease reclaim", async () => {
    const { d, store } = await queuedDrain();
    let releaseOld: (() => void) | undefined; let signalEntered: (() => void) | undefined;
    const oldGate = new Promise<void>((resolve) => { releaseOld = resolve; });
    const entered = new Promise<void>((resolve) => { signalEntered = resolve; });
    let enteredOld = 0; const enteredNew = vi.fn(async (_env: unknown, opts: { jobId: string; repo: string }) => providerReceipt(opts));
    const oldRun = runContainmentDrain(env(d, store), {
      claimSpawn: async () => true,
      bindContainmentSpawnClaim: async () => {},
      driveSpawn: async (_env, opts) => { enteredOld++; signalEntered!(); await oldGate; return providerReceipt(opts); },
    });
    await entered;
    expect(enteredOld).toBe(1);
    vi.setSystemTime(T0 + 120_000);
    await runContainmentDrain(env(d, store), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: enteredNew });
    expect(enteredNew).not.toHaveBeenCalled();
    expect((await d.instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", effect_permit: expect.any(Object) });
    releaseOld!(); await oldRun;
    expect(enteredOld).toBe(1);
  });

  it("recovers a proven permit through the real drain without a second continuation entry", async () => {
    const store = kv(); const d = makeDO({ RUNNER_JOB_PATS: store });
    await bootstrap(d, "1");
    await d.instance.append(event(1)); await d.instance.acquireLease("old", T0); await d.instance.claimNext("old", 1, T0);
    const permit = await d.instance.beginEffect("evt-1", "old", 1, T0); expect(permit).not.toBeNull();
    await writeDeliveredProof(store, { jobId: "1", effect_id: "containment:v1:evt-1", containment_event_id: "evt-1", effect_permit_id: permit!.permit_id });
    vi.clearAllMocks();
    const claim = vi.fn(async () => true); const bind = vi.fn(async () => {}); const drive = vi.fn(async () => {});
    vi.setSystemTime(T0 + 120_000);
    await runContainmentDrain(env(d, store), { claimSpawn: claim, bindContainmentSpawnClaim: bind, driveSpawn: drive });
    expect(claim).not.toHaveBeenCalled(); expect(bind).not.toHaveBeenCalled(); expect(drive).not.toHaveBeenCalled();
    expect(await d.instance.getEvent("evt-1")).toBeNull();
    expect(await d.instance.snapshot()).toMatchObject({ drain_cursor: 1, backlog_count: 0, drain_requested: false });
  });

  it("clears drain_requested only on the final ACK and leaves a repeated empty drain empty", async () => {
    const store = kv(); const d = makeDO({ RUNNER_JOB_PATS: store }); const instance = d.instance;
    await bootstrap(d, "1"); await bootstrap(d, "2");
    await instance.append(event(1)); await instance.append(event(2));
    expect(await instance.requestDrain()).toMatchObject({ drain_requested: true, backlog_count: 2 });
    await instance.acquireLease("owner", T0); await instance.claimNext("owner", 1, T0);
    const firstPermit = await instance.beginEffect("evt-1", "owner", 1, T0); expect(firstPermit).not.toBeNull();
    await writeDeliveredProof(store, { jobId: "1", effect_id: "containment:v1:evt-1", containment_event_id: "evt-1", effect_permit_id: firstPermit!.permit_id });
    expect(await instance.markEffectCommitted("evt-1", "owner", 1)).toBe(true);
    expect(await instance.acknowledge("evt-1", "owner", 1)).toBe(true);
    expect(await instance.snapshot()).toMatchObject({ drain_cursor: 1, backlog_count: 1, drain_requested: true });
    await instance.claimNext("owner", 1, T0);
    const secondPermit = await instance.beginEffect("evt-2", "owner", 1, T0); expect(secondPermit).not.toBeNull();
    await writeDeliveredProof(store, { jobId: "2", effect_id: "containment:v1:evt-2", containment_event_id: "evt-2", effect_permit_id: secondPermit!.permit_id });
    expect(await instance.markEffectCommitted("evt-2", "owner", 1)).toBe(true);
    expect(await instance.acknowledge("evt-2", "owner", 1)).toBe(true);
    expect(await instance.snapshot()).toMatchObject({ drain_cursor: 2, backlog_count: 0, drain_requested: false });
    expect(await instance.requestDrain()).toMatchObject({ drain_requested: false, backlog_count: 0, drain_cursor: 2 });
  });
});
