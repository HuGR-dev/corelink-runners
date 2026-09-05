import { describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import {
  ContainmentDO,
  type ContainmentEffectAttempt,
  type ContainmentEffectBinding,
  type ContainmentEffectIdentity,
  type ContainmentEffectMirror,
  containmentEffectMirrorKey,
  readContainmentEffectMirror,
} from "../src/index";

function clone<T>(value: T): T { return value === undefined ? value : JSON.parse(JSON.stringify(value)) as T; }
class Storage {
  map = new Map<string, unknown>();
  private tail = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async transaction<T>(fn: (s: Storage) => Promise<T>): Promise<T> {
    const run = this.tail.then(async () => {
      const snapshot = new Map([...this.map].map(([k, v]) => [k, clone(v)]));
      const tx = Object.create(Storage.prototype) as Storage;
      tx.map = snapshot;
      const result = await fn(tx);
      this.map.clear(); for (const [k, v] of snapshot) this.map.set(k, v);
      return result;
    });
    this.tail = run.then(() => undefined, () => undefined);
    return run;
  }
}
function make() { const storage = new Storage(); return { storage, authority: new ContainmentDO({ storage } as never, {} as never) }; }
function identity(repo = "acme/repo", job_id = "123", effect_id = "containment:v1:redrive:acme/repo/123"): ContainmentEffectIdentity { return { repo, job_id, effect_id }; }
function binding(): ContainmentEffectBinding { return { schema_version: 1, provider: "cloudflare-container", resource_id: "handle-1", idempotency_key: "idem-1", binding_sha256: "a".repeat(64) }; }
function mirror(attempt: ContainmentEffectAttempt): ContainmentEffectMirror {
  return { schema_version: 1, repo: attempt.repo, job_id: attempt.job_id, effect_id: attempt.effect_id, nonce: attempt.nonce, owner: attempt.owner, owner_token: attempt.owner_token, state: attempt.state as Exclude<ContainmentEffectMirror["state"], "ABORTED_PRE_EFFECT">, permit_id: attempt.permit?.permit_id ?? null, binding_sha256: attempt.binding?.binding_sha256 ?? null };
}
function step(attempt: ContainmentEffectAttempt, now = 1_750_000_000_000) { return { identity: identity(attempt.repo, attempt.job_id, attempt.effect_id), nonce: attempt.nonce, owner: attempt.owner, owner_token: attempt.owner_token, now }; }

describe("T3-W17 canonical effect owner ledger", () => {
  it("serializes PREPARED through COMMITTED and keeps completion canonical", async () => {
    const { authority } = make(); const id = identity();
    const prepared = await authority.prepareEffect({ identity: id, owner: "owner-a", now: 1 });
    expect(prepared.status).toBe("prepared"); if (prepared.status !== "prepared") return;
    const claim = await authority.acquireEffectClaim(step(prepared.attempt, 2)); expect(claim.status).toBe("transitioned");
    const issued = await authority.issueEffectPermit(step(prepared.attempt, 3)); expect(issued.status).toBe("transitioned"); if (issued.status !== "transitioned") return;
    const bound = await authority.bindEffect({ ...step(issued.attempt, 4), permit_id: issued.attempt.permit!.permit_id, binding: binding() }); expect(bound.status).toBe("transitioned"); if (bound.status !== "transitioned") return;
    const driving = await authority.beginEffectDrive({ ...step(bound.attempt, 5), permit_id: bound.attempt.permit!.permit_id, mirror: mirror(bound.attempt) }); expect(driving.status).toBe("transitioned"); if (driving.status !== "transitioned") return;
    const committed = await authority.commitEffect({ ...step(driving.attempt, 6), permit_id: driving.attempt.permit!.permit_id, mirror: mirror(driving.attempt), provider_receipt: "receipt-1" });
    expect(committed.status).toBe("committed"); expect((await authority.prepareEffect({ identity: id, owner: "owner-b" })).status).toBe("committed");
  });

  it("allows one active pointer and fences competing owners/nonces", async () => {
    const { authority, storage } = make(); const id = identity();
    const first = await authority.prepareEffect({ identity: id, owner: "a" }); expect(first.status).toBe("prepared"); if (first.status !== "prepared") return;
    expect((await authority.prepareEffect({ identity: id, owner: "b" })).status).toBe("active");
    const forged = { ...step(first.attempt), nonce: "other", owner: "b", owner_token: "token" };
    expect((await authority.acquireEffectClaim(forged)).status).toBe("stale");
    expect([...storage.map.keys()].filter((key) => key.includes("effect-owner:")).length).toBe(1);
  });

  it("refuses missing, legacy, and divergent mirrors before DRIVING", async () => {
    const { authority } = make(); const prepared = await authority.prepareEffect({ identity: identity(), owner: "a" }); if (prepared.status !== "prepared") return;
    await authority.acquireEffectClaim(step(prepared.attempt)); const issued = await authority.issueEffectPermit(step(prepared.attempt)); if (issued.status !== "transitioned") return;
    const bound = await authority.bindEffect({ ...step(issued.attempt), permit_id: issued.attempt.permit!.permit_id, binding: binding() }); if (bound.status !== "transitioned") return;
    const args = { ...step(bound.attempt), permit_id: bound.attempt.permit!.permit_id };
    expect((await authority.beginEffectDrive(args)).status).toBe("mirror_unavailable");
    expect((await authority.beginEffectDrive({ ...args, mirror: { schema_version: 0 } as never })).status).toBe("mirror_mismatch");
    expect((await authority.beginEffectDrive({ ...args, mirror: { ...mirror(bound.attempt), owner: "other" } })).status).toBe("mirror_mismatch");
    expect((await authority.beginEffectDrive({ ...args, mirror: mirror(bound.attempt) })).status).toBe("transitioned");
  });

  it("treats DRIVING as unknown-terminal and never permits a second drive", async () => {
    const { authority } = make(); const prepared = await authority.prepareEffect({ identity: identity(), owner: "a" }); if (prepared.status !== "prepared") return;
    await authority.acquireEffectClaim(step(prepared.attempt)); const issued = await authority.issueEffectPermit(step(prepared.attempt)); if (issued.status !== "transitioned") return;
    const bound = await authority.bindEffect({ ...step(issued.attempt), permit_id: issued.attempt.permit!.permit_id, binding: binding() }); if (bound.status !== "transitioned") return;
    const driving = await authority.beginEffectDrive({ ...step(bound.attempt), permit_id: bound.attempt.permit!.permit_id, mirror: mirror(bound.attempt) }); if (driving.status !== "transitioned") return;
    expect((await authority.prepareEffect({ identity: identity(), owner: "b" })).status).toBe("unknown_terminal");
    expect((await authority.beginEffectDrive({ ...step(driving.attempt), permit_id: driving.attempt.permit!.permit_id, mirror: mirror(driving.attempt) })).status).toBe("unknown_terminal");
    expect((await authority.commitEffect({ ...step(driving.attempt), permit_id: driving.attempt.permit!.permit_id, mirror: mirror(driving.attempt), provider_receipt: "" })).status).toBe("invalid");
  });

  it("writes immutable pre-effect tombstones and permits a later nonce", async () => {
    const { authority } = make(); const id = identity(); const prepared = await authority.prepareEffect({ identity: id, owner: "a" }); if (prepared.status !== "prepared") return;
    expect((await authority.abortEffect(step(prepared.attempt))).status).toBe("aborted");
    expect((await authority.reapEffect(step(prepared.attempt))).status).toBe("stale");
    const next = await authority.prepareEffect({ identity: id, owner: "b" }); expect(next.status).toBe("prepared");
  });

  it("scopes pointers by repository and rejects malformed KV mirrors", async () => {
    const { authority } = make(); const a = await authority.prepareEffect({ identity: identity("acme/repo"), owner: "a" }); const b = await authority.prepareEffect({ identity: identity("other/repo"), owner: "b" });
    expect(a.status).toBe("prepared"); expect(b.status).toBe("prepared");
    const kv = { get: vi.fn(async () => JSON.stringify({ schema_version: 0 })) };
    expect(await readContainmentEffectMirror(kv, identity())).toBeNull();
    expect(containmentEffectMirrorKey(identity("acme/repo"))).not.toBe(containmentEffectMirrorKey(identity("other/repo")));
  });
});
