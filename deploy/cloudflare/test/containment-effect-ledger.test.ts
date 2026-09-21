import { describe, expect, it, vi } from "vitest";
import { ContainmentEffectLedger, ownerTupleDigest, type ContainmentEffectReceipt, type OwnerTuple } from "../src/containment_effect_ledger";
import { containmentSpawnActiveKey, containmentSpawnAttemptKey } from "../src/containment_effect_route";

const nonce = "0123456789abcdef0123456789abcdef";
const nonce2 = "fedcba9876543210fedcba9876543210";
const tuple = (caller_nonce = nonce): OwnerTuple => ({
  repo: "Acme/Repo", job_id: "123", path: "redrive", event_id: "delivery-1",
  reservation_epoch: 7, effect_id: "containment:v1:redrive:acme/repo/123", owner: "owner-a",
  token: "token-a", lease_epoch: 3, drain_owner: "drain-a", drain_lease_epoch: 3, caller_nonce,
});
const clone = <T>(v: T): T => v === undefined ? v : JSON.parse(JSON.stringify(v)) as T;
class Storage {
  map = new Map<string, unknown>(); private tail = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async transaction<T>(fn: (s: Storage) => Promise<T>): Promise<T> { const run = this.tail.then(async () => { const tx = new Storage(); tx.map = new Map([...this.map].map(([k, v]) => [k, clone(v)])); const out = await fn(tx); this.map = tx.map; return out; }); this.tail = run.then(() => undefined, () => undefined); return run; }
}
function make() {
  const storage = new Storage(); const map = new Map<string, string>();
  const kv = { get: vi.fn(async (k: string) => map.get(k) ?? null), put: vi.fn(async (k: string, v: string) => { map.set(k, v); }), delete: vi.fn(async (k: string) => { map.delete(k); }) };
  return { storage, map, kv, ledger: new ContainmentEffectLedger(storage, kv) };
}
async function claim(ledger: ContainmentEffectLedger, t = tuple()) {
  const request = { schema_version: 1 as const, tuple: t, caller_nonce: t.caller_nonce };
  const prepared = await ledger.prepare(request); expect(prepared.kind).toBe("prepared");
  const acquired = await ledger.acquire(request); expect(acquired.kind).toBe("acquired");
  const mirror = await ledger.mirror(request); expect(mirror.kind).toBe("exact");
  const confirmed = await ledger.confirm({ ...request, observation_kind: mirror.kind, observation_digest: mirror.payload_digest }, mirror.payload_digest!, mirror.payload_digest!);
  expect(confirmed.kind).toBe("permit_issued"); expect(confirmed.permit).not.toBeNull();
  return { request, mirror, confirmed };
}

describe("T3-W17-R14 owner ledger", () => {
  it.each(["start proof", "binding", "mirror"] as const)("does not call a %s-only residue missing", async sidecar => {
    const { ledger, storage, map } = make(); const t = { ...tuple(), repo: "acme/repo" };
    const active = containmentSpawnActiveKey(t); const attempt = containmentSpawnAttemptKey(t);
    const suffix = active.slice("containment:v1:spawn-active:".length);
    if (sidecar === "start proof") storage.map.set(`containment:v1:effect-start:${suffix}`, { stale: true });
    else if (sidecar === "binding") map.set(`containment:v1:effect-binding:${suffix}`, "stale");
    else map.set(`containment:v1:spawn-mirror:${suffix}`, "stale");

    expect((await ledger.observe(active, attempt)).kind).toBe("unknown");
  });

  it("reads pointer and attempt from one consistent storage snapshot", async () => {
    const storage = new Storage(); const values = new Map<string, string>();
    const kv = { get: vi.fn(async (key: string) => values.get(key) ?? null), put: vi.fn(async (key: string, value: string) => { values.set(key, value); }), delete: vi.fn(async (key: string) => { values.delete(key); }) };
    const t = tuple();
    const active = containmentSpawnActiveKey(t); const attempt = containmentSpawnAttemptKey(t);
    const request = { schema_version: 1 as const, tuple: t, caller_nonce: t.caller_nonce };
    let entered = false; let release!: () => void; let signal!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const activeRead = new Promise<void>(resolve => { signal = resolve; });
    const originalGet = storage.get.bind(storage);
    storage.get = async <T>(key: string): Promise<T | undefined> => {
      const value = await originalGet<T>(key);
      if (key === active && !entered) { entered = true; signal(); await gate; }
      return value;
    };
    const ledger = new ContainmentEffectLedger(storage, kv);
    const observed = ledger.observe(active, attempt);
    await Promise.race([activeRead, new Promise(resolve => setTimeout(resolve, 0))]);
    if (entered) { await ledger.prepare(request); release(); }

    expect((await observed).kind).toBe("missing");
  });

  it("uses exact active/attempt/mirror keys and preserves caller nonce", async () => {
    const { ledger, storage, map } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    expect((await ledger.prepare(request)).kind).toBe("prepared");
    expect([...storage.map]).toEqual([
      ["containment:v1:spawn-attempt:acme/repo/123/redrive/containment%3Av1%3Aredrive%3Aacme%2Frepo%2F123/0123456789abcdef0123456789abcdef", expect.anything()],
      ["containment:v1:spawn-active:acme/repo/123/redrive/containment%3Av1%3Aredrive%3Aacme%2Frepo%2F123", expect.anything()],
    ]);
    const pointer = storage.map.get([...storage.map.keys()][1]) as any;
    expect(pointer.created_ms).toBeUndefined(); expect(pointer.effect_started).toBeUndefined();
    expect(pointer.attempt_key).toBe([...storage.map.keys()][0]);
    const again = await ledger.acquire(request); expect(again.kind).toBe("acquired");
    const mirror = await ledger.mirror(request); expect(mirror.key).toContain("spawn-mirror:"); expect(map.has(mirror.key)).toBe(true);
    expect(again.record?.tuple.caller_nonce).toBe(nonce);
  });

  it("uses deterministic nonce-qualified V2 mirrors and projects the confirmation digest into the active pointer", async () => {
    const { ledger, storage, map } = make(); const t = tuple();
    const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    await ledger.prepare(request); await ledger.acquire(request);
    const first = await ledger.mirror(request, "acquired");
    const firstRaw = map.get(first.key);
    const second = await ledger.mirror(request, "acquired");
    expect(first.kind).toBe("exact");
    expect(first.key).toBe(`containment:v1:spawn-mirror:acme/repo/123/redrive/${encodeURIComponent(t.effect_id)}/${nonce}`);
    expect(second.key).toBe(first.key);
    expect(second.payload).toBe(first.payload);
    expect(second.payload_digest).toBe(first.payload_digest);
    expect(map.get(first.key)).toBe(firstRaw);

    const confirmed = await ledger.confirm({ ...request, observation_kind: second.kind, observation_digest: second.payload_digest }, second.payload_digest!, second.payload_digest!);
    expect(confirmed.kind).toBe("permit_issued");
    const attempt = storage.map.get(containmentSpawnAttemptKey(t)) as Record<string, unknown>;
    const pointer = storage.map.get(containmentSpawnActiveKey(t)) as Record<string, unknown>;
    expect(attempt.sidecar_version).toBe(2);
    expect(pointer.sidecar_version).toBe(2);
    expect(pointer.mirror_digest).toBe(second.payload_digest);
    expect("binding_intent" in pointer).toBe(false);

    storage.map.set(containmentSpawnActiveKey(t), { ...pointer, mirror_digest: "f".repeat(64) });
    expect((await ledger.observe(containmentSpawnActiveKey(t), containmentSpawnAttemptKey(t))).kind).toBe("unknown");
  });

  it("requires exact mirror write/readback before issuing a permit", async () => {
    const { ledger } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    await ledger.prepare(request); await ledger.acquire(request); const digest = await ownerTupleDigest(t);
    expect((await ledger.confirm({ ...request, observation_kind: "mismatch", observation_digest: digest }, digest, digest)).kind).toBe("rejected");
    expect((await ledger.acquire(request)).kind).toBe("owned");
  });

  it("authorizes mirror writes only after the canonical claim exists", async () => {
    const { ledger, map } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    expect((await ledger.mirror(request)).kind).toBe("unavailable"); expect(map.size).toBe(0);
    await ledger.prepare(request); expect((await ledger.mirror(request)).kind).toBe("unavailable"); expect(map.size).toBe(0);
    await ledger.acquire(request); expect((await ledger.mirror(request)).kind).toBe("exact");
  });

  it("prevents a stale mirror writer from replacing a newer nonce's canonical mirror", async () => {
    const { ledger, kv, map } = make(); const firstTuple = tuple(nonce);
    const firstRequest = { schema_version: 1 as const, tuple: firstTuple, caller_nonce: nonce };
    expect((await ledger.prepare(firstRequest)).kind).toBe("prepared");
    expect((await ledger.acquire(firstRequest)).kind).toBe("acquired");
    let entered!: () => void; let release!: () => void;
    const putEntered = new Promise<void>(resolve => { entered = resolve; });
    const putGate = new Promise<void>(resolve => { release = resolve; });
    const originalPut = kv.put.getMockImplementation()!;
    kv.put.mockImplementation(async (key, value) => {
      const payload = JSON.parse(value) as { caller_nonce: string };
      if (payload.caller_nonce === nonce) { entered(); await putGate; }
      await originalPut(key, value);
    });

    const staleWrite = ledger.mirror(firstRequest, "acquired");
    await putEntered;
    expect((await ledger.abort(firstRequest)).kind).toBe("aborted");
    const nextTuple = tuple(nonce2);
    const nextRequest = { schema_version: 1 as const, tuple: nextTuple, caller_nonce: nonce2 };
    expect((await ledger.prepare(nextRequest)).kind).toBe("prepared");
    expect((await ledger.acquire(nextRequest)).kind).toBe("acquired");
    const currentMirror = await ledger.mirror(nextRequest, "acquired");
    expect(currentMirror.kind).toBe("exact");
    release();
    const staleResult = await staleWrite;
    expect(staleResult.kind).not.toBe("exact");
    expect(staleResult.key).not.toBe(currentMirror.key);

    const persisted = await kv.get(currentMirror.key);
    expect(persisted).not.toBeNull();
    expect(JSON.parse(persisted!).caller_nonce).toBe(nonce2);
  });

  it("rejects a caller nonce mismatch and preserves a crash-retry nonce", async () => {
    const { ledger } = make(); const t = tuple();
    const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    expect((await ledger.prepare({ ...request, caller_nonce: nonce2 })).kind).toBe("rejected");
    expect((await ledger.prepare(request)).kind).toBe("prepared");
    expect((await ledger.prepare(request)).kind).toBe("busy");
    expect((await ledger.acquire(request)).record?.nonce).toBe(nonce);
  });

  it("requires schema and explicit nonce on every transition", async () => {
    const { ledger } = make(); const t = tuple();
    expect((await ledger.prepare({ tuple: t, caller_nonce: nonce } as any)).kind).toBe("rejected");
    expect((await ledger.beginEffect({ schema_version: 1, tuple: t } as any, "permit")).kind).toBe("rejected");
    expect((await ledger.prepare({ schema_version: 1, tuple: t, caller_nonce: nonce, now: Number.NaN } as any)).kind).toBe("rejected");
  });

  it("fences canonical pointer corruption and leaves stale bind without KV artifacts", async () => {
    const { ledger, storage, map } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    await ledger.prepare(request); const active = [...storage.map.keys()][1]; const pointer: any = storage.map.get(active); pointer.state = "DRIVING";
    expect((await ledger.acquire(request)).kind).toBe("legacy_unknown");
    const c = await claim(ledger, { ...t, effect_id: "effect-bind" }); const started = await ledger.beginEffect(c.request, c.confirmed.permit!.permit_id);
    const attempt = [...storage.map.keys()].find(k => k.includes("effect-bind") && k.includes("spawn-attempt"))!; (storage.map.get(attempt) as any).state = "DRIVING";
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "c".repeat(64) };
    expect((await ledger.bind(c.request, c.confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("unknown");
    expect(map.has("containment:v1:effect-binding:acme/repo/123/redrive/effect-bind")).toBe(false);
  });

  it("rejects a binding replacement observed by the final DO transaction", async () => {
    const { ledger, kv, map } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t); const permit = confirmed.permit!;
    const started = await ledger.beginEffect(request, permit.permit_id); const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "f".repeat(64) };
    let reads = 0; kv.get.mockImplementation(async key => { const raw = map.get(key) ?? null; reads++; if (reads === 2 && raw) { const x = JSON.parse(raw); x.binding.resource_id = "replacement"; return JSON.stringify(x); } return raw; });
    expect((await ledger.bind(request, permit.permit_id, started.proof!.proof_id, binding)).kind).toBe("unknown");
  });

  it("does not overwrite a conflicting binding sidecar that predates the canonical intent", async () => {
    const { ledger, map } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t);
    const started = await ledger.beginEffect(request, confirmed.permit!.permit_id);
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "a".repeat(64) };
    const key = `containment:v1:effect-binding:acme/repo/${t.job_id}/${t.path}/${encodeURIComponent(t.effect_id)}`;
    const preexisting = "foreign-sidecar-must-not-be-replaced";
    map.set(key, preexisting);

    const result = await ledger.bind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding);

    expect(result.kind).not.toBe("bound");
    expect(map.get(key)).toBe(preexisting);
  });

  it("does not replace the canonical binding sidecar on a conflicting post-BOUND retry", async () => {
    const { ledger, map } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t);
    const started = await ledger.beginEffect(request, confirmed.permit!.permit_id);
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "b".repeat(64) };
    expect((await ledger.bind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    const key = `containment:v1:effect-binding:acme/repo/${t.job_id}/${t.path}/${encodeURIComponent(t.effect_id)}`;
    const original = map.get(key);
    const conflict = { ...binding, resource_id: "replacement-resource", binding_sha256: "c".repeat(64) };

    expect((await ledger.bind(request, confirmed.permit!.permit_id, started.proof!.proof_id, conflict)).kind).not.toBe("bound");
    expect(map.get(key)).toBe(original);
  });

  it("fences a conflicting retry against an intent persisted before the KV sidecar", async () => {
    const { ledger, storage, kv, map } = make(); const t = { ...tuple(), repo: "acme/repo" };
    const { request, confirmed } = await claim(ledger, t);
    const beforeKv = structuredClone([...map.entries()]);
    const started = await ledger.beginEffect(request, confirmed.permit!.permit_id);
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "d".repeat(64) };
    kv.put.mockRejectedValueOnce(new Error("simulated pre-sidecar failure"));
    expect((await ledger.bind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("unavailable");
    const attemptKey = containmentSpawnAttemptKey(t);
    const before = storage.map.get(attemptKey) as { binding_intent: unknown };
    expect(before.binding_intent).toBeDefined();
    const replacement = { ...binding, resource_id: "replacement-resource", binding_sha256: "e".repeat(64) };
    kv.put.mockClear();

    expect((await ledger.bind(request, confirmed.permit!.permit_id, started.proof!.proof_id, replacement)).kind).not.toBe("bound");

    expect((storage.map.get(attemptKey) as { binding_intent: unknown }).binding_intent).toEqual(before.binding_intent);
    expect([...map.entries()]).toEqual(beforeKv);
    expect(kv.put).not.toHaveBeenCalled();
  });

  it("returns the existing proof on retries and never authorizes a second drive", async () => {
    const { ledger } = make(); const { request, confirmed } = await claim(ledger); const permit = confirmed.permit!;
    const first = await ledger.beginEffect(request, permit.permit_id); const again = await ledger.beginEffect(request, permit.permit_id);
    expect(again.kind).toBe("already_started"); expect(again.proof?.proof_id).toBe(first.proof?.proof_id);
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "d".repeat(64) };
    await ledger.bind(request, permit.permit_id, first.proof!.proof_id, binding); const drive = await ledger.markDriving(request, permit.permit_id, first.proof!.proof_id);
    const retry = await ledger.markDriving(request, permit.permit_id, first.proof!.proof_id); expect(drive.kind).toBe("driving"); expect(retry.kind).toBe("already_started"); expect(retry.proof?.proof_id).toBe(first.proof?.proof_id);
  });

  it("observes a committed record as UNKNOWN when its typed receipt is altered", async () => {
    const { ledger, storage } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t); const permit = confirmed.permit!;
    const started = await ledger.beginEffect(request, permit.permit_id); const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "e".repeat(64) };
    await ledger.bind(request, permit.permit_id, started.proof!.proof_id, binding); await ledger.markDriving(request, permit.permit_id, started.proof!.proof_id);
    const receipt: ContainmentEffectReceipt = { schema_version: 1, trusted: true, repo: "acme/repo", job_id: "123", path: "redrive", event_id: t.event_id, reservation_epoch: 7, effect_id: t.effect_id, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", nonce, permit_id: permit.permit_id, binding_sha256: binding.binding_sha256, receipt_id: "receipt-1", receipt_sha256: "a".repeat(64), provider_signature: "sig" };
    const committed = await ledger.commitEffect(request, permit.permit_id, started.proof!.proof_id, receipt); expect(committed.kind).toBe("committed");
    const attempt = storage.map.get(committed.attempt_key) as any; attempt.effect_observation.provider = "tampered";
    expect((await ledger.observe(committed.active_pointer_key, committed.attempt_key)).kind).toBe("unknown");
  });

  it("executes prepare/acquire/confirm/bind/DRIVING/commit with identity-bound receipt", async () => {
    const { ledger } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t); const permit = confirmed.permit!;
    const started = await ledger.beginEffect(request, permit.permit_id); expect(started.kind).toBe("already_started");
    const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "b".repeat(64) };
    const bound = await ledger.bind(request, permit.permit_id, started.proof!.proof_id, binding); expect(bound.kind).toBe("bound");
    const driving = await ledger.markDriving(request, permit.permit_id, started.proof!.proof_id); expect(driving.kind).toBe("driving");
    const receipt: ContainmentEffectReceipt = { schema_version: 1, trusted: true, repo: "acme/repo", job_id: "123", path: "redrive", event_id: "delivery-1", reservation_epoch: 7, effect_id: t.effect_id, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", nonce, permit_id: permit.permit_id, binding_sha256: binding.binding_sha256, receipt_id: "receipt-1", receipt_sha256: "a".repeat(64), provider_signature: "sig" };
    expect((await ledger.commitEffect(request, permit.permit_id, started.proof!.proof_id, receipt)).kind).toBe("committed");
  });

  it("fails closed on corrupt, missing, or legacy records", async () => {
    const { ledger, storage, map } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    await ledger.prepare(request); const active = [...storage.map.keys()][1]; storage.map.set(active, { schema_version: 0 });
    expect((await ledger.acquire(request)).kind).toBe("legacy_unknown"); storage.map.delete(active); expect((await ledger.acquire(request)).kind).toBe("legacy_unknown");
    map.set("spawn:123", JSON.stringify({ owner: "legacy" })); expect((await ledger.acquire(request)).kind).toBe("legacy_unknown");
  });

  it("turns an untrusted commit into UNKNOWN and never drives a second time", async () => {
    const { ledger } = make(); const t = tuple(); const { request, confirmed } = await claim(ledger, t); const p = confirmed.permit!; const started = await ledger.beginEffect(request, p.permit_id); const binding = { schema_version: 1 as const, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", binding_sha256: "b".repeat(64) }; await ledger.bind(request, p.permit_id, started.proof!.proof_id, binding); await ledger.markDriving(request, p.permit_id, started.proof!.proof_id);
    const bad: ContainmentEffectReceipt = { schema_version: 1, trusted: true, repo: "other/repo", job_id: "123", path: "redrive", event_id: t.event_id, reservation_epoch: 7, effect_id: t.effect_id, provider: "provider", resource_id: "resource-1", idempotency_key: "idem-1", nonce, permit_id: p.permit_id, binding_sha256: binding.binding_sha256, receipt_id: "receipt-1", receipt_sha256: "a".repeat(64), provider_signature: "sig" };
    expect((await ledger.commitEffect(request, p.permit_id, started.proof!.proof_id, bad)).kind).toBe("unknown"); expect((await ledger.beginEffect(request, p.permit_id)).kind).toBe("rejected");
  });

  it("aborts/reaps only pre-effect, tombstones nonce, and rejects stale tuples", async () => {
    const { ledger } = make(); const t = tuple(); const request = { schema_version: 1 as const, tuple: t, caller_nonce: nonce };
    await ledger.prepare(request); expect((await ledger.abort(request, t.owner, t.token)).kind).toBe("aborted"); expect((await ledger.acquire(request)).kind).toBe("rejected");
    const fresh = { ...request, tuple: tuple(nonce2), caller_nonce: nonce2 }; expect((await ledger.prepare(fresh)).kind).toBe("prepared");
    const claimed = await ledger.acquire(fresh); expect(claimed.kind).toBe("acquired"); const wrong = { ...fresh, tuple: { ...fresh.tuple, token: "stale" } }; expect((await ledger.acquire(wrong)).kind).toBe("legacy_unknown");
    const c = await claim(ledger, { ...tuple("abcdefabcdefabcdefabcdefabcdefab"), effect_id: "effect-2" }); const p = c.confirmed.permit!; expect((await ledger.abort(c.request)).kind).toBe("rejected"); expect((await ledger.reap(c.request, 1, "wrong-authority")).kind).toBe("rejected"); expect((await ledger.reap(c.request, 1)).kind).toBe("rejected"); expect(p.permit_id).toBeTruthy();
  });
});
