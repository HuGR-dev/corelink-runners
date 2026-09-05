import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn(() => ({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) })) }));

import worker, { ContainmentDO, REDRIVE_RESERVATION_TTL_MS, containmentEffectPointerKey, redriveOrphanedJobs, retryOrphanedSpawns, type ContainmentRedriveReservation } from "../src/index";
import { canonicalSafeJobId, redriveEffectId } from "../src/containment_authority_helpers";
import { authorityProxy, bootstrap, ctx, env, envWithAuthority, event, makeDO, kv, reserveKey, settle, T0, webhook, writeDeliveredProof } from "./containment-redrive-test-helpers";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T0); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
describe("atomic redrive reservation state machine", () => {
  it("canonical job ids reject zero, leading zeroes, whitespace, signs, fractions, and exponents", () => {
    for (const value of ["0", "01", " 1", "1 ", "+1", "-1", "1.0", "1e3", "9007199254740992", "9007199254740991x"]) {
      expect(canonicalSafeJobId(value)).toBeNull();
    }
    expect(canonicalSafeJobId("1")).toBe("1");
    expect(canonicalSafeJobId("9007199254740991")).toBe("9007199254740991");
  });

  it("uses the repo-job active index schema and supports 100 active events", async () => {
    const d = makeDO();
    await d.instance.bootstrapContainedEventIndex("acme/repo", "1");
    for (let i = 0; i < 100; i++) await d.instance.append(event(1, { event_id: `evt-index-${i}`, effect_id: `containment:v1:evt-index-${i}` }));
    expect(d.storage.map.get("containment:v1:repo-job-index:acme/repo/1")).toMatchObject({ schema_version: 1, repo: "acme/repo", job_id: "1", active_count: 100, active_event_ids: expect.any(Array), updated_at_ms: expect.any(Number) });
    expect((d.storage.map.get("containment:v1:repo-job-index:acme/repo/1") as { active_event_ids: string[] }).active_event_ids).toHaveLength(100);
  });

  it("admin drain bootstraps index metadata atomically before the next append", async () => {
    const d = makeDO();
    await d.instance.requestDrain();
    expect(d.storage.map.get("containment:v1:job-index-meta")).toEqual({ schema_version: 1, initialized: true });
    await expect(d.instance.append(event(1))).rejects.toThrow("containment job index missing");
    expect(await d.instance.bootstrapContainedEventIndex("acme/repo", "1")).toEqual({ status: "bootstrapped" });
    expect((await d.instance.append(event(1))).status).toBe("appended");
  });

  it("does not allocate a missing pair during direct append or redrive admission", async () => {
    const d = makeDO();
    await expect(d.instance.append(event(1))).rejects.toThrow("containment job index missing");
    await expect(d.instance.reserveRedriveCandidate("acme/repo", "1", T0)).rejects.toThrow("containment job index missing");
    expect(d.storage.map.has("containment:v1:repo-job-index:acme/repo/1")).toBe(false);
  });

  it("requires the pair marker before duplicate append or acknowledgement mutation", async () => {
    const d = makeDO(); await d.instance.bootstrapContainedEventIndex("acme/repo", "1");
    await d.instance.append(event(1));
    d.storage.map.delete("containment:v1:repo-job-index-marker:acme/repo/1");
    await expect(d.instance.append(event(1))).rejects.toThrow("containment job index marker divergent");
    const before = new Map(d.storage.map);
    const fresh = makeDO(); await fresh.instance.bootstrapContainedEventIndex("acme/repo", "1"); await fresh.instance.append(event(1));
    await fresh.instance.acquireLease("owner", T0); const claimed = await fresh.instance.claimNext("owner", 1, T0);
    fresh.storage.map.set("containment:v1:event:evt-1", { ...claimed!.event, state: "EFFECT_COMMITTED" });
    fresh.storage.map.delete("containment:v1:repo-job-index-marker:acme/repo/1");
    expect(await fresh.instance.acknowledge("evt-1", "owner", 1)).toBe(false);
    expect(fresh.storage.map.has("containment:v1:event:evt-1")).toBe(true);
    expect(before.has("containment:v1:event:evt-1")).toBe(true);
  });

  it("fails closed for a missing or damaged pair index and rejects the legacy key", async () => {
    const d = makeDO(); await d.instance.requestDrain();
    d.storage.map.set("containment:v1:job-index:acme/repo/1", { schema_version: 1, repo: "acme/repo", job_id: "1", event_ids: [] });
    await expect(d.instance.append(event(1))).rejects.toThrow("containment job index missing");
    expect(await d.instance.bootstrapContainedEventIndex("acme/repo", "1")).toEqual({ status: "blocked" });
    d.storage.map.set("containment:v1:repo-job-index:acme/repo/1", { schema_version: 1, repo: "acme/repo", job_id: "1", active_event_ids: ["evt-old"], active_count: 2, updated_at_ms: T0 });
    await expect(d.instance.append(event(1))).rejects.toThrow("containment job index divergent");
  });

  it("bootstraps unrelated pairs despite another active pair and never recreates a deleted index", async () => {
    const d = makeDO(); await d.instance.requestDrain();
    expect(await d.instance.bootstrapContainedEventIndex("acme/repo", "1")).toEqual({ status: "bootstrapped" });
    await d.instance.append(event(1));
    expect(await d.instance.bootstrapContainedEventIndex("acme/repo", "2")).toEqual({ status: "bootstrapped" });
    d.storage.map.delete("containment:v1:repo-job-index:acme/repo/2");
    expect(await d.instance.bootstrapContainedEventIndex("acme/repo", "2")).toEqual({ status: "blocked" });
  });

  it("blocks first bootstrap on exact reservation and effect-owner state", async () => {
    const reserved = makeDO(); await reserved.instance.requestDrain();
    reserved.storage.map.set("containment:v1:reservation:acme/repo/7", { schema_version: 1 });
    expect(await reserved.instance.bootstrapContainedEventIndex("acme/repo", "7")).toEqual({ status: "blocked" });
    const owned = makeDO(); await owned.instance.requestDrain();
    owned.storage.map.set(containmentEffectPointerKey({ repo: "acme/repo", job_id: "7", effect_id: redriveEffectId("acme/repo", "7") }), { schema_version: 1 });
    expect(await owned.instance.bootstrapContainedEventIndex("acme/repo", "7")).toEqual({ status: "blocked" });
  });

  it("retryOrphanedSpawns: 100 concurrent candidates have exactly one eligibility/effect", async () => {
    const d = makeDO(); const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1000 }) });
    const drive = vi.fn(async () => {}); const verify = vi.fn(async () => null); const contexts = [...Array(100)].map(() => ctx());
    await Promise.all(contexts.map((c) => retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), c as never, T0, drive, verify)));
    await Promise.all(contexts.map(settle));
    expect(drive).toHaveBeenCalledTimes(1); expect(verify).toHaveBeenCalledTimes(0);
    expect((d.storage.map.get(reserveKey()) as ContainmentRedriveReservation).state).toBe("COMPLETED");
    expect(d.storage.map.get("containment:v1:repo-job-index-marker:acme/repo/123")).toMatchObject({ schema_version: 1, repo: "acme/repo", job_id: "123" });
  });

  it("redriveOrphanedJobs: 100 concurrent GitHub scans produce exactly one effect", async () => {
    const d = makeDO(); const store = kv(); const generated = vi.fn();
    const old = new Date(T0 - 1_000_000).toISOString();
    const fetchSpy = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("generate-jitconfig")) { generated(); return new Response(JSON.stringify({ encoded_jit_config: "jit", runner: { id: 7 } }), { status: 200 }); }
      if (url.endsWith("/jobs")) return new Response(JSON.stringify({ jobs: [{ id: 123, status: "queued", runner_id: 0, labels: ["corelink"] }] }), { status: 200 });
      return new Response(JSON.stringify({ workflow_runs: [{ id: 1, created_at: old }] }), { status: 200 });
    });
    vi.stubGlobal("fetch", fetchSpy);
    const contexts = [...Array(100)].map(() => ctx()); const calls = contexts.map((c) => redriveOrphanedJobs(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }), c as never, undefined, T0));
    await Promise.all(calls); await Promise.all(contexts.map(settle));
    expect(generated).toHaveBeenCalledTimes(1); expect((d.storage.map.get(reserveKey()) as ContainmentRedriveReservation).state).toBe("COMPLETED");
  });

  it("admitExpiredHeldAtomicFence: active HELD is busy; expiry appends and fences stale owner", async () => {
    const d = makeDO(); await bootstrap(d, "123"); const held = await d.instance.reserveRedriveCandidate("acme/repo", "123", T0); expect(held.status).toBe("reserved");
    const active = await worker.fetch(await webhook(123, "held-active"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never); expect(active.status).toBe(503);
    vi.setSystemTime(T0 + REDRIVE_RESERVATION_TTL_MS); const expired = await worker.fetch(await webhook(123, "held-expired"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never); expect(expired.status).toBe(202);
    expect(d.storage.map.has(reserveKey())).toBe(false); expect(await d.instance.beginReservedEffect("acme/repo", "123", held.reservation!.owner, held.reservation!.token, held.reservation!.epoch)).toMatchObject({ status: "stale" });
  });

  it("returns redrive_owned (route 202) for eligible and completed tombstones", async () => {
    const d = makeDO(); await bootstrap(d, "123"); const held = await d.instance.reserveRedriveCandidate("acme/repo", "123", T0); const r = held.reservation!;
    expect((await d.instance.beginReservedEffect(r.repo, r.job_id, r.owner, r.token, r.epoch, r.path, r.effect_id)).status).toBe("eligible");
    expect((await worker.fetch(await webhook(123, "eligible"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never)).status).toBe(202);
    expect((await d.instance.completeRedrive(r.repo, r.job_id, r.owner, r.token, r.epoch, r.effect_id)).status).toBe("completed");
    expect((await worker.fetch(await webhook(123, "completed-tombstone"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never)).status).toBe(202);
  });

  it("repoJobNormalizerFailClosed: refuses contained-first, canonicalizes identity, and never reopens effects", async () => {
    const d = makeDO(); await bootstrap(d, "123"); await d.instance.append(event(123)); expect((await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).status).toBe("contained");
    const e = makeDO(); await bootstrap(e, "123"); const r = await e.instance.reserveRedriveCandidate("acme/repo", "123", T0); const first = r.reservation!;
    vi.setSystemTime(T0 + REDRIVE_RESERVATION_TTL_MS); const reclaimed = await e.instance.reserveRedriveCandidate("acme/repo", "123"); expect(reclaimed.status).toBe("reserved"); expect(reclaimed.reservation?.epoch).toBe(2); expect(reclaimed.reservation?.owner).not.toBe(first.owner);
    const p = reclaimed.reservation!; await e.instance.beginReservedEffect(p.repo, p.job_id, p.owner, p.token, p.epoch, p.path, p.effect_id); vi.setSystemTime(T0 + 2 * REDRIVE_RESERVATION_TTL_MS); expect((await e.instance.reserveRedriveCandidate(p.repo, p.job_id)).status).toBe("effect_eligible");
    expect((await e.instance.completeRedrive(p.repo, p.job_id, p.owner, p.token, p.epoch, p.effect_id)).status).toBe("completed"); expect((await e.instance.reserveRedriveCandidate(p.repo, p.job_id)).status).toBe("completed");
  });

  it("completionObservedLatchStateMachine: handles HELD, latch, and tombstone interleavings", async () => {
    const held = makeDO(); await bootstrap(held, "123"); const h = await held.instance.reserveRedriveCandidate("acme/repo", "123", T0); expect((await held.instance.clearCompletedRedrive("acme/repo", "123", h.reservation!.effect_id)).status).toBe("terminal"); expect((await held.instance.beginReservedEffect("acme/repo", "123", h.reservation!.owner, h.reservation!.token, 1)).status).toBe("stale");
    const latch = makeDO(); await bootstrap(latch, "123"); const l = await latch.instance.reserveRedriveCandidate("acme/repo", "123", T0); const lr = l.reservation!; await latch.instance.beginReservedEffect(lr.repo, lr.job_id, lr.owner, lr.token, lr.epoch, lr.path, lr.effect_id); expect((await latch.instance.clearCompletedRedrive(lr.repo, lr.job_id, lr.effect_id)).status).toBe("latched"); expect((await latch.instance.completeRedrive(lr.repo, lr.job_id, lr.owner, lr.token, lr.epoch, lr.effect_id)).status).toBe("cleared_after_completion"); expect(latch.storage.map.has(reserveKey())).toBe(false);
    const tomb = makeDO(); await bootstrap(tomb, "123"); const t = await tomb.instance.reserveRedriveCandidate("acme/repo", "123", T0); const tr = t.reservation!; await tomb.instance.beginReservedEffect(tr.repo, tr.job_id, tr.owner, tr.token, tr.epoch, tr.path, tr.effect_id); expect((await tomb.instance.completeRedrive(tr.repo, tr.job_id, tr.owner, tr.token, tr.epoch, tr.effect_id)).status).toBe("completed"); expect((await tomb.instance.clearCompletedRedrive(tr.repo, tr.job_id, tr.effect_id)).status).toBe("cleared");
  });
});

describe("redrive gates and identity/authorization", () => {
  it("suppresses both reconcilers for paused or invalid switches before KV/GitHub", async () => {
    const d = makeDO(); const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 }) }); const drive = vi.fn(async () => {});
    for (const value of ["1", "true", " "]) { const c = ctx(); await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: value }), c as never, T0, drive, vi.fn(async () => null)); expect(store.list).toHaveBeenCalledTimes(0); }
  });

  it("preserves orphan repo/labels/installation and forces installation-only drive", async () => {
    const d = makeDO(); const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1000 }) }); const drive = vi.fn(async (_env: unknown, opts: { repo: string; installationId: string; labels: string[]; credential_source?: string }) => { expect(opts.repo).toBe("acme/repo"); expect(opts.installationId).toBe("42"); expect(opts.labels).toEqual(["corelink"]); expect(opts.credential_source).toBe("installation-only"); });
    await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), ctx() as never, T0, drive, vi.fn(async () => null)); expect(drive).toHaveBeenCalledTimes(1);
    const badDo = makeDO(); const bad = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "999", labels: ["other"], attempts: 1, firstRecordedMs: T0 }) }); await retryOrphanedSpawns(env(badDo, bad, { AUTOSCALER_REDRIVE_PAUSED: "0" }), ctx() as never, T0, drive, vi.fn(async () => null)); expect(drive).toHaveBeenCalledTimes(1);
  });

  it("canonicalizes identity by trimming and fences every tuple field", async () => {
    const d = makeDO(); await bootstrap(d, "123"); const r = await d.instance.reserveRedriveCandidate("  Acme/repo  ", "123", T0); expect(r.reservation?.repo).toBe("acme/repo"); expect(r.reservation?.job_id).toBe("123"); const p = r.reservation!;
    expect((await d.instance.beginReservedEffect("ACME/REPO", "123", p.owner, "wrong", p.epoch)).status).toBe("stale"); expect((await d.instance.beginReservedEffect("ACME/REPO", "123", p.owner, p.token, p.epoch, "redrive", "wrong-effect")).status).toBe("invalid"); expect((await d.instance.completeRedrive("ACME/REPO", "123", p.owner, p.token, p.epoch, p.effect_id)).status).toBe("incomplete");
  });

  it("does not promote an expired HELD tuple and keeps repo/job indexes independent", async () => {
    const d = makeDO(); await bootstrap(d, "7"); await bootstrap(d, "7", "other/repo");
    const list = vi.spyOn(d.storage, "list");
    const first = (await d.instance.reserveRedriveCandidate(" Acme/Repo ", "7", T0)).reservation!;
    expect((await d.instance.beginReservedEffect(first.repo, first.job_id, first.owner, first.token, first.epoch, first.path, first.effect_id, T0 + REDRIVE_RESERVATION_TTL_MS)).status).toBe("ineligible");
    const other = await d.instance.reserveRedriveCandidate("other/repo", "7", T0 + REDRIVE_RESERVATION_TTL_MS);
    expect(other.status).toBe("reserved");
    expect((await d.instance.reserveRedriveCandidate("acme/repo", "7", T0 + REDRIVE_RESERVATION_TTL_MS)).status).toBe("reserved");
    expect(list).not.toHaveBeenCalled();
  });
});

describe("T3-W17 anti-vacuity queue and reservation fences", () => {
  it("keeps the claimed head ahead of later appends and reclaims a pre-permit claim with a new epoch", async () => {
    const d = makeDO(); await bootstrap(d, "1"); await bootstrap(d, "2"); await bootstrap(d, "3");
    await d.instance.append(event(1));
    await d.instance.append(event(2));
    const first = await d.instance.acquireLease("first", T0);
    expect(first).toMatchObject({ epoch: 1 });
    expect((await d.instance.claimNext("first", 1, T0))?.event.event_id).toBe("evt-1");
    // Claiming again cannot jump the unresolved head to evt-2.
    expect((await d.instance.claimNext("first", 1, T0))?.event.event_id).toBe("evt-1");
    await d.instance.append(event(3));
    expect(d.storage.map.get("containment:v1:pause:00000000000000000003")).toEqual({ schema_version: 1, event_id: "evt-3", pause_seq: 3 });
    expect(await d.instance.snapshot()).toMatchObject({ drain_cursor: 0, backlog_count: 3, next_pause_seq: 4 });

    const reclaimed = await d.instance.acquireLease("second", T0 + 120_000);
    expect(reclaimed).toMatchObject({ owner: "second", epoch: 2 });
    const sameHead = await d.instance.claimNext("second", 2, T0 + 120_000);
    expect(sameHead?.event).toMatchObject({ event_id: "evt-1", state: "CLAIMED", claim: { owner: "second", lease_epoch: 2 }, effect_permit: null });
    expect(await d.instance.beginEffect("evt-1", "second", 2, T0 + 120_000)).toMatchObject({ issued_to_owner: "second", issued_to_epoch: 2 });
  });

  it("rejects every stale lease transition after reclaim without moving the head", async () => {
    const d = makeDO(); await bootstrap(d, "1");
    await d.instance.append(event(1));
    await d.instance.acquireLease("old", T0);
    await d.instance.claimNext("old", 1, T0);
    const fresh = await d.instance.acquireLease("new", T0 + 120_000);
    expect(fresh?.epoch).toBe(2);
    const before = await d.instance.getEvent("evt-1");
    expect(await d.instance.claimNext("old", 1, T0 + 120_000)).toBeNull();
    expect(await d.instance.beginEffect("evt-1", "old", 1, T0 + 120_000)).toBeNull();
    expect(await d.instance.markEffectCommitted("evt-1", "old", 1)).toBe(false);
    expect(await d.instance.acknowledge("evt-1", "old", 1)).toBe(false);
    expect(await d.instance.getEvent("evt-1")).toEqual(before);
    expect(await d.instance.snapshot()).toMatchObject({ drain_cursor: 0, backlog_count: 1, lease_epoch: 2 });
  });

  it("never transfers an issued permit to a lease reclaimer", async () => {
    const d = makeDO(); await bootstrap(d, "1");
    await d.instance.append(event(1));
    await d.instance.acquireLease("old", T0);
    await d.instance.claimNext("old", 1, T0);
    const permit = await d.instance.beginEffect("evt-1", "old", 1, T0);
    expect(permit).not.toBeNull();
    await d.instance.acquireLease("new", T0 + 120_000);
    await d.instance.claimNext("new", 2, T0 + 120_000);
    expect(await d.instance.beginEffect("evt-1", "new", 2, T0 + 120_000)).toBeNull();
    expect(await d.instance.markEffectCommitted("evt-1", "new", 2)).toBe(false);
    expect((await d.instance.getEvent("evt-1"))?.effect_permit).toEqual(permit);
  });

  it("persists the exact repo-scoped reservation schema and stable effect id", async () => {
    const d = makeDO(); await bootstrap(d, "123");
    const result = await d.instance.reserveRedriveCandidate("acme/repo", "123", T0);
    expect(result.status).toBe("reserved");
    expect(d.storage.map.get(reserveKey())).toMatchObject({
      schema_version: 1, repo: "acme/repo", job_id: "123", epoch: 1, path: "redrive",
      state: "HELD", expires_ms: T0 + REDRIVE_RESERVATION_TTL_MS, event_id: null,
      effect_id: "containment:v1:redrive:acme/repo/123", completion_observed: false,
    });
    const stored = d.storage.map.get(reserveKey()) as ContainmentRedriveReservation;
    expect(stored.owner).toEqual(expect.any(String));
    expect(stored.token).toEqual(expect.any(String));
    expect(stored.owner).not.toBe(stored.token);
    expect([...d.storage.map.keys()].filter((key) => key.startsWith("containment:v1:reservation:"))).toEqual([reserveKey()]);
  });

  it("fails closed for every invalid canonical repo/job before creating a reservation", async () => {
    const invalid: Array<[string, string]> = [
      ["", "123"], ["acme", "123"], ["acme/repo/extra", "123"], ["acme//repo", "123"], ["acme/.", "123"],
      ["acme/repo", ""], ["acme/repo", "-0"], ["acme/repo", "-1"], ["acme/repo", "1.5"],
      ["acme/repo", "01"], ["acme/repo", " 1 "], ["acme/repo", "9007199254740992"],
    ];
    for (const [repo, job] of invalid) {
      const d = makeDO();
      expect((await d.instance.reserveRedriveCandidate(repo, job, T0)).status).toBe("invalid");
      expect(d.storage.map.size).toBe(0);
    }
  });

  it("fences stale reservation tuples before any caller can enter a second effect", async () => {
    const d = makeDO(); await bootstrap(d, "123");
    const old = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).reservation!;
    const next = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0 + REDRIVE_RESERVATION_TTL_MS)).reservation!;
    expect(next.epoch).toBe(2);
    expect(await d.instance.beginReservedEffect(old.repo, old.job_id, old.owner, old.token, old.epoch, old.path, old.effect_id)).toMatchObject({ status: "stale" });
    expect((d.storage.map.get(reserveKey()) as ContainmentRedriveReservation)).toMatchObject({ owner: next.owner, token: next.token, epoch: 2, state: "HELD" });
    expect((await d.instance.beginReservedEffect(next.repo, next.job_id, next.owner, next.token, next.epoch, next.path, next.effect_id)).status).toBe("eligible");
    expect((await d.instance.reserveRedriveCandidate(next.repo, next.job_id, T0 + 10 * REDRIVE_RESERVATION_TTL_MS)).status).toBe("effect_eligible");
  });

  it("routes a verified completed webhook through the reservation latch while intake is paused", async () => {
    const d = makeDO(); await bootstrap(d, "123");
    const held = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).reservation!;
    await d.instance.beginReservedEffect(held.repo, held.job_id, held.owner, held.token, held.epoch, held.path, held.effect_id);
    const response = await worker.fetch(await webhook(123, "completed-during-pause", "completed"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never);
    expect(response.status).toBe(200);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE", completion_observed: true });
    expect((await d.instance.completeRedrive(held.repo, held.job_id, held.owner, held.token, held.epoch, held.effect_id)).status).toBe("cleared_after_completion");
    expect(d.storage.map.has(reserveKey())).toBe(false);
  });
});

describe("T3-W17 deterministic first-party reservation seams", () => {
  it("orders 100 first-party contenders behind one eligibility before every mutation", async () => {
    const d = makeDO(); const store = kv(); const order: string[] = [];
    const list = vi.fn(async () => [{ jobId: "123", labels: ["corelink"] }]);
    const release = vi.fn(async () => {
      order.push("release");
      expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
    });
    const claim = vi.fn(async () => { order.push("claim"); return true; });
    const drive = vi.fn(async () => { order.push("drive"); });
    const orphan = vi.fn(async () => { order.push("orphan"); });
    const contexts = [...Array(100)].map(() => ctx());
    await Promise.all(contexts.map((c) => redriveOrphanedJobs(
      env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }), c as never, undefined, T0,
      { listOrphanRunnerJobs: list, releaseSpawnClaim: release, claimSpawn: claim, driveSpawn: drive, recordOrphan: orphan },
    )));
    await Promise.all(contexts.map(settle));
    expect(list).toHaveBeenCalledTimes(100); expect(release).toHaveBeenCalledTimes(1); expect(claim).toHaveBeenCalledTimes(1); expect(drive).toHaveBeenCalledTimes(1); expect(orphan).not.toHaveBeenCalled();
    expect(order).toEqual(["release", "claim", "drive"]);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "COMPLETED", epoch: 1, effect_id: "containment:v1:redrive:acme/repo/123" });
  });

  it("keeps re-drive enabled while intake is paused and preserves the unmapped cold fallback", async () => {
    const d = makeDO(); const store = kv(); const drive = vi.fn(async () => {});
    const c = ctx();
    await redriveOrphanedJobs(
      env(d, store, { AUTOSCALER_INTAKE_PAUSED: "1", AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo", REPO_INSTALLATION_MAP: undefined }),
      c as never, undefined, T0,
      { listOrphanRunnerJobs: async () => [{ jobId: "123", labels: ["corelink"] }], releaseSpawnClaim: async () => {}, claimSpawn: async () => true, driveSpawn: drive, recordOrphan: async () => {} },
    );
    await settle(c);
    expect(drive).toHaveBeenCalledTimes(1);
    expect(drive).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ repo: "acme/repo", installationId: "", labels: ["corelink"], credential_source: "installation-only" }));
  });

  it("leaves an eligible first-party reservation fail-closed after its waitUntil drive crashes", async () => {
    const d = makeDO(); const store = kv(); const c = ctx(); const drive = vi.fn(async () => { throw new Error("first-party crash"); });
    const deps = { listOrphanRunnerJobs: async () => [{ jobId: "123", labels: ["corelink"] }], releaseSpawnClaim: async () => {}, claimSpawn: async () => true, driveSpawn: drive, recordOrphan: async () => {} };
    await redriveOrphanedJobs(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }), c as never, undefined, T0, deps);
    await settle(c);
    expect(drive).toHaveBeenCalledTimes(1); expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
    const retry = ctx();
    await redriveOrphanedJobs(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }), retry as never, undefined, T0 + 10 * REDRIVE_RESERVATION_TTL_MS, deps);
    await settle(retry);
    expect(drive).toHaveBeenCalledTimes(1); expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
  });

  it("fences a stale first-party owner before release, claim, KV mutation, or drive", async () => {
    const d = makeDO(); const store = kv(); const originalPut = store.put.getMockImplementation()!;
    const originalDelete = store.delete.getMockImplementation()!;
    const authority = authorityProxy(d.instance, {
      beginReservedEffect: async (...args: any[]) => {
        await d.instance.reserveRedriveCandidate("acme/repo", "123", T0 + REDRIVE_RESERVATION_TTL_MS);
        return d.instance.beginReservedEffect(...args);
      },
    });
    const release = vi.fn(async () => {}); const claim = vi.fn(async () => true); const drive = vi.fn(async () => {});
    await redriveOrphanedJobs(
      envWithAuthority(d, store, authority, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }), ctx() as never, undefined, T0,
      { listOrphanRunnerJobs: async () => [{ jobId: "123", labels: ["corelink"] }], releaseSpawnClaim: release, claimSpawn: claim, driveSpawn: drive, recordOrphan: async () => {} },
    );
    expect(release).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled(); expect(drive).not.toHaveBeenCalled();
    expect(store.put).not.toHaveBeenCalled(); expect(store.delete).not.toHaveBeenCalled();
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "HELD", epoch: 2 });
    // Keep the explicit real implementations referenced so this assertion cannot
    // accidentally pass because the fake KV lost its mutation behavior.
    expect(originalPut).toEqual(expect.any(Function)); expect(originalDelete).toEqual(expect.any(Function));
  });
});

describe("T3-W17 retry ordering and independent switches", () => {
  it("makes retry orphan mutation and spawn claim only after EFFECT_ELIGIBLE", async () => {
    const d = makeDO(); const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1 }) });
    const put = store.put.getMockImplementation()!; const order: string[] = [];
    store.put.mockImplementation(async (key: string, value: string) => {
      if (key === "orphan:123" || key === "spawn:123") {
        expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE" });
        order.push(key);
      }
      await put(key, value);
    });
    const drive = vi.fn(async () => {}); const contexts = [...Array(100)].map(() => ctx());
    await Promise.all(contexts.map((c) => retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), c as never, T0, drive, vi.fn(async () => null))));
    await Promise.all(contexts.map(settle));
    expect(order).toEqual(["orphan:123", "spawn:123"]); expect(drive).toHaveBeenCalledTimes(1);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "COMPLETED" });
  });

  it("does not let a paused redrive switch suppress fresh intake, while both switches suppress both surfaces", async () => {
    const live = makeDO(); const liveStore = kv(); const liveCtx = ctx();
    const fresh = await worker.fetch(await webhook(123, "redrive-only-pause"), env(live, liveStore, { AUTOSCALER_INTAKE_PAUSED: "0", AUTOSCALER_REDRIVE_PAUSED: "1" }), liveCtx as never);
    expect(fresh.status).toBe(202); expect(liveStore.put).toHaveBeenCalled(); expect((await live.instance.snapshot()).backlog_count).toBe(0);

    const both = makeDO(); const bothStore = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 }) });
    const contained = await worker.fetch(await webhook(124, "both-paused"), env(both, bothStore, { AUTOSCALER_INTAKE_PAUSED: "1", AUTOSCALER_REDRIVE_PAUSED: "1" }), ctx() as never);
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(env(both, bothStore, { AUTOSCALER_INTAKE_PAUSED: "1", AUTOSCALER_REDRIVE_PAUSED: "1" }), ctx() as never, T0, drive, vi.fn(async () => null));
    expect(contained.status).toBe(202); expect((await both.instance.snapshot()).backlog_count).toBe(1); expect(drive).not.toHaveBeenCalled(); expect(bothStore.list).not.toHaveBeenCalled();
  });
});
