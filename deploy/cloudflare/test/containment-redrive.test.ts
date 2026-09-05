import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn(() => ({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) })) }));

import { ContainmentDO } from "../src/index";
import { digest, event, bootstrap, makeDO, kv, T0 } from "./containment-redrive-test-helpers";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T0); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("ordered queue / lease / immutable evidence recovery", () => {
  it("claims strictly in order, issues one permit, and fences stale leases", async () => {
    const d = makeDO(); await bootstrap(d, "1"); await bootstrap(d, "2"); await d.instance.append(event(1)); await d.instance.append(event(2));
    const a = await d.instance.acquireLease("owner-a"); expect(a).toEqual({ owner: "owner-a", epoch: 1, expires_ms: T0 + 120_000 });
    expect(await d.instance.acquireLease("owner-b")).toBeNull();
    const head = await d.instance.claimNext("owner-a", 1); expect(head?.event.pause_seq).toBe(1); expect(head?.committed).toBe(false);
    const permit = await d.instance.beginEffect("evt-1", "owner-a", 1); expect(permit?.issued_to_owner).toBe("owner-a"); expect(await d.instance.beginEffect("evt-1", "owner-a", 1)).toEqual(permit);
    expect(await d.instance.markEffectCommitted("evt-1", "owner-a", 1)).toBe(false);
    expect(await d.instance.claimNext("owner-b", 1)).toBeNull();
    expect(await d.instance.acknowledge("evt-1", "owner-b", 1)).toBe(false);
  });
  it("renews at the exact 30-second window and reclaims with a new epoch", async () => {
    const d = makeDO(); const first = await d.instance.acquireLease("owner-a");
    expect(await d.instance.renewLease("owner-a", 1, T0 + 89_999)).toBe(true);
    const before = await d.instance.snapshot(); expect(before.lease?.expires_ms).toBe(T0 + 120_000);
    expect(await d.instance.renewLease("owner-a", 1, T0 + 90_000)).toBe(true);
    expect((await d.instance.snapshot()).lease?.expires_ms).toBe(T0 + 210_000);
    const next = await d.instance.acquireLease("owner-b", T0 + 210_000); expect(next?.epoch).toBe(2); expect(first?.epoch).toBe(1);
    expect(await d.instance.renewLease("owner-a", 1, T0 + 210_001)).toBe(false);
  });
  it("recovers an eligible head from canonical immutable evidence and acknowledges it", async () => {
    const d = makeDO(); await bootstrap(d, "1"); const store = kv(); const withKv = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
    await withKv.append(event(1)); await withKv.acquireLease("owner-a"); await withKv.claimNext("owner-a", 1); const permit = await withKv.beginEffect("evt-1", "owner-a", 1); expect(permit).not.toBeNull();
    const effect = "containment:v1:evt-1"; const evidence = [["spawn_claim", "spawn:1", "123"], ["attempt", "github:generate-jitconfig", JSON.stringify({ runner_name: "runner", runner_id: 9, attempt: 1 })], ["placement", "orphan:1", JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0, placedMs: T0 + 1, effect_id: effect })], ["lease", "jhandle:1", "handle-1"]] as const;
    const records: Record<string, unknown>[] = [];
    for (const [kind, sourceKey, sourceValue] of evidence) { const rec = { schema_version: 1, kind, effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: sourceKey, source_value: sourceValue, source_sha256: await digest(sourceValue), ...(kind === "attempt" ? { attempt_count: 1 } : {}) }; records.push(rec); await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:${kind}`, JSON.stringify(rec)); }
    const resultSource = JSON.stringify({ terminal: "DELIVERED", attempt_count: 1, spawn_claim_sha256: (records[0] as { source_sha256: string }).source_sha256, attempt_sha256: (records[1] as { source_sha256: string }).source_sha256, placement_sha256: (records[2] as { source_sha256: string }).source_sha256, lease_sha256: (records[3] as { source_sha256: string }).source_sha256 });
    const result = { schema_version: 1, kind: "result", effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: "result", source_value: resultSource, source_sha256: await digest(resultSource), terminal: "DELIVERED", attempt_count: 1 };
    await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:result`, JSON.stringify(result));
    expect(await withKv.acquireLease("owner-b", T0 + 120_000)).toMatchObject({ epoch: 2 }); const claimed = await withKv.claimNext("owner-b", 2); expect(claimed?.committed).toBe(false);
    expect(await withKv.recoverEffectCommitted(effect, "owner-b", 2, await digest([...records, result].map((r) => JSON.stringify(r)).join()))).toBe(false);
    expect(await withKv.recoverEffectCommitted(effect, "owner-b", 2, await digest(JSON.stringify([...records, result].map((r) => JSON.stringify(r)))))).toBe(true);
    expect(await withKv.acknowledge("evt-1", "owner-b", 2)).toBe(true);
  });
  it("fails closed on malformed schema or tampered evidence", async () => {
    const d = makeDO(); await bootstrap(d, "1"); await bootstrap(d, "123"); const store = kv(); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
    await instance.append(event(1)); await instance.acquireLease("a"); await instance.claimNext("a", 1); const permit = await instance.beginEffect("evt-1", "a", 1); expect(permit).not.toBeNull(); await instance.markEffectCommitted("evt-1", "a", 1);
    expect(await instance.recoverEffectCommitted("containment:v1:evt-1", "a", 1, "0".repeat(64))).toBe(false);
    d.storage.map.set("containment:v1:reservation:acme/repo/123", { schema_version: 2, repo: "acme/repo", job_id: "123" }); expect((await instance.reserveRedriveCandidate("acme/repo", "123")).status).toBe("busy");
  });
});
