import { expect, vi } from "vitest";

import { ContainmentDO, REDRIVE_RESERVATION_TTL_MS, type ContainmentEvent, type ContainmentRedriveReservation } from "../src/index";

export const T0 = 1_750_000_000_000;

function clone<T>(value: T): T { return value === undefined ? value : JSON.parse(JSON.stringify(value)) as T; }
class TxnStorage {
  constructor(private readonly map: Map<string, unknown>) {}
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> { return new Map([...this.map].filter(([key]) => key.startsWith(opts.prefix ?? "")).map(([key, value]) => [key, clone(value) as T])); }
  async transaction<T>(fn: (storage: TxnStorage) => Promise<T>): Promise<T> { return fn(this); }
}
export class FakeStorage {
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

export function ns<T>(instance: T, name = "global") { return { idFromName: vi.fn(() => name), get: vi.fn(() => instance) }; }
export function makeDO(runtimeEnv: Record<string, unknown> = {}) { const storage = new FakeStorage(); const instance = new ContainmentDO({ storage } as never, runtimeEnv as never); return { storage, instance, binding: ns(instance), runtimeEnv }; }
export async function bootstrap(d: ReturnType<typeof makeDO>, job = "1", repo = "acme/repo") {
  const result = await d.instance.bootstrapContainedEventIndex(repo, job);
  expect(result).toMatchObject({ status: "bootstrapped" });
  return result;
}
export function kv(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return { map, get: vi.fn(async (key: string) => map.get(key) ?? null), put: vi.fn(async (key: string, value: string) => { map.set(key, value); }), delete: vi.fn(async (key: string) => { map.delete(key); }), list: vi.fn(async ({ prefix }: { prefix?: string } = {}) => ({ keys: [...map.keys()].filter((key) => key.startsWith(prefix ?? "")).map((name) => ({ name })) })) };
}
export function event(n: number, overrides: Partial<ContainmentEvent> = {}): Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit"> {
  return { schema_version: 1, event_id: `evt-${n}`, received_at_ms: T0, body_sha256: "a".repeat(64), raw_payload: "{}", action: "queued", job_id: String(n), repo: "acme/repo", installation_id: "42", labels: ["corelink"], effect_id: `containment:v1:evt-${n}`, ...overrides };
}
export function ctx() { const tasks: Promise<unknown>[] = []; return { tasks, waitUntil(p: Promise<unknown>) { tasks.push(Promise.resolve(p)); }, passThroughOnException() {} }; }
export async function settle(c: ReturnType<typeof ctx>) { for (let i = 0; i < 8 && c.tasks.length; i++) await Promise.all(c.tasks.splice(0)); }
export function env(d: ReturnType<typeof makeDO>, store = kv(), extra: Record<string, unknown> = {}) {
  d.runtimeEnv.RUNNER_JOB_PATS = store;
  return {
    GITHUB_WEBHOOK_SECRET: "secret", GITHUB_MINT_TOKEN: "mint", RUNNER_JOB_PATS: store, CONTAINMENT: d.binding,
    REPO_INSTALLATION_MAP: JSON.stringify({ "acme/repo": "42" }), RUNNER_CONTAINER: {}, CHECK_HOST_CONTAINER: {},
    CONCURRENCY_SLOTS: ns({ acquire: vi.fn(async () => ({ admitted: true })), release: vi.fn(async () => {}) }),
    CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }), ...extra,
  } as never;
}
export function providerReceipt(opts: { jobId: string; repo: string }) {
  return { resource_id: `job:${opts.repo}/${opts.jobId}`, receipt_id: `receipt-${opts.jobId}`, provider_signature: "test-signature" };
}
export function envWithAuthority(d: ReturnType<typeof makeDO>, store: ReturnType<typeof kv>, authority: unknown, extra: Record<string, unknown> = {}) {
  return { ...env(d, store, extra), CONTAINMENT: ns(authority) } as never;
}
export function authorityProxy(instance: ContainmentDO, overrides: Record<string, (...args: any[]) => unknown>) {
  return new Proxy(instance, {
    get(target, property) {
      const override = overrides[String(property)];
      if (override) return override;
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}
export function reserveKey(repo = "acme/repo", job = "123") { return `containment:v1:reservation:${repo}/${job}`; }
export async function digest(value: string): Promise<string> { const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)); return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join(""); }
export async function webhook(job: number, delivery: string, action = "queued"): Promise<Request> {
  const raw = new TextEncoder().encode(JSON.stringify({ action, workflow_job: { id: job, labels: ["corelink"] }, repository: { full_name: "acme/repo" }, installation: { id: 42 } }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode("secret"), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = await crypto.subtle.sign("HMAC", key, raw);
  const sig = `sha256=${[...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("")}`;
  return new Request("https://worker/webhook", { method: "POST", headers: { "content-type": "application/json", "x-github-event": "workflow_job", "x-hub-signature-256": sig, "x-github-delivery": delivery }, body: raw });
}
export function reservation(storage: FakeStorage, repo = "acme/repo", job = "123", state: ContainmentRedriveReservation["state"] = "HELD", owner = "owner-a", token = "token-a", epoch = 1): ContainmentRedriveReservation {
  const rec: ContainmentRedriveReservation = { schema_version: 1, repo, job_id: job, owner, token, epoch, path: "redrive", state, expires_ms: T0 + REDRIVE_RESERVATION_TTL_MS, event_id: null, effect_id: `containment:v1:redrive:${repo}/${job}`, completion_observed: false };
  storage.map.set(`containment:v1:reservation:${repo}/${job}`, rec); return rec;
}
export async function recoveryFixture(mutate: (records: Record<string, unknown>[]) => void = () => {}) {
  const d = makeDO(); await bootstrap(d, "1"); const store = kv(); const instance = new ContainmentDO({ storage: d.storage }, { RUNNER_JOB_PATS: store } as never);
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
  for (const [kind, sourceKey, sourceValue] of source) records.push({ schema_version: 1, kind, effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: sourceKey, source_value: sourceValue, source_sha256: await digest(sourceValue), ...(kind === "attempt" ? { attempt_count: 1 } : {}) });
  const resultSource = JSON.stringify({ terminal: "DELIVERED", attempt_count: 1, spawn_claim_sha256: records[0].source_sha256, attempt_sha256: records[1].source_sha256, placement_sha256: records[2].source_sha256, lease_sha256: records[3].source_sha256 });
  records.push({ schema_version: 1, kind: "result", effect_id: effect, event_id: "evt-1", job_id: "1", permit_id: permit!.permit_id, source_key: "result", source_value: resultSource, source_sha256: await digest(resultSource), terminal: "DELIVERED", attempt_count: 1 });
  mutate(records);
  for (const rec of records) await store.put(`containment:v1:effect:${encodeURIComponent(effect)}:${rec.kind}`, JSON.stringify(rec));
  await instance.acquireLease("new", T0 + 120_000); await instance.claimNext("new", 2, T0 + 120_000);
  return { d, instance, records, effect };
}
export async function writeDeliveredProof(store: ReturnType<typeof kv>, opts: { jobId: string; effect_id: string; containment_event_id: string; effect_permit_id: string }) {
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
