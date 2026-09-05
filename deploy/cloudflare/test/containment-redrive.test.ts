import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn(() => ({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) })) }));

import worker, { ContainmentDO, REDRIVE_RESERVATION_TTL_MS, redriveOrphanedJobs, retryOrphanedSpawns, runContainmentDrain, type ContainmentEvent, type ContainmentRedriveReservation } from "../src/index";

const T0 = 1_750_000_000_000;

function clone<T>(value: T): T { return value === undefined ? value : JSON.parse(JSON.stringify(value)) as T; }
class TxnStorage {
  constructor(private readonly map: Map<string, unknown>) {}
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> { return new Map([...this.map].filter(([key]) => key.startsWith(opts.prefix ?? "")).map(([key, value]) => [key, clone(value) as T])); }
}
class FakeStorage {
  readonly map = new Map<string, unknown>();
  private tail: Promise<void> = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> { return new Map([...this.map].filter(([key]) => key.startsWith(opts.prefix ?? "")).map(([key, value]) => [key, clone(value) as T])); }
  async transaction<T>(fn: (storage: TxnStorage) => Promise<T>): Promise<T> {
    const run = this.tail.then(async () => {
      const snapshot = new Map([...this.map].map(([key, value]) => [key, clone(value)]));
      const result = await fn(new TxnStorage(snapshot));
      this.map.clear(); for (const [key, value] of snapshot) this.map.set(key, value);
      return result;
    });
    this.tail = run.then(() => undefined, () => undefined); return run;
  }
}

function ns<T>(instance: T, name = "global") { return { idFromName: vi.fn(() => name), get: vi.fn(() => instance) }; }
function makeDO(runtimeEnv: Record<string, unknown> = {}) { const storage = new FakeStorage(); const instance = new ContainmentDO({ storage } as never, runtimeEnv as never); return { storage, instance, binding: ns(instance) }; }
function kv(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return { map, get: vi.fn(async (key: string) => map.get(key) ?? null), put: vi.fn(async (key: string, value: string) => { map.set(key, value); }), delete: vi.fn(async (key: string) => { map.delete(key); }), list: vi.fn(async ({ prefix }: { prefix?: string } = {}) => ({ keys: [...map.keys()].filter((key) => key.startsWith(prefix ?? "")).map((name) => ({ name })) })) };
}
function event(n: number, overrides: Partial<ContainmentEvent> = {}): Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit"> {
  return { schema_version: 1, event_id: `evt-${n}`, received_at_ms: T0, body_sha256: "a".repeat(64), raw_payload: "{}", action: "queued", job_id: String(n), repo: "acme/repo", installation_id: "42", labels: ["corelink"], effect_id: `containment:v1:evt-${n}`, ...overrides };
}
function ctx() { const tasks: Promise<unknown>[] = []; return { tasks, waitUntil(p: Promise<unknown>) { tasks.push(Promise.resolve(p)); }, passThroughOnException() {} }; }
async function settle(c: ReturnType<typeof ctx>) { for (let i = 0; i < 8 && c.tasks.length; i++) await Promise.all(c.tasks.splice(0)); }
async function webhook(job: number, delivery: string, action = "queued"): Promise<Request> {
  const raw = new TextEncoder().encode(JSON.stringify({ action, workflow_job: { id: job, labels: ["corelink"] }, repository: { full_name: "acme/repo" }, installation: { id: 42 } }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode("secret"), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = await crypto.subtle.sign("HMAC", key, raw);
  const sig = `sha256=${[...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("")}`;
  return new Request("https://worker/webhook", { method: "POST", headers: { "content-type": "application/json", "x-github-event": "workflow_job", "x-hub-signature-256": sig, "x-github-delivery": delivery }, body: raw });
}
function env(d: ReturnType<typeof makeDO>, store = kv(), extra: Record<string, unknown> = {}) {
  return {
    GITHUB_WEBHOOK_SECRET: "secret", GITHUB_MINT_TOKEN: "mint", RUNNER_JOB_PATS: store, CONTAINMENT: d.binding,
    REPO_INSTALLATION_MAP: JSON.stringify({ "acme/repo": "42" }), RUNNER_CONTAINER: {}, CHECK_HOST_CONTAINER: {},
    CONCURRENCY_SLOTS: ns({ acquire: vi.fn(async () => ({ admitted: true })), release: vi.fn(async () => {}) }),
    CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }), ...extra,
  } as never;
}
function envWithAuthority(d: ReturnType<typeof makeDO>, store: ReturnType<typeof kv>, authority: unknown, extra: Record<string, unknown> = {}) {
  return { ...env(d, store, extra), CONTAINMENT: ns(authority) } as never;
}
function authorityProxy(instance: ContainmentDO, overrides: Record<string, (...args: any[]) => unknown>) {
  return new Proxy(instance, {
    get(target, property) {
      const override = overrides[String(property)];
      if (override) return override;
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}
function reservation(storage: FakeStorage, repo = "acme/repo", job = "123", state: ContainmentRedriveReservation["state"] = "HELD", owner = "owner-a", token = "token-a", epoch = 1): ContainmentRedriveReservation {
  const rec: ContainmentRedriveReservation = { schema_version: 1, repo, job_id: job, owner, token, epoch, path: "redrive", state, expires_ms: T0 + REDRIVE_RESERVATION_TTL_MS, event_id: null, effect_id: `containment:v1:redrive:${repo}/${job}`, completion_observed: false };
  storage.map.set(`containment:v1:reservation:${repo}/${job}`, rec); return rec;
}
function reserveKey(repo = "acme/repo", job = "123") { return `containment:v1:reservation:${repo}/${job}`; }

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T0); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("ordered queue / lease / immutable evidence recovery", () => {
  it("claims strictly in order, issues one permit, and fences stale leases", async () => {
    const d = makeDO(); await d.instance.append(event(1)); await d.instance.append(event(2));
    const a = await d.instance.acquireLease("owner-a"); expect(a).toEqual({ owner: "owner-a", epoch: 1, expires_ms: T0 + 120_000 });
    expect(await d.instance.acquireLease("owner-b")).toBeNull();
    const head = await d.instance.claimNext("owner-a", 1); expect(head?.event.pause_seq).toBe(1); expect(head?.committed).toBe(false);
    const permit = await d.instance.beginEffect("evt-1", "owner-a", 1); expect(permit?.issued_to_owner).toBe("owner-a"); expect(await d.instance.beginEffect("evt-1", "owner-a", 1)).toEqual(permit);
    expect(await d.instance.markEffectCommitted("evt-1", "owner-a", 1)).toBe(false); // evidence is required
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
    const d = makeDO(); const store = kv(); const withKv = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
    await withKv.append(event(1)); await withKv.acquireLease("owner-a"); await withKv.claimNext("owner-a", 1); const permit = await withKv.beginEffect("evt-1", "owner-a", 1); expect(permit).not.toBeNull();
    const effect = "containment:v1:evt-1"; const evidence = [
      ["spawn_claim", `spawn:1`, "123"],
      ["attempt", "github:generate-jitconfig", JSON.stringify({ runner_name: "runner", runner_id: 9, attempt: 1 })],
      ["placement", "orphan:1", JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0, placedMs: T0 + 1, effect_id: effect })],
      ["lease", "jhandle:1", "handle-1"],
    ] as const;
    const records: Record<string, unknown>[] = [];
    for (const [kind, sourceKey, sourceValue] of evidence) {
      const rec = { schema_version: 1, kind, effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: sourceKey, source_value: sourceValue, source_sha256: await digest(sourceValue), ...(kind === "attempt" ? { attempt_count: 1 } : {}) };
      records.push(rec); await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:${kind}`, JSON.stringify(rec));
    }
    const resultSource = JSON.stringify({ terminal: "DELIVERED", attempt_count: 1, spawn_claim_sha256: (records[0] as { source_sha256: string }).source_sha256, attempt_sha256: (records[1] as { source_sha256: string }).source_sha256, placement_sha256: (records[2] as { source_sha256: string }).source_sha256, lease_sha256: (records[3] as { source_sha256: string }).source_sha256 });
    const result = { schema_version: 1, kind: "result", effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: "result", source_value: resultSource, source_sha256: await digest(resultSource), terminal: "DELIVERED", attempt_count: 1 };
    await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:result`, JSON.stringify(result));
    expect(await withKv.acquireLease("owner-b", T0 + 120_000)).toMatchObject({ epoch: 2 });
    const claimed = await withKv.claimNext("owner-b", 2); expect(claimed?.committed).toBe(false);
    const canonical = [...records, result].map((r) => JSON.stringify(r)).join();
    expect(await withKv.recoverEffectCommitted(effect, "owner-b", 2, await digest(canonical))).toBe(false); // proof is the exact array JSON, not a comma join
    expect(await withKv.recoverEffectCommitted(effect, "owner-b", 2, await digest(JSON.stringify([...records, result].map((r) => JSON.stringify(r)))))).toBe(true);
    expect(await withKv.acknowledge("evt-1", "owner-b", 2)).toBe(true);
  });

  it("fails closed on malformed schema or tampered evidence", async () => {
    const d = makeDO(); const store = kv(); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
    await instance.append(event(1)); await instance.acquireLease("a"); await instance.claimNext("a", 1); const permit = await instance.beginEffect("evt-1", "a", 1); expect(permit).not.toBeNull();
    await instance.markEffectCommitted("evt-1", "a", 1); // no evidence, remains CLAIMED
    expect(await instance.recoverEffectCommitted("containment:v1:evt-1", "a", 1, "0".repeat(64))).toBe(false);
    const rec = reservation(d.storage); (rec as unknown as { schema_version: number }).schema_version = 2; d.storage.map.set(reserveKey(), rec);
    expect((await instance.reserveRedriveCandidate("acme/repo", "123")).status).toBe("busy");
  });
});

async function digest(value: string): Promise<string> { const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)); return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join(""); }

async function recoveryFixture(mutate: (records: Record<string, unknown>[]) => void = () => {}) {
  const d = makeDO(); const store = kv(); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
  await instance.append(event(1)); await instance.acquireLease("old", T0); await instance.claimNext("old", 1, T0);
  const permit = await instance.beginEffect("evt-1", "old", 1, T0); expect(permit).not.toBeNull();
  const effect = "containment:v1:evt-1";
  const source = [
    ["spawn_claim", "spawn:1", "123"],
    ["attempt", "github:generate-jitconfig", JSON.stringify({ runner_name: "runner", runner_id: 9, attempt: 1 })],
    ["placement", "orphan:1", JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0, placedMs: T0 + 1, effect_id: effect })],
    ["lease", "jhandle:1", "handle-1"],
  ] as const;
  const records: Record<string, unknown>[] = [];
  for (const [kind, sourceKey, sourceValue] of source) {
    records.push({ schema_version: 1, kind, effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: sourceKey, source_value: sourceValue, source_sha256: await digest(sourceValue), ...(kind === "attempt" ? { attempt_count: 1 } : {}) });
  }
  const resultSource = JSON.stringify({ terminal: "DELIVERED", attempt_count: 1, spawn_claim_sha256: records[0].source_sha256, attempt_sha256: records[1].source_sha256, placement_sha256: records[2].source_sha256, lease_sha256: records[3].source_sha256 });
  records.push({ schema_version: 1, kind: "result", effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: "result", source_value: resultSource, source_sha256: await digest(resultSource), terminal: "DELIVERED", attempt_count: 1 });
  mutate(records);
  for (const rec of records) await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:${rec.kind}`, JSON.stringify(rec));
  await instance.acquireLease("new", T0 + 120_000); await instance.claimNext("new", 2, T0 + 120_000);
  return { d, instance, records, effect };
}

async function writeDeliveredProof(store: ReturnType<typeof kv>, opts: { jobId: string; effect_id: string; containment_event_id: string; effect_permit_id: string }) {
  const entries: Record<string, unknown>[] = [];
  const sources = [
    ["spawn_claim", `spawn:${opts.jobId}`, "123"],
    ["attempt", "github:generate-jitconfig", JSON.stringify({ runner_name: "runner", runner_id: 9, attempt: 1 })],
    ["placement", `orphan:${opts.jobId}`, JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0, placedMs: T0 + 1, effect_id: opts.effect_id })],
    ["lease", `jhandle:${opts.jobId}`, "handle-1"],
  ] as const;
  for (const [kind, sourceKey, sourceValue] of sources) entries.push({ schema_version: 1, kind, effect_id: opts.effect_id, event_id: opts.containment_event_id, job_id: opts.jobId, permit_id: opts.effect_permit_id, source_key: sourceKey, source_value: sourceValue, source_sha256: await digest(sourceValue), ...(kind === "attempt" ? { attempt_count: 1 } : {}) });
  const resultSource = JSON.stringify({ terminal: "DELIVERED", attempt_count: 1, spawn_claim_sha256: entries[0].source_sha256, attempt_sha256: entries[1].source_sha256, placement_sha256: entries[2].source_sha256, lease_sha256: entries[3].source_sha256 });
  entries.push({ schema_version: 1, kind: "result", effect_id: opts.effect_id, event_id: opts.containment_event_id, job_id: opts.jobId, permit_id: opts.effect_permit_id, source_key: "result", source_value: resultSource, source_sha256: await digest(resultSource), terminal: "DELIVERED", attempt_count: 1 });
  for (const entry of entries) await store.put(`containment:v1:effect:${encodeURIComponent(opts.effect_id)}:${entry.kind}`, JSON.stringify(entry));
}

describe("atomic redrive reservation state machine", () => {
  it("retryOrphanedSpawns: 100 concurrent candidates have exactly one eligibility/effect", async () => {
    const d = makeDO(); const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1000 }) });
    const drive = vi.fn(async () => {}); const verify = vi.fn(async () => null); const contexts = [...Array(100)].map(() => ctx());
    await Promise.all(contexts.map((c) => retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), c as never, T0, drive, verify)));
    await Promise.all(contexts.map(settle));
    expect(drive).toHaveBeenCalledTimes(1); expect(verify).toHaveBeenCalledTimes(0);
    expect((d.storage.map.get(reserveKey()) as ContainmentRedriveReservation).state).toBe("COMPLETED");
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
    const d = makeDO(); const held = await d.instance.reserveRedriveCandidate("acme/repo", "123", T0); expect(held.status).toBe("reserved");
    const active = await worker.fetch(await webhook(123, "held-active"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never); expect(active.status).toBe(503);
    vi.setSystemTime(T0 + REDRIVE_RESERVATION_TTL_MS); const expired = await worker.fetch(await webhook(123, "held-expired"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never); expect(expired.status).toBe(202);
    expect(d.storage.map.has(reserveKey())).toBe(false); expect(await d.instance.beginReservedEffect("acme/repo", "123", held.reservation!.owner, held.reservation!.token, held.reservation!.epoch)).toMatchObject({ status: "stale" });
  });

  it("returns redrive_owned (route 202) for eligible and completed tombstones", async () => {
    const d = makeDO(); const held = await d.instance.reserveRedriveCandidate("acme/repo", "123", T0); const r = held.reservation!;
    expect((await d.instance.beginReservedEffect(r.repo, r.job_id, r.owner, r.token, r.epoch, r.path, r.effect_id)).status).toBe("eligible");
    expect((await worker.fetch(await webhook(123, "eligible"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never)).status).toBe(202);
    expect((await d.instance.completeRedrive(r.repo, r.job_id, r.owner, r.token, r.epoch, r.effect_id)).status).toBe("completed");
    expect((await worker.fetch(await webhook(123, "completed-tombstone"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never)).status).toBe(202);
  });

  it("repoJobNormalizerFailClosed: refuses contained-first, canonicalizes identity, and never reopens effects", async () => {
    const d = makeDO(); await d.instance.append(event(123)); expect((await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).status).toBe("contained");
    const e = makeDO(); const r = await e.instance.reserveRedriveCandidate("acme/repo", "123", T0); const first = r.reservation!;
    vi.setSystemTime(T0 + REDRIVE_RESERVATION_TTL_MS); const reclaimed = await e.instance.reserveRedriveCandidate("acme/repo", "123"); expect(reclaimed.status).toBe("reserved"); expect(reclaimed.reservation?.epoch).toBe(2); expect(reclaimed.reservation?.owner).not.toBe(first.owner);
    const p = reclaimed.reservation!; await e.instance.beginReservedEffect(p.repo, p.job_id, p.owner, p.token, p.epoch, p.path, p.effect_id); vi.setSystemTime(T0 + 2 * REDRIVE_RESERVATION_TTL_MS); expect((await e.instance.reserveRedriveCandidate(p.repo, p.job_id)).status).toBe("effect_eligible");
    expect((await e.instance.completeRedrive(p.repo, p.job_id, p.owner, p.token, p.epoch, p.effect_id)).status).toBe("completed"); expect((await e.instance.reserveRedriveCandidate(p.repo, p.job_id)).status).toBe("completed");
  });

  it("completionObservedLatchStateMachine: handles HELD, latch, and tombstone interleavings", async () => {
    const held = makeDO(); const h = await held.instance.reserveRedriveCandidate("acme/repo", "123", T0); expect((await held.instance.clearCompletedRedrive("acme/repo", "123", h.reservation!.effect_id)).status).toBe("terminal"); expect((await held.instance.beginReservedEffect("acme/repo", "123", h.reservation!.owner, h.reservation!.token, 1)).status).toBe("stale");
    const latch = makeDO(); const l = await latch.instance.reserveRedriveCandidate("acme/repo", "123", T0); const lr = l.reservation!; await latch.instance.beginReservedEffect(lr.repo, lr.job_id, lr.owner, lr.token, lr.epoch, lr.path, lr.effect_id); expect((await latch.instance.clearCompletedRedrive(lr.repo, lr.job_id, lr.effect_id)).status).toBe("latched"); expect((await latch.instance.completeRedrive(lr.repo, lr.job_id, lr.owner, lr.token, lr.epoch, lr.effect_id)).status).toBe("cleared_after_completion"); expect(latch.storage.map.has(reserveKey())).toBe(false);
    const tomb = makeDO(); const t = await tomb.instance.reserveRedriveCandidate("acme/repo", "123", T0); const tr = t.reservation!; await tomb.instance.beginReservedEffect(tr.repo, tr.job_id, tr.owner, tr.token, tr.epoch, tr.path, tr.effect_id); expect((await tomb.instance.completeRedrive(tr.repo, tr.job_id, tr.owner, tr.token, tr.epoch, tr.effect_id)).status).toBe("completed"); expect((await tomb.instance.clearCompletedRedrive(tr.repo, tr.job_id, tr.effect_id)).status).toBe("cleared");
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
    const d = makeDO(); const r = await d.instance.reserveRedriveCandidate("  Acme/repo  ", " 000123 ", T0); expect(r.reservation?.repo).toBe("acme/repo"); expect(r.reservation?.job_id).toBe("123"); const p = r.reservation!;
    expect((await d.instance.beginReservedEffect("ACME/REPO", "123", p.owner, "wrong", p.epoch)).status).toBe("stale"); expect((await d.instance.beginReservedEffect("ACME/REPO", "123", p.owner, p.token, p.epoch, "redrive", "wrong-effect")).status).toBe("invalid"); expect((await d.instance.completeRedrive("ACME/REPO", "123", p.owner, p.token, p.epoch, p.effect_id)).status).toBe("incomplete");
  });

  it("does not promote an expired HELD tuple and keeps repo/job indexes independent", async () => {
    const d = makeDO();
    const list = vi.spyOn(d.storage, "list");
    const first = (await d.instance.reserveRedriveCandidate(" Acme/Repo ", "0007", T0)).reservation!;
    expect((await d.instance.beginReservedEffect(first.repo, first.job_id, first.owner, first.token, first.epoch, first.path, first.effect_id, T0 + REDRIVE_RESERVATION_TTL_MS)).status).toBe("ineligible");
    const other = await d.instance.reserveRedriveCandidate("other/repo", "7", T0 + REDRIVE_RESERVATION_TTL_MS);
    expect(other.status).toBe("reserved");
    expect((await d.instance.reserveRedriveCandidate("acme/repo", "7", T0 + REDRIVE_RESERVATION_TTL_MS)).status).toBe("reserved");
    expect(list).not.toHaveBeenCalled();
  });
});

describe("T3-W17 anti-vacuity queue and reservation fences", () => {
  it("keeps the claimed head ahead of later appends and reclaims a pre-permit claim with a new epoch", async () => {
    const d = makeDO();
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
    const d = makeDO();
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
    const d = makeDO();
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
    const d = makeDO();
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
      ["acme/repo", "9007199254740992"],
    ];
    for (const [repo, job] of invalid) {
      const d = makeDO();
      expect((await d.instance.reserveRedriveCandidate(repo, job, T0)).status).toBe("invalid");
      expect(d.storage.map.size).toBe(0);
    }
  });

  it("fences stale reservation tuples before any caller can enter a second effect", async () => {
    const d = makeDO();
    const old = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).reservation!;
    const next = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0 + REDRIVE_RESERVATION_TTL_MS)).reservation!;
    expect(next.epoch).toBe(2);
    expect(await d.instance.beginReservedEffect(old.repo, old.job_id, old.owner, old.token, old.epoch, old.path, old.effect_id)).toMatchObject({ status: "stale" });
    expect((d.storage.map.get(reserveKey()) as ContainmentRedriveReservation)).toMatchObject({ owner: next.owner, token: next.token, epoch: 2, state: "HELD" });
    expect((await d.instance.beginReservedEffect(next.repo, next.job_id, next.owner, next.token, next.epoch, next.path, next.effect_id)).status).toBe("eligible");
    expect((await d.instance.reserveRedriveCandidate(next.repo, next.job_id, T0 + 10 * REDRIVE_RESERVATION_TTL_MS)).status).toBe("effect_eligible");
  });

  it("routes a verified completed webhook through the reservation latch while intake is paused", async () => {
    const d = makeDO();
    const held = (await d.instance.reserveRedriveCandidate("acme/repo", "123", T0)).reservation!;
    await d.instance.beginReservedEffect(held.repo, held.job_id, held.owner, held.token, held.epoch, held.path, held.effect_id);
    const response = await worker.fetch(await webhook(123, "completed-during-pause", "completed"), env(d, kv(), { AUTOSCALER_INTAKE_PAUSED: "1" }), ctx() as never);
    expect(response.status).toBe(200);
    expect(d.storage.map.get(reserveKey())).toMatchObject({ state: "EFFECT_ELIGIBLE", completion_observed: true });
    expect((await d.instance.completeRedrive(held.repo, held.job_id, held.owner, held.token, held.epoch, held.effect_id)).status).toBe("cleared_after_completion");
    expect(d.storage.map.has(reserveKey())).toBe(false);
  });
});

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
    const d = makeDO(); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: kv() } as never);
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
    const d = makeDO();
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
      const authority = authorityProxy(d.instance, { beginEffect: async () => { throw new Error("after claim before permit"); } });
      await expect(runContainmentDrain(envWithAuthority(d, store, authority))).rejects.toThrow("after claim before permit");
      expect((await d.instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", effect_permit: null });
    }
    {
      const { d, store } = await queuedDrain();
      await expect(runContainmentDrain(env(d, store), { claimSpawn: async () => { throw new Error("after permit"); } })).rejects.toThrow("after permit");
      expect((await d.instance.getEvent("evt-1"))).toMatchObject({ state: "CLAIMED", effect_permit: expect.any(Object) });
      expect(store.map.has("spawn:1")).toBe(false);
    }
    {
      const { d, store } = await queuedDrain();
      const claim = vi.fn(async () => { await store.put("spawn:1", "123"); return true; });
      const bind = vi.fn(async () => { throw new Error("after claim KV"); });
      await runContainmentDrain(env(d, store), { claimSpawn: claim, bindContainmentSpawnClaim: bind });
      expect(claim).toHaveBeenCalledTimes(1); expect(bind).toHaveBeenCalledTimes(1); expect(store.map.get("spawn:1")).toBe("123");
      expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
    }
    {
      const { d, store } = await queuedDrain();
      const bind = vi.fn(async () => {}); const drive = vi.fn(async () => { throw new Error("after bind"); });
      await runContainmentDrain(env(d, store), { claimSpawn: async () => true, bindContainmentSpawnClaim: bind, driveSpawn: drive });
      expect(bind).toHaveBeenCalledTimes(1); expect(drive).toHaveBeenCalledTimes(1);
      expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
    }
    {
      const { d, store } = await queuedDrain();
      const authority = authorityProxy(d.instance, { markEffectCommitted: async () => { throw new Error("after drive before commit"); } });
      const drive = vi.fn(async () => {});
      await expect(runContainmentDrain(envWithAuthority(d, store, authority), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: drive })).rejects.toThrow("after drive before commit");
      expect(drive).toHaveBeenCalledTimes(1); expect((await d.instance.getEvent("evt-1"))?.state).toBe("CLAIMED");
    }
    {
      const { d, store } = await queuedDrain();
      const authority = authorityProxy(d.instance, { acknowledge: async () => { throw new Error("after commit before ack"); } });
      const drive = vi.fn(async (_env: unknown, opts: { jobId: string; effect_id?: string; containment_event_id?: string; effect_permit_id?: string }) => {
        await writeDeliveredProof(store, { jobId: opts.jobId, effect_id: opts.effect_id!, containment_event_id: opts.containment_event_id!, effect_permit_id: opts.effect_permit_id! });
      });
      await expect(runContainmentDrain(envWithAuthority(d, store, authority), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: drive as never })).rejects.toThrow("after commit before ack");
      expect(drive).toHaveBeenCalledTimes(1); expect((await d.instance.getEvent("evt-1"))?.state).toBe("EFFECT_COMMITTED");
      expect(await d.instance.snapshot()).toMatchObject({ drain_cursor: 0, backlog_count: 1 });
    }
  });

  it("allows only the deferred old permit holder into the continuation after lease reclaim", async () => {
    const { d, store } = await queuedDrain();
    let releaseOld: (() => void) | undefined; let signalEntered: (() => void) | undefined;
    const oldGate = new Promise<void>((resolve) => { releaseOld = resolve; });
    const entered = new Promise<void>((resolve) => { signalEntered = resolve; });
    let enteredOld = 0; const enteredNew = vi.fn(async () => {});
    const oldRun = runContainmentDrain(env(d, store), {
      claimSpawn: async () => true,
      bindContainmentSpawnClaim: async () => {},
      driveSpawn: async () => { enteredOld++; signalEntered!(); await oldGate; },
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
