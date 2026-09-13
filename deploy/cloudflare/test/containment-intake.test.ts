import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const containerSeams = vi.hoisted(() => {
  const start = vi.fn(async () => {});
  const startWithEnv = vi.fn(async () => {});
  const isAlive = vi.fn(async () => false);
  const teardown = vi.fn(async () => {});
  const destroy = vi.fn(async () => {});
  const cutEgress = vi.fn(async () => {});
  const instance = { start, startWithEnv, isAlive, teardown, destroy, cutEgress };
  return { getContainer: vi.fn(() => instance), instance, start, startWithEnv, isAlive, teardown, destroy, cutEgress };
});
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: containerSeams.getContainer,
}));

import worker, { ContainmentDO, MetricsDO, parseContainmentSwitch, retryOrphanedSpawns, type ContainmentEvent } from "../src/index";
import { COUNTER_NAMES } from "../src/metrics";
import { canonicalWorkflowJobIdFromRaw } from "../src/workflow_job_id";
import { runnerCredentialLeaseId } from "../src/lib/runner_credential_lease";

const T0 = 1_750_000_000_000;
const SECRET = "containment-webhook-secret";

function clone<T>(value: T): T { return value === undefined ? value : JSON.parse(JSON.stringify(value)) as T; }

class TxnStorage {
  constructor(private readonly map: Map<string, unknown>) {}
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> {
    return new Map([...this.map].filter(([key]) => key.startsWith(opts.prefix ?? "")).map(([key, value]) => [key, clone(value) as T]));
  }
}

class FakeStorage {
  readonly map = new Map<string, unknown>();
  private tail: Promise<void> = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> {
    return new Map([...this.map].filter(([key]) => key.startsWith(opts.prefix ?? "")).map(([key, value]) => [key, clone(value) as T]));
  }
  async transaction<T>(fn: (storage: TxnStorage) => Promise<T>): Promise<T> {
    const run = this.tail.then(async () => {
      const snapshot = new Map([...this.map].map(([key, value]) => [key, clone(value)]));
      const result = await fn(new TxnStorage(snapshot));
      this.map.clear();
      for (const [key, value] of snapshot) this.map.set(key, value);
      return result;
    });
    this.tail = run.then(() => undefined, () => undefined);
    return run;
  }
}

class FailingStorage extends FakeStorage {
  async transaction<T>(_fn: (storage: TxnStorage) => Promise<T>): Promise<T> {
    throw new Error("injected containment storage failure");
  }
}

function namespace<T>(instance: T, name = "global") {
  return { idFromName: vi.fn(() => name), get: vi.fn(() => instance) };
}

function makeDO() {
  const storage = new FakeStorage();
  const instance = new ContainmentDO({ storage } as never, {} as never);
  return { storage, instance, binding: namespace(instance) };
}

function makeFailingDO() {
  const storage = new FailingStorage();
  const instance = new ContainmentDO({ storage } as never, {} as never);
  return { storage, instance, binding: namespace(instance) };
}

function makeKv(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return {
    map,
    get: vi.fn(async (key: string) => map.get(key) ?? null),
    put: vi.fn(async (key: string, value: string) => { map.set(key, value); }),
    delete: vi.fn(async (key: string) => { map.delete(key); }),
    list: vi.fn(async ({ prefix }: { prefix?: string } = {}) => ({ keys: [...map.keys()].filter((key) => key.startsWith(prefix ?? "")).map((name) => ({ name })) })),
  };
}

function makeMetrics() {
  const storage = new FakeStorage();
  const instance = new MetricsDO({ storage } as never, {} as never);
  return { instance, binding: namespace(instance, "singleton") };
}

function hmac(secret: string, body: Uint8Array): Promise<string> {
  return crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"])
    .then((key) => crypto.subtle.sign("HMAC", key, body))
    .then((mac) => `sha256=${[...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("")}`);
}

async function a317ProofHeader(claim: Record<string, unknown>, key = "a317-proof-key") {
  const encoded = btoa(JSON.stringify(claim)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
  const signing = await crypto.subtle.importKey("raw", new TextEncoder().encode(key), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = new Uint8Array(await crypto.subtle.sign("HMAC", signing, new TextEncoder().encode(`a317:v1\n${encoded}`)));
  return `${encoded}.${btoa(String.fromCharCode(...mac)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "")}`;
}

async function sha256Hex(value: string | Uint8Array): Promise<string> {
  const bytes = typeof value === "string" ? new TextEncoder().encode(value) : value;
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function externalSeams() {
  const acquire = vi.fn(async () => ({ admitted: true }));
  const release = vi.fn(async () => {});
  const readRetry = vi.fn(async () => 0);
  const wipe = vi.fn(async () => {});
  return {
    acquire,
    release,
    readRetry,
    wipe,
    slots: namespace({ acquire, release, readRetry }),
    stash: namespace({ wipe }),
  };
}

function outboxRecords(d: ReturnType<typeof makeDO>) {
  return [...d.storage.map.entries()]
    .filter(([key]) => key.startsWith("containment:v1:outbox:"))
    .map(([, value]) => value as { signal_id: string; state: string; attempts: number });
}

function body(job = 41, repo = "acme/repo", labels = ["corelink"], action = "queued"): Uint8Array {
  return new TextEncoder().encode(JSON.stringify({ action, workflow_job: { id: job, labels }, repository: { full_name: repo }, installation: { id: 7 } }));
}

async function request(raw: Uint8Array, opts: { sig?: string; event?: string; delivery?: string } = {}): Promise<Request> {
  const headers: Record<string, string> = { "content-type": "application/json", "x-github-event": opts.event ?? "workflow_job", "x-hub-signature-256": opts.sig ?? await hmac(SECRET, raw) };
  if (opts.delivery !== undefined) headers["x-github-delivery"] = opts.delivery;
  return new Request("https://worker/webhook", { method: "POST", headers, body: raw });
}

function ctx() {
  const tasks: Promise<unknown>[] = [];
  return { tasks, waitUntil(p: Promise<unknown>) { tasks.push(Promise.resolve(p)); }, passThroughOnException() {} };
}
async function settle(c: ReturnType<typeof ctx>) { for (let n = 0; n < 8 && c.tasks.length; n++) await Promise.all(c.tasks.splice(0)); }

function env(d: ReturnType<typeof makeDO>, kv = makeKv(), metrics = makeMetrics(), extra: Record<string, unknown> = {}) {
  return {
    GITHUB_WEBHOOK_SECRET: SECRET, GITHUB_MINT_TOKEN: "mint", RUNNER_JOB_PATS: kv, CONTAINMENT: d.binding, METRICS: metrics.binding,
    RUNNER_CONTAINER: {}, CHECK_HOST_CONTAINER: {},
    CONCURRENCY_SLOTS: namespace({ acquire: vi.fn(async () => ({ admitted: true })), release: vi.fn(async () => {}), readRetry: vi.fn(async () => 0) }),
    CRED_STASH: namespace({ wipe: vi.fn(async () => {}) }), ...extra,
  } as never;
}

function event(n: number, overrides: Partial<ContainmentEvent> = {}): Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit"> {
  return {
    schema_version: 1, event_id: `evt-${n}`, received_at_ms: T0 + n, body_sha256: "a".repeat(64),
    raw_payload: JSON.stringify({ action: "queued", workflow_job: { id: String(n) } }), action: "queued", job_id: String(n), repo: "acme/repo", installation_id: "7", labels: ["corelink"], effect_id: `containment:v1:evt-${n}`, ...overrides,
  };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T0); vi.clearAllMocks(); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("T3-W17 switch/HMAC intake matrix", () => {
  it("lexically rejects non-canonical workflow job ids before JSON admission", async () => {
    expect(canonicalWorkflowJobIdFromRaw('{"workflow_job":{"id":1}}')).toBe("1");
    expect(canonicalWorkflowJobIdFromRaw('{"workflow_job":{"id":"1"}}')).toBe("1");
    for (const token of ["1e3", "1.0", "[1]", "null", "true", "\" 1\"", "\"01\"", "-1"]) {
      expect(canonicalWorkflowJobIdFromRaw(`{"workflow_job":{"id":${token}}}`)).toBeNull();
    }
    expect(canonicalWorkflowJobIdFromRaw('{"other":{"a":[{"b":1}]},"workflow_job":{"id":1}}')).toBe("1");
    for (const raw of [
      '{"workflow_job":{"id":1},"workflow_job":{"id":2}}',
      '{"workflow_job":{"id":1,"id":2}}',
      '{"workflow_job":{"id":"\\u0031"}}',
      '{"other":{"a":[1}},"workflow_job":{"id":1}}',
      '{"workflow_job":{"id":1}} trailing',
    ]) expect(canonicalWorkflowJobIdFromRaw(raw)).toBeNull();
    const d = makeDO(); const store = makeKv(); const metrics = makeMetrics();
    for (const token of ["1e3", "1.0", "[1]"]) {
      const raw = new TextEncoder().encode(`{"action":"queued","workflow_job":{"id":${token},"labels":["corelink"]},"repository":{"full_name":"acme/repo"},"installation":{"id":7}}`);
      expect((await worker.fetch(await request(raw), env(d, store, metrics), ctx() as never)).status).toBe(400);
    }
    expect(d.storage.map.size).toBe(0); expect(store.put).not.toHaveBeenCalled(); expect(store.delete).not.toHaveBeenCalled();
  });

  it("uses the exact independent 0/1/invalid table", () => {
    expect(parseContainmentSwitch(undefined)).toBe("normal"); expect(parseContainmentSwitch("0")).toBe("normal"); expect(parseContainmentSwitch("1")).toBe("paused");
    for (const raw of ["", " ", "\t", " 0", "0 ", "01", "2", "true", "TRUE", "false", "on", "yes"]) expect(parseContainmentSwitch(raw)).toBe("invalid");
  });

  it("verifies HMAC over exact bytes before UTF-8/JSON parsing", async () => {
    const d = makeDO(); const kv = makeKv(); const metrics = makeMetrics(); const seams = externalSeams();
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 })); vi.stubGlobal("fetch", fetchSpy);
    const metricBump = vi.spyOn(metrics.instance, "bump"); const metricBumpOnce = vi.spyOn(metrics.instance, "bumpOnce");
    const leaseAcquire = vi.spyOn(d.instance, "acquireLease"); const leaseRelease = vi.spyOn(d.instance, "releaseLease");
    const e = env(d, kv, metrics, { CONCURRENCY_SLOTS: seams.slots, CRED_STASH: seams.stash });
    const invalidUtf8 = new Uint8Array([0xff, 0xfe]);
    expect((await worker.fetch(await request(invalidUtf8, { sig: "sha256=00" }), e, ctx() as never)).status).toBe(401);
    expect((await worker.fetch(await request(invalidUtf8), e, ctx() as never)).status).toBe(400);
    expect((await worker.fetch(await request(new TextEncoder().encode("{bad")), e, ctx() as never)).status).toBe(400);
    expect(d.storage.map.size).toBe(0);
    expect(kv.get).not.toHaveBeenCalled(); expect(kv.put).not.toHaveBeenCalled(); expect(kv.delete).not.toHaveBeenCalled(); expect(kv.list).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled(); expect(containerSeams.getContainer).not.toHaveBeenCalled(); expect(containerSeams.start).not.toHaveBeenCalled(); expect(containerSeams.startWithEnv).not.toHaveBeenCalled(); expect(containerSeams.teardown).not.toHaveBeenCalled(); expect(containerSeams.destroy).not.toHaveBeenCalled();
    expect(seams.acquire).not.toHaveBeenCalled(); expect(seams.release).not.toHaveBeenCalled(); expect(seams.wipe).not.toHaveBeenCalled();
    expect(metricBump).not.toHaveBeenCalled(); expect(metricBumpOnce).not.toHaveBeenCalled(); expect(leaseAcquire).not.toHaveBeenCalled(); expect(leaseRelease).not.toHaveBeenCalled();
  });

  it("contains paused and invalid intake independently of redrive switch", async () => {
    const values = ["0", "1", "bogus"] as const;
    for (const intake of values) for (const redrive of values) {
      const d = makeDO(); const c = ctx(); const kv = makeKv(); const metrics = makeMetrics(); const seams = externalSeams();
      const response = await worker.fetch(await request(body(41), { delivery: `d-${intake ?? "normal"}-${redrive ?? "normal"}` }), env(d, kv, metrics, {
        AUTOSCALER_INTAKE_PAUSED: intake,
        AUTOSCALER_REDRIVE_PAUSED: redrive,
        INSTALLATION_ALLOWLIST: "7",
        CONCURRENCY_SLOTS: seams.slots,
        CRED_STASH: seams.stash,
      }), c as never);
      if (intake === "0") {
        expect(response.status).toBe(202);
        expect(await response.json()).toMatchObject({ ok: true, queued: true, job_id: "41" });
        expect(d.storage.map.has("containment:v1:repo-job-index:acme/repo/41")).toBe(true);
        expect((await d.instance.snapshot()).backlog_count).toBe(0);
      } else {
        expect(response.status).toBe(202);
        expect((await d.instance.snapshot()).backlog_count).toBe(1);
      }
      expect(kv.put).not.toHaveBeenCalled(); expect(seams.acquire).not.toHaveBeenCalled(); expect(seams.release).not.toHaveBeenCalled(); expect(seams.wipe).not.toHaveBeenCalled();
      await settle(c);
    }
  });

  it("keeps HMAC, event type, and identity checks ahead of every effect seam", async () => {
    const d = makeDO(); const kv = makeKv(); const metrics = makeMetrics(); const seams = externalSeams();
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 })); vi.stubGlobal("fetch", fetchSpy);
    const admit = vi.spyOn(d.instance, "admitQueued");
    const record = vi.spyOn(d.instance, "recordInvalidConfig");
    const metricBump = vi.spyOn(metrics.instance, "bump"); const metricBumpOnce = vi.spyOn(metrics.instance, "bumpOnce");
    const leaseAcquire = vi.spyOn(d.instance, "acquireLease"); const leaseRelease = vi.spyOn(d.instance, "releaseLease");
    const e = env(d, kv, metrics, { CONCURRENCY_SLOTS: seams.slots, CRED_STASH: seams.stash });
    const malformed = new TextEncoder().encode("{bad");
    expect((await worker.fetch(await request(malformed), e, ctx() as never)).status).toBe(400);
    expect((await worker.fetch(await request(new TextEncoder().encode(JSON.stringify({ action: "queued" })), { event: "push" }), e, ctx() as never)).status).toBe(200);
    expect((await worker.fetch(await request(body(42, "acme/repo", ["other"])), e, ctx() as never)).status).toBe(200);
    const missingJob = new TextEncoder().encode(JSON.stringify({ action: "queued", workflow_job: { labels: ["corelink"] }, repository: { full_name: "acme/repo" }, installation: { id: 7 } }));
    expect((await worker.fetch(await request(missingJob), e, ctx() as never)).status).toBe(400);
    const missingRepo = new TextEncoder().encode(JSON.stringify({ action: "queued", workflow_job: { id: 42, labels: ["corelink"] }, installation: { id: 7 } }));
    expect((await worker.fetch(await request(missingRepo), e, ctx() as never)).status).toBe(400);
    expect(admit).not.toHaveBeenCalled(); expect(record).not.toHaveBeenCalled();
    expect(kv.put).not.toHaveBeenCalled(); expect(seams.acquire).not.toHaveBeenCalled(); expect(seams.release).not.toHaveBeenCalled(); expect(seams.wipe).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled(); expect(containerSeams.getContainer).not.toHaveBeenCalled(); expect(containerSeams.start).not.toHaveBeenCalled(); expect(containerSeams.startWithEnv).not.toHaveBeenCalled(); expect(containerSeams.teardown).not.toHaveBeenCalled(); expect(containerSeams.destroy).not.toHaveBeenCalled();
    expect(metricBump).not.toHaveBeenCalled(); expect(metricBumpOnce).not.toHaveBeenCalled(); expect(leaseAcquire).not.toHaveBeenCalled(); expect(leaseRelease).not.toHaveBeenCalled();
  });

  it("returns 503 for 100 injected authority storage failures with zero external seams", async () => {
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 })); vi.stubGlobal("fetch", fetchSpy);
    for (let n = 0; n < 100; n++) {
      const d = makeFailingDO(); const kv = makeKv(); const metrics = makeMetrics(); const seams = externalSeams(); const c = ctx();
      const metricBump = vi.spyOn(metrics.instance, "bump"); const metricBumpOnce = vi.spyOn(metrics.instance, "bumpOnce");
      const leaseAcquire = vi.spyOn(d.instance, "acquireLease"); const leaseRelease = vi.spyOn(d.instance, "releaseLease");
      const response = await worker.fetch(await request(body(n + 1000), { delivery: `storage-failure-${n}` }), env(d, kv, metrics, {
        AUTOSCALER_INTAKE_PAUSED: "1", RUNNER_JOB_PATS: kv, CONCURRENCY_SLOTS: seams.slots, CRED_STASH: seams.stash,
      }), c as never);
      expect(response.status).toBe(503);
      expect(kv.put).not.toHaveBeenCalled(); expect(kv.delete).not.toHaveBeenCalled(); expect(seams.acquire).not.toHaveBeenCalled(); expect(seams.release).not.toHaveBeenCalled(); expect(seams.wipe).not.toHaveBeenCalled();
      expect(c.tasks).toHaveLength(0);
      expect(fetchSpy).not.toHaveBeenCalled(); expect(containerSeams.getContainer).not.toHaveBeenCalled(); expect(containerSeams.start).not.toHaveBeenCalled(); expect(containerSeams.startWithEnv).not.toHaveBeenCalled(); expect(containerSeams.teardown).not.toHaveBeenCalled(); expect(containerSeams.destroy).not.toHaveBeenCalled();
      expect(metricBump).not.toHaveBeenCalled(); expect(metricBumpOnce).not.toHaveBeenCalled(); expect(leaseAcquire).not.toHaveBeenCalled(); expect(leaseRelease).not.toHaveBeenCalled();
    }
  });

  it("observes the redrive switch independently through the reconciler", async () => {
    for (const redrive of ["0", "1", "bogus"] as const) {
      const d = makeDO(); const metrics = makeMetrics();
      const orphan = { schema_version: 1, jobId: "123", repo: "acme/repo", installationId: "7", labels: ["corelink"], attempts: 0, firstRecordedMs: T0, placedMs: T0 };
      const kv = makeKv({ "orphan:123": JSON.stringify(orphan) });
      const drive = vi.fn(async () => {}); const verify = vi.fn(async () => null);
      const readRetry = vi.fn(async () => 0);
      const e = env(d, kv, metrics, { AUTOSCALER_INTAKE_PAUSED: "1", AUTOSCALER_REDRIVE_PAUSED: redrive, CONCURRENCY_SLOTS: namespace({ readRetry }) });
      await retryOrphanedSpawns(e, ctx() as never, T0, drive, verify);
      if (redrive === "0") {
        expect(kv.list).toHaveBeenCalledWith({ prefix: "orphan:" });
        expect(readRetry).toHaveBeenCalledWith("123");
        expect(drive).not.toHaveBeenCalled(); // the fresh placement remains within grace
      } else {
        expect(readRetry).not.toHaveBeenCalled();
        expect(kv.list).not.toHaveBeenCalled();
        expect(drive).not.toHaveBeenCalled(); expect(kv.get).not.toHaveBeenCalled();
      }
      if (redrive === "bogus") {
        const digest = await sha256Hex("bogus");
        const invalid = [...d.storage.map.entries()].find(([key]) => key === `containment:v1:invalid:AUTOSCALER_REDRIVE_PAUSED:${digest}`);
        expect(invalid?.[1]).toMatchObject({ schema_version: 1, raw_value_sha256: digest, switch_name: "AUTOSCALER_REDRIVE_PAUSED" });
        const firstOutbox = outboxRecords(d); expect(firstOutbox).toHaveLength(1); expect(firstOutbox[0]?.state).toBe("DELIVERED");
        const signal = firstOutbox[0]?.signal_id;
        expect(signal).toBe(await sha256Hex(`containment:v1:config-invalid\nAUTOSCALER_REDRIVE_PAUSED\n${digest}`));
        expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1);
        await retryOrphanedSpawns(e, ctx() as never, T0, drive, verify);
        const secondOutbox = outboxRecords(d); expect(secondOutbox).toHaveLength(1); expect(secondOutbox[0]?.signal_id).toBe(signal); expect(secondOutbox[0]?.state).toBe("DELIVERED");
        expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1); expect(kv.list).not.toHaveBeenCalled(); expect(drive).not.toHaveBeenCalled();
      }
    }
  });

  it("keeps paused intake closed while 100 signed A3.17 proofs enter retry with zero provider seams", async () => {
    const d = makeDO(); const metrics = makeMetrics();
    const acquire = vi.fn(async () => ({ admitted: true })); const start = containerSeams.startWithEnv;
    const e = env(d, makeKv(), metrics, { GITHUB_MINT_TOKEN: undefined, AUTOSCALER_INTAKE_PAUSED: "1", A317_LIVE_PROOF_HMAC_KEY: "a317-proof-key", A317_LIVE_PROOF_BUILD_SHA: "abcdef1", A317_LIVE_PROOF_REPO: "acme/repo", CONCURRENCY_SLOTS: namespace({ acquire, release: vi.fn(async () => {}), readRetry: vi.fn(async () => 0) }) });
    const run = "11111111-1111-4111-8111-111111111111";
    const wrongRun = "22222222-2222-4222-8222-222222222222";
    const storeRun = "33333333-3333-4333-8333-333333333333";
    for (let i = 0; i < 100; i++) {
      const raw = body(10_000 + i, "acme/repo", ["corelink-a317-proof"]); const req = await request(raw, { delivery: `github-delivery-${i}` });
      req.headers.set("x-corelink-a317-proof", await a317ProofHeader({ v: 1, run_id: run, phase: "missing_key", i, exp_ms: T0 + 500_000, build_sha: "abcdef1", installation_id: "7", nonce: `a317-proof-nonce-${String(i).padStart(3, "0")}` }));
      const c = ctx(); expect((await worker.fetch(req, e, c as never)).status).toBe(202); await settle(c);
    }
    const authorizationFetch = vi.fn(async () => new Response("forbidden", { status: 403 })); vi.stubGlobal("fetch", authorizationFetch);
    for (let i = 0; i < 100; i++) {
      const raw = body(20_000 + i, "acme/repo", ["corelink-a317-proof"]); const req = await request(raw, { delivery: `github-wrong-delivery-${i}` });
      req.headers.set("x-corelink-a317-proof", await a317ProofHeader({ v: 1, run_id: wrongRun, phase: "wrong_key", i, exp_ms: T0 + 500_000, build_sha: "abcdef1", installation_id: "7", nonce: `a317-wrong-nonce-${String(i).padStart(3, "0")}` }));
      const c = ctx(); expect((await worker.fetch(req, e, c as never)).status).toBe(202); await settle(c);
    }
    for (let i = 0; i < 100; i++) {
      const raw = body(30_000 + i, "acme/repo", ["corelink-a317-proof"]); const req = await request(raw, { delivery: `github-store-delivery-${i}` });
      req.headers.set("x-corelink-a317-proof", await a317ProofHeader({ v: 1, run_id: storeRun, phase: "store_unavailable", i, exp_ms: T0 + 500_000, build_sha: "abcdef1", installation_id: "7", nonce: `a317-store-nonce-${String(i).padStart(3, "0")}` }));
      expect((await worker.fetch(req, e, ctx() as never)).status).toBe(503);
    }
    const snapshotRequest = new Request("https://worker/internal/v1/a317-live-proof", { headers: { "x-corelink-a317-proof": await a317ProofHeader({ v: 1, run_id: run, phase: "missing_key", i: 0, exp_ms: T0 + 500_000, build_sha: "abcdef1", installation_id: "7", nonce: "a317-proof-nonce-snapshot" }) } });
    expect(await (await worker.fetch(snapshotRequest, e, ctx() as never)).json()).toMatchObject({ run_id: run, accepted: 100, pending: 100, authorization_attempts: 0, authorization_refusals: 0 });
    expect(authorizationFetch).toHaveBeenCalledTimes(100);
    expect(acquire).not.toHaveBeenCalled(); expect(start).not.toHaveBeenCalled();
  }, 15_000);
});

describe("durable intake authority and delivery identity", () => {
  it("fresh paused webhook explicitly bootstraps its repo-job pair before admission", async () => {
    const d = makeDO(); const c = ctx();
    const response = await worker.fetch(await request(body(777)), env(d, makeKv(), makeMetrics(), { AUTOSCALER_INTAKE_PAUSED: "1" }), c as never);
    expect(response.status).toBe(202);
    expect(d.storage.map.get("containment:v1:repo-job-index:acme/repo/777")).toMatchObject({ active_count: 1, active_event_ids: [expect.any(String)] });
    expect(d.storage.map.get("containment:v1:repo-job-index-marker:acme/repo/777")).toMatchObject({ schema_version: 1, repo: "acme/repo", job_id: "777" });
  });

  it("appends an ordered event with canonical schema and deduplicates/conflicts", async () => {
    const d = makeDO();
    await d.instance.bootstrapContainedEventIndex("acme/repo", "1");
    expect((await d.instance.append(event(1))).status).toBe("appended");
    expect((await d.instance.append(event(1))).status).toBe("duplicate");
    expect((await d.instance.append({ ...event(1), body_sha256: "b".repeat(64) })).status).toBe("conflict");
    expect(await d.instance.snapshot()).toMatchObject({ schema_version: 1, next_pause_seq: 2, backlog_count: 1, drain_cursor: 0 });
    expect(d.storage.map.get("containment:v1:pause:00000000000000000001")).toEqual({ schema_version: 1, event_id: "evt-1", pause_seq: 1 });
    expect(await d.instance.getEvent("evt-1")).toMatchObject({ state: "QUEUED", claim: null, effect_permit: null, effect_id: "containment:v1:evt-1" });
  });

  it("allocates 100 distinct pause sequences atomically with exact metadata and pause records", async () => {
    const d = makeDO();
    await Promise.all([...Array(100)].map((_, i) => d.instance.bootstrapContainedEventIndex("acme/repo", String(i + 1))));
    const results = await Promise.all([...Array(100)].map((_, i) => d.instance.append(event(i + 1))));
    expect(results.every((result) => result.status === "appended")).toBe(true);
    expect(await d.instance.snapshot()).toEqual({ schema_version: 1, next_pause_seq: 101, drain_cursor: 0, backlog_count: 100, lease_epoch: 0, lease: null, drain_requested: false });
    for (let i = 1; i <= 100; i++) {
      const seq = String(i).padStart(20, "0");
      expect(d.storage.map.get(`containment:v1:pause:${seq}`)).toEqual({ schema_version: 1, event_id: `evt-${i}`, pause_seq: i });
      expect(await d.instance.getEvent(`evt-${i}`)).toEqual({
        ...event(i), pause_seq: i, state: "QUEUED", claim: null, effect_permit: null,
      });
    }
  });

  it("trims delivery headers and derives byte-sensitive fallback ids", async () => {
    const d = makeDO(); const c = ctx(); const e = env(d, makeKv(), makeMetrics(), { AUTOSCALER_INTAKE_PAUSED: "1" });
    const one = body(10, "acme/repo"); const two = body(10, "acme/other");
    expect((await worker.fetch(await request(one, { delivery: " \td-10\t " }), e, c as never)).status).toBe(202);
    expect((await worker.fetch(await request(one, { delivery: "d-10" }), e, c as never)).status).toBe(202);
    expect((await worker.fetch(await request(two, { delivery: "d-10" }), e, c as never)).status).toBe(409);
    expect((await worker.fetch(await request(two, { delivery: "  " }), e, c as never)).status).toBe(202);
    expect((await d.instance.snapshot()).backlog_count).toBe(2);
  });

  it("uses the exact fallback formula for absent/ASCII-whitespace deliveries and raw-byte differences", async () => {
    const d = makeDO(); const e = env(d, makeKv(), makeMetrics(), { AUTOSCALER_INTAKE_PAUSED: "1" }); const c = ctx();
    const firstRaw = new TextEncoder().encode('{"action":"queued","workflow_job":{"id":10,"labels":["corelink"]},"repository":{"full_name":"acme/repo"},"installation":{"id":7}}');
    const secondRaw = new TextEncoder().encode('{ "action": "queued", "workflow_job": { "id": 10, "labels": ["corelink"] }, "repository": { "full_name": "acme/repo" }, "installation": { "id": 7 } }');
    const expected = async (raw: Uint8Array) => {
      const bodyDigest = await sha256Hex(raw);
      return sha256Hex(`containment:v1\n10\nqueued\n${bodyDigest}`);
    };
    expect((await worker.fetch(await request(firstRaw), e, c as never)).status).toBe(202);
    expect((await worker.fetch(await request(secondRaw, { delivery: "\t\n\v\r " }), e, c as never)).status).toBe(202);
    const ids = [...d.storage.map.keys()].filter((key) => key.startsWith("containment:v1:event:")).map((key) => key.slice("containment:v1:event:".length));
    expect(ids).toEqual(expect.arrayContaining([await expected(firstRaw), await expected(secondRaw)]));
    expect(new Set(ids).size).toBe(2); expect((await d.instance.snapshot()).backlog_count).toBe(2);
  });

  it("accepts completion outside paused intake without claiming unknown credentials revoked", async () => {
    const d = makeDO(); const c = ctx();
    const response = await worker.fetch(await request(body(91, "acme/repo", ["corelink"], "completed"), { delivery: "done-91" }), env(d, makeKv(), makeMetrics(), { AUTOSCALER_INTAKE_PAUSED: "1" }), c as never);
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ revoked: false });
    await settle(c); expect((await d.instance.snapshot()).backlog_count).toBe(0);
  });

  it("seeds containment, then observes completion cleanup while intake remains paused", async () => {
    const d = makeDO(); const kv = makeKv(); const metrics = makeMetrics(); const seams = externalSeams();
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 })); vi.stubGlobal("fetch", fetchSpy);
    const metricBump = vi.spyOn(metrics.instance, "bump");
    // The bare jobId projection is the legacy PAT lookup and remains TTL-bound;
    // durable credential authority state is the revocation source of truth.
    kv.map.set("91", "pat-91"); kv.map.set("jtenant:91", "tenant-91"); kv.map.set("orphan:91", JSON.stringify({ jobId: "91" })); kv.map.set("jhandle:91", "handle-91");
    await d.instance.registerCredential({ jobId: "91", tenant: "tenant-91", patId: "pat-91" });
    await d.instance.bootstrapContainedEventIndex("acme/repo", "91");
    const reservation = await d.instance.reserveRedriveCandidate("acme/repo", "91");
    expect(reservation.status).toBe("reserved");
    await d.instance.append(event(91));
    const clearReservation = vi.spyOn(d.instance, "clearCompletedRedrive");
    const completed = new TextEncoder().encode(JSON.stringify({ action: "completed", workflow_job: { id: 91, labels: ["corelink"], started_at: "2025-06-15T00:00:00.000Z", completed_at: "2025-06-15T00:00:02.000Z" }, repository: { full_name: "acme/repo" }, installation: { id: 7 } }));
    const c = ctx(); const response = await worker.fetch(await request(completed, { delivery: "done-seeded-91-timed" }), env(d, kv, metrics, {
      AUTOSCALER_INTAKE_PAUSED: "1", CONCURRENCY_SLOTS: seams.slots, CRED_STASH: seams.stash,
      CORELINK_RUNNER_MINT_AUTH_KEY: "mint-auth", CORELINK_MINT_URL: "https://corelink.test", BILLING_INGEST_URL: "https://billing.test", BILLING_INGEST_AUTH_KEY: "billing-auth", BILLING_REGION: "iad",
    }), c as never);
    expect(response.status).toBe(200); expect(await response.json()).toMatchObject({ revoked: true, billed: true, ledgered: true, tornDown: true, deduped: false }); await settle(c);
    expect(kv.map.has("orphan:91")).toBe(false); expect(kv.map.has("jhandle:91")).toBe(false); expect(kv.map.has("91")).toBe(true); expect(kv.map.has("jtenant:91")).toBe(false);
    expect(kv.map.has("usage:91")).toBe(true); expect(JSON.parse(kv.map.get("usage:91") as string)).toMatchObject({ jobId: "91", tenant: "tenant-91", region: "iad" });
    expect(seams.release).toHaveBeenCalledWith("91"); expect(seams.wipe).toHaveBeenCalled(); expect(seams.stash.idFromName).toHaveBeenCalledWith(runnerCredentialLeaseId("91", "tenant-91", "pat-91"));
    expect(fetchSpy).toHaveBeenCalledWith("https://corelink.test/internal/v1/runner/revoke", expect.objectContaining({ body: JSON.stringify({ pat_id: "pat-91", owner_tenant: "tenant-91" }) }));
    expect((await d.instance.pendingCredentials({ kind: "job", jobId: "91" })).records).toHaveLength(0); expect(clearReservation).toHaveBeenCalledWith("acme/repo", "91", "containment:v1:redrive:acme/repo/91");
    expect(fetchSpy).toHaveBeenCalledTimes(2); expect(containerSeams.getContainer).toHaveBeenCalled(); expect(containerSeams.teardown).toHaveBeenCalledWith(); expect(metricBump).toHaveBeenCalled();
    expect(d.storage.map.has("containment:v1:reservation:acme/repo/91")).toBe(false);
    expect((await d.instance.snapshot()).backlog_count).toBe(1); expect(d.storage.map.has("containment:v1:pause:00000000000000000001")).toBe(true);
    // A redelivery still re-runs idempotent security cleanup, but the completed
    // signal is deduplicated by the durable `done:<job>` claim.
    const secondCtx = ctx(); const second = await worker.fetch(await request(completed, { delivery: "done-seeded-91-redelivery" }), env(d, kv, metrics, {
      AUTOSCALER_INTAKE_PAUSED: "1", CONCURRENCY_SLOTS: seams.slots, CRED_STASH: seams.stash,
      CORELINK_RUNNER_MINT_AUTH_KEY: "mint-auth", CORELINK_MINT_URL: "https://corelink.test", BILLING_INGEST_URL: "https://billing.test", BILLING_INGEST_AUTH_KEY: "billing-auth", BILLING_REGION: "iad",
    }), secondCtx as never);
    // Terminal revocation remains confirmed on replay, without another remote
    // revoke, exact-PAT ticket wipe or billing push. The historical job-scoped
    // stash still receives its idempotent migration cleanup on both deliveries.
    expect(second.status).toBe(200); expect(await second.json()).toMatchObject({ deduped: true, revoked: true, billed: false, ledgered: false, tornDown: false }); await settle(secondCtx);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
    expect(seams.stash.idFromName.mock.calls).toEqual([
      [runnerCredentialLeaseId("91", "tenant-91", "pat-91")], ["91"], ["91"],
    ]);
    expect((await metrics.instance.snapshot()).webhook_job_completed).toBe(1);
  });

  it("admin drain is key-gated and accepts only an empty body", async () => {
    const d = makeDO(); const c = ctx(); const base = env(d, makeKv(), makeMetrics(), { CONTAINMENT_ADMIN_KEY: "admin", AUTOSCALER_INTAKE_PAUSED: "1" });
    const route = (body: string | undefined, key?: string) => worker.fetch(new Request("https://worker/internal/v1/containment/drain", { method: "POST", headers: key ? { "x-corelink-internal-auth": key } : {}, body }), base, c as never);
    const missingConfig = env(d, makeKv(), makeMetrics(), { AUTOSCALER_INTAKE_PAUSED: "1" });
    expect((await worker.fetch(new Request("https://worker/internal/v1/containment/drain", { method: "POST", body: "" }), missingConfig, c as never)).status).toBe(404);
    expect((await route(undefined)).status).toBe(401); expect((await route("{}", "wrong")).status).toBe(401); expect((await route("{}", "admin")).status).toBe(400); expect((await route(" ", "admin")).status).toBe(400);
    const empty = { schema_version: 1, drain_requested: false, intake_paused: true, backlog_count: 0, drain_cursor: 0 };
    expect(await (await route(undefined, "admin")).json()).toEqual(empty); expect(await (await route("", "admin")).json()).toEqual(empty);
    await d.instance.bootstrapContainedEventIndex("acme/repo", "501"); await d.instance.append(event(501));
    const expected = { schema_version: 1, drain_requested: true, intake_paused: true, backlog_count: 1, drain_cursor: 0 };
    const pausedBacklogCtx = ctx();
    expect(await (await worker.fetch(new Request("https://worker/internal/v1/containment/drain", { method: "POST", headers: { "x-corelink-internal-auth": "admin" }, body: "" }), base, pausedBacklogCtx as never)).json()).toEqual(expected);
    expect(pausedBacklogCtx.tasks).toHaveLength(0);
    expect(await (await route("", "admin")).json()).toEqual(expected);
  });

  it("never bypasses an older backlog on a fresh normal intake", async () => {
    const d = makeDO(); await d.instance.bootstrapContainedEventIndex("acme/repo", "600"); await d.instance.append(event(600)); const c = ctx(); const e = env(d, makeKv(), makeMetrics(), { RUNNER_JOB_PATS: undefined });
    const response = await worker.fetch(await request(body(601), { delivery: "fresh-after-backlog" }), e, c as never);
    expect(response.status).toBe(202); expect(await response.json()).toMatchObject({ contained: true, deduped: false });
    expect(await d.instance.snapshot()).toMatchObject({ next_pause_seq: 3, backlog_count: 2 });
    expect((await d.instance.getEvent("evt-600"))?.pause_seq).toBe(1); expect((await d.instance.getEvent("fresh-after-backlog"))?.pause_seq).toBe(2);
    await settle(c);
  });

  it("refuses a non-allowlisted installation before claim or spawn", async () => {
    const d = makeDO(); const kv = makeKv(); const c = ctx();
    const response = await worker.fetch(await request(body(77)), env(d, kv, makeMetrics(), { INSTALLATION_ALLOWLIST: "99" }), c as never);
    expect(response.status).toBe(202); expect(await response.json()).toMatchObject({ ignored: "installation not allowlisted" }); expect(kv.put).not.toHaveBeenCalled();
  });
});

describe("invalid-config outbox and MetricsDO.bumpOnce", () => {
  it("deduplicates one invalid signal and retries the same outbox id", async () => {
    const d = makeDO(); const digest = await sha256Hex("bogus");
    const first = await d.instance.recordInvalidConfig("AUTOSCALER_INTAKE_PAUSED", "bogus", digest); const second = await d.instance.recordInvalidConfig("AUTOSCALER_INTAKE_PAUSED", "bogus", digest);
    expect(second.signal_id).toBe(first.signal_id); expect((await d.instance.pendingInvalidConfig())).toHaveLength(1); await d.instance.markInvalidConfigAttempt(first.signal_id); await d.instance.acknowledgeInvalidConfig(first.signal_id); expect(await d.instance.pendingInvalidConfig()).toHaveLength(0);
  });

  it("registers containment_config_invalid and atomically deduplicates a signal across 100 calls", async () => {
    expect(COUNTER_NAMES).toContain("containment_config_invalid"); const metrics = makeMetrics();
    expect(metrics.instance.bumpOnce).toBeTypeOf("function");
    await Promise.all([...Array(100)].map(() => metrics.instance.bumpOnce("a".repeat(64), "containment_config_invalid")));
    expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1);
  });

  it("bumps once for the same raw value, but once per distinct raw value", async () => {
    const d = makeDO(); const metrics = makeMetrics(); const e = env(d, makeKv(), metrics, { AUTOSCALER_INTAKE_PAUSED: "bogus" }) as any;
    const first = await worker.fetch(await request(body(901), { delivery: "invalid-same-1" }), e, ctx() as never);
    const second = await worker.fetch(await request(body(902), { delivery: "invalid-same-2" }), e, ctx() as never);
    expect(first.status).toBe(202); expect(second.status).toBe(202);
    expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1);
    e.AUTOSCALER_INTAKE_PAUSED = "also-bogus";
    const third = await worker.fetch(await request(body(903), { delivery: "invalid-different" }), e, ctx() as never);
    expect(third.status).toBe(202); expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(2);
  });

  it("keeps equal raw bytes distinct across supported switch names", async () => {
    const d = makeDO(); const metrics = makeMetrics(); const raw = "same-invalid-value"; const digest = await sha256Hex(raw);
    await d.instance.recordInvalidConfig("AUTOSCALER_INTAKE_PAUSED", raw, digest);
    await d.instance.recordInvalidConfig("AUTOSCALER_REDRIVE_PAUSED", raw, digest);
    const e = env(d, makeKv(), metrics, { AUTOSCALER_INTAKE_PAUSED: "1" });
    await worker.scheduled({} as ScheduledEvent, e, ctx() as never);
    expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(2);
  });

  it("fails closed on unsupported or malformed durable identity records", async () => {
    const d = makeDO(); const digest = await sha256Hex("bogus");
    await expect(d.instance.recordInvalidConfig("UNSUPPORTED_SWITCH", "bogus", digest)).rejects.toThrow();
    await expect(d.instance.recordInvalidConfig("AUTOSCALER_INTAKE_PAUSED", "bogus", "0".repeat(64))).rejects.toThrow();
    d.storage.map.set("containment:v1:invalid:AUTOSCALER_INTAKE_PAUSED:bad", { schema_version: 1, signal_id: "a".repeat(64), switch_name: "AUTOSCALER_INTAKE_PAUSED", raw_value_sha256: "bad" });
    d.storage.map.set(`containment:v1:outbox:${"a".repeat(64)}`, { schema_version: 1, signal_id: "a".repeat(64), state: "PENDING", attempts: 0 });
    expect(await d.instance.pendingInvalidConfig()).toHaveLength(0);
  });

  it("does not duplicate delivery across repeated scheduled retries", async () => {
    const d = makeDO(); const metrics = makeMetrics(); const raw = "scheduled-invalid"; const digest = await sha256Hex(raw);
    await d.instance.recordInvalidConfig("AUTOSCALER_INTAKE_PAUSED", raw, digest);
    const e = env(d, makeKv(), metrics, { AUTOSCALER_INTAKE_PAUSED: "1" });
    await worker.scheduled({} as ScheduledEvent, e, ctx() as never);
    await worker.scheduled({} as ScheduledEvent, e, ctx() as never);
    expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1);
    expect(await d.instance.pendingInvalidConfig()).toHaveLength(0);
  });

  it("retries one stable invalid-config signal through Worker/scheduled across every cross-DO seam", async () => {
    for (const seam of ["record", "pending", "attempt", "metrics", "ack"] as const) {
      const d = makeDO(); const kv = makeKv(); const metrics = makeMetrics(); const c = ctx();
      const e = env(d, kv, metrics, { AUTOSCALER_INTAKE_PAUSED: "bogus" });
      const target = seam === "record" ? vi.spyOn(d.instance, "recordInvalidConfig")
        : seam === "pending" ? vi.spyOn(d.instance, "pendingInvalidConfig")
          : seam === "attempt" ? vi.spyOn(d.instance, "markInvalidConfigAttempt")
            : seam === "ack" ? vi.spyOn(d.instance, "acknowledgeInvalidConfig")
              : vi.spyOn(metrics.instance, "bumpOnce");
      target.mockRejectedValue(new Error(`injected ${seam} failure`));
      const response = await worker.fetch(await request(body(700 + seam.length), { delivery: `invalid-${seam}` }), e, c as never);
      expect(response.status).toBe(seam === "record" ? 503 : 202);
      await settle(c);
      target.mockRestore();
      const before = outboxRecords(d);
      if (seam === "record") expect(before).toHaveLength(0); else expect(before).toHaveLength(1);
      const priorSignal = before[0]?.signal_id;
      await worker.scheduled({} as ScheduledEvent, e, ctx() as never);
      const after = outboxRecords(d);
      expect(after).toHaveLength(1); expect(after[0]?.state).toBe("DELIVERED"); expect(after[0]?.signal_id).toMatch(/^[0-9a-f]{64}$/);
      if (priorSignal) expect(after[0]?.signal_id).toBe(priorSignal);
      expect((await metrics.instance.snapshot()).containment_config_invalid).toBe(1);
    }
  });

  it("keeps an invalid switch fail-closed when the metrics stub lacks bumpOnce, then delivers once after restore", async () => {
    const d = makeDO(); const kv = makeKv(); const fallbackMetrics = makeMetrics();
    const e = env(d, kv, fallbackMetrics, { AUTOSCALER_INTAKE_PAUSED: "bogus" }) as any;
    e.METRICS = namespace({}); // Deliberately no bumpOnce surface.
    const c = ctx(); const response = await worker.fetch(await request(body(811), { delivery: "invalid-no-bump" }), e, c as never);
    expect(response.status).toBe(202); expect((await d.instance.snapshot()).backlog_count).toBe(1);
    await settle(c);
    const pending = await d.instance.pendingInvalidConfig(); expect(pending).toHaveLength(1); expect(outboxRecords(d)[0]?.state).toBe("PENDING");
    const restored = makeMetrics(); e.METRICS = restored.binding;
    await worker.scheduled({} as ScheduledEvent, e, ctx() as never);
    expect(await d.instance.pendingInvalidConfig()).toHaveLength(0); expect((await restored.instance.snapshot()).containment_config_invalid).toBe(1);
    expect(outboxRecords(d)[0]?.state).toBe("DELIVERED"); expect(outboxRecords(d)[0]?.signal_id).toBe(pending[0]?.signal_id);
  });
});
