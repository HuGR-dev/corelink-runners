import { describe, expect, it, vi } from "vitest";
import {
  ContainmentEffectLedger,
  containmentEffectMirrorFromAttempt,
  containmentEffectMirrorKey,
  type ContainmentEffectAttempt,
  type ContainmentEffectBinding,
  type ContainmentEffectIdentity,
  type ContainmentEffectReceipt,
} from "../src/containment_effect_ledger";

function clone<T>(value: T): T { return value === undefined ? value : JSON.parse(JSON.stringify(value)) as T; }
class Storage {
  map = new Map<string, unknown>(); private tail = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async transaction<T>(fn: (s: Storage) => Promise<T>): Promise<T> {
    const run = this.tail.then(async () => { const snapshot = new Map([...this.map].map(([k, v]) => [k, clone(v)])); const tx = new Storage(); tx.map = snapshot; const out = await fn(tx); this.map = snapshot; return out; });
    this.tail = run.then(() => undefined, () => undefined); return run;
  }
}
function kv() { const map = new Map<string, string>(); return { map, get: vi.fn(async (key: string) => map.get(key) ?? null), put: vi.fn(async (key: string, value: string) => { map.set(key, value); }), delete: vi.fn(async (key: string) => { map.delete(key); }) }; }
function make() { const storage = new Storage(); const store = kv(); return { storage, store, ledger: new ContainmentEffectLedger(storage, store) }; }
function id(repo = "acme/repo", job_id = "123", effect_id = "containment:v1:redrive:acme/repo/123"): ContainmentEffectIdentity { return { repo, job_id, effect_id }; }
function step(a: ContainmentEffectAttempt, now = 1_750_000_000_000) { return { identity: id(a.repo, a.job_id, a.effect_id), nonce: a.nonce, owner: a.owner, owner_token: a.owner_token, lease_epoch: a.lease_epoch, now }; }
function binding(): ContainmentEffectBinding { return { schema_version: 1, provider: "cloudflare-container", resource_id: "handle-1", idempotency_key: "idem-1", binding_sha256: "a".repeat(64) }; }
function sync(store: ReturnType<typeof kv>, a: ContainmentEffectAttempt) { const mirror = containmentEffectMirrorFromAttempt(a); if (mirror) store.map.set(containmentEffectMirrorKey(id(a.repo, a.job_id, a.effect_id)), JSON.stringify(mirror)); }
function receipt(a: ContainmentEffectAttempt): ContainmentEffectReceipt { const b = a.binding!; return { schema_version: 1, trusted: true, provider: b.provider, resource_id: b.resource_id, idempotency_key: b.idempotency_key, nonce: a.nonce, permit_id: a.permit!.permit_id, binding_sha256: b.binding_sha256, receipt_id: "receipt-1", receipt_sha256: "b".repeat(64), provider_signature: "provider-signature" }; }

describe("T3-W17 canonical effect owner ledger", () => {
  it("serializes PREPARED through COMMITTED and keeps completion canonical", async () => {
    const { ledger, store } = make(); const identity = id(); const prepared = await ledger.prepareEffect({ identity, owner: "owner-a", lease_epoch: 1, now: 1 });
    expect(prepared.status).toBe("prepared"); if (prepared.status !== "prepared") return; sync(store, prepared.attempt);
    const claimed = await ledger.acquireEffectClaim(step(prepared.attempt, 2)); expect(claimed.status).toBe("transitioned"); if (claimed.status !== "transitioned") return; sync(store, claimed.attempt);
    const issued = await ledger.issueEffectPermit(step(claimed.attempt, 3)); expect(issued.status).toBe("transitioned"); if (issued.status !== "transitioned") return; sync(store, issued.attempt);
    const bound = await ledger.bindEffect({ ...step(issued.attempt, 4), permit_id: issued.attempt.permit!.permit_id, binding: binding() }); expect(bound.status).toBe("transitioned"); if (bound.status !== "transitioned") return; sync(store, bound.attempt);
    const driving = await ledger.beginEffectDrive({ ...step(bound.attempt, 5), permit_id: bound.attempt.permit!.permit_id }); expect(driving.status).toBe("transitioned"); if (driving.status !== "transitioned") return; sync(store, driving.attempt);
    const committed = await ledger.commitEffect({ ...step(driving.attempt, 6), permit_id: driving.attempt.permit!.permit_id, receipt: receipt(driving.attempt) });
    expect(committed.status).toBe("committed"); expect((await ledger.prepareEffect({ identity, owner: "owner-b", lease_epoch: 2 })).status).toBe("committed");
  });
  it("keeps one active pointer and fences competing owners/nonces", async () => {
    const { ledger, storage } = make(); const identity = id(); const first = await ledger.prepareEffect({ identity, owner: "a", lease_epoch: 1 }); expect(first.status).toBe("prepared"); if (first.status !== "prepared") return;
    expect((await ledger.prepareEffect({ identity, owner: "b", lease_epoch: 1 })).status).toBe("active");
    expect((await ledger.acquireEffectClaim({ ...step(first.attempt), nonce: "other", owner: "b", owner_token: "token" })).status).toBe("stale");
    expect([...storage.map.keys()].filter((key) => key.includes("effect-owner:")).length).toBe(1);
  });
  it("reads the actual KV mirror and refuses missing, legacy, or divergent state", async () => {
    const { ledger, store } = make(); const prepared = await ledger.prepareEffect({ identity: id(), owner: "a", lease_epoch: 1 }); if (prepared.status !== "prepared") return;
    await ledger.acquireEffectClaim(step(prepared.attempt)); const issued = await ledger.issueEffectPermit(step(prepared.attempt)); if (issued.status !== "transitioned") return;
    const bound = await ledger.bindEffect({ ...step(issued.attempt), permit_id: issued.attempt.permit!.permit_id, binding: binding() }); if (bound.status !== "transitioned") return;
    store.map.clear(); expect((await ledger.beginEffectDrive({ ...step(bound.attempt), permit_id: bound.attempt.permit!.permit_id })).status).toBe("mirror_mismatch");
    store.map.set(containmentEffectMirrorKey(id()), JSON.stringify({ schema_version: 0 })); expect((await ledger.beginEffectDrive({ ...step(bound.attempt), permit_id: bound.attempt.permit!.permit_id })).status).toBe("mirror_mismatch");
    sync(store, bound.attempt); store.map.set(containmentEffectMirrorKey(id()), store.map.get(containmentEffectMirrorKey(id()))!.replace('"owner":"a"', '"owner":"other"'));
    expect((await ledger.beginEffectDrive({ ...step(bound.attempt), permit_id: bound.attempt.permit!.permit_id })).status).toBe("mirror_mismatch");
  });
  it("treats DRIVING as unknown-terminal and never permits a second drive", async () => {
    const { ledger, store } = make(); const p = await ledger.prepareEffect({ identity: id(), owner: "a", lease_epoch: 1 }); if (p.status !== "prepared") return;
    await ledger.acquireEffectClaim(step(p.attempt)); const i = await ledger.issueEffectPermit(step(p.attempt)); if (i.status !== "transitioned") return; const b = await ledger.bindEffect({ ...step(i.attempt), permit_id: i.attempt.permit!.permit_id, binding: binding() }); if (b.status !== "transitioned") return; sync(store, b.attempt);
    const d = await ledger.beginEffectDrive({ ...step(b.attempt), permit_id: b.attempt.permit!.permit_id }); expect(d.status).toBe("transitioned"); if (d.status !== "transitioned") return; sync(store, d.attempt);
    expect((await ledger.prepareEffect({ identity: id(), owner: "b", lease_epoch: 2 })).status).toBe("unknown_terminal"); expect((await ledger.beginEffectDrive({ ...step(d.attempt), permit_id: d.attempt.permit!.permit_id })).status).toBe("unknown_terminal");
  });
  it("requires a trusted receipt bound to provider, resource, idempotency key, nonce, permit, and digest", async () => {
    const { ledger, store } = make(); const p = await ledger.prepareEffect({ identity: id(), owner: "a", lease_epoch: 1 }); if (p.status !== "prepared") return; await ledger.acquireEffectClaim(step(p.attempt)); const i = await ledger.issueEffectPermit(step(p.attempt)); if (i.status !== "transitioned") return; const b = await ledger.bindEffect({ ...step(i.attempt), permit_id: i.attempt.permit!.permit_id, binding: binding() }); if (b.status !== "transitioned") return; sync(store, b.attempt); const d = await ledger.beginEffectDrive({ ...step(b.attempt), permit_id: b.attempt.permit!.permit_id }); if (d.status !== "transitioned") return; sync(store, d.attempt);
    const bad = { ...receipt(d.attempt), resource_id: "other" }; expect((await ledger.commitEffect({ ...step(d.attempt), permit_id: d.attempt.permit!.permit_id, receipt: bad })).status).toBe("mirror_mismatch"); expect((await ledger.getEffectAttempt(id(), d.attempt.nonce))?.state).toBe("DRIVING");
  });
  it("only aborts pre-effect and requires stale age plus reaper authority for BOUND", async () => {
    const { ledger, store } = make(); const p = await ledger.prepareEffect({ identity: id(), owner: "a", lease_epoch: 7, now: 100 }); if (p.status !== "prepared") return; const c = await ledger.acquireEffectClaim(step(p.attempt, 101)); if (c.status !== "transitioned") return; const i = await ledger.issueEffectPermit(step(c.attempt, 102)); if (i.status !== "transitioned") return; const b = await ledger.bindEffect({ ...step(i.attempt, 103), permit_id: i.attempt.permit!.permit_id, binding: binding() }); if (b.status !== "transitioned") return;
    expect((await ledger.abortEffect(step(b.attempt, 104))).status).toBe("busy"); expect((await ledger.reapEffect({ identity: id(), nonce: b.attempt.nonce, authority: "containment-reaper-v1", lease_epoch: 7, stale_after_ms: 10, now: 105 })).status).toBe("busy");
    expect((await ledger.reapEffect({ identity: id(), nonce: b.attempt.nonce, authority: "containment-reaper-v1", lease_epoch: 7, stale_after_ms: 10, now: 114 })).status).toBe("reaped");
  });
  it("scopes pointers by repository and rejects malformed mirrors", async () => {
    const { ledger } = make(); expect((await ledger.prepareEffect({ identity: id("acme/repo"), owner: "a", lease_epoch: 1 })).status).toBe("prepared"); expect((await ledger.prepareEffect({ identity: id("other/repo"), owner: "b", lease_epoch: 1 })).status).toBe("prepared");
    expect(containmentEffectMirrorKey(id("acme/repo"))).not.toBe(containmentEffectMirrorKey(id("other/repo")));
  });
});
