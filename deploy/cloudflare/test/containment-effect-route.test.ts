import { describe, expect, it, vi } from "vitest";
import { ContainmentEffectLedger, type OwnerTuple } from "../src/containment_effect_ledger";
import { intakeOwnerTuple, redriveOwnerTuple, runCanonicalEffect } from "../src/containment_effect_route";

const clone = <T>(value: T): T => value === undefined ? value : JSON.parse(JSON.stringify(value)) as T;
class Storage {
  map = new Map<string, unknown>(); private tail = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.map.get(key) as T); }
  async put(key: string, value: unknown): Promise<void> { this.map.set(key, clone(value)); }
  async transaction<T>(fn: (s: Storage) => Promise<T>): Promise<T> {
    const run = this.tail.then(async () => { const tx = new Storage(); tx.map = new Map([...this.map].map(([k, v]) => [k, clone(v)])); const out = await fn(tx); this.map = tx.map; return out; });
    this.tail = run.then(() => undefined, () => undefined); return run;
  }
}
function make() {
  const storage = new Storage(); const values = new Map<string, string>();
  const kv = { get: vi.fn(async (key: string) => values.get(key) ?? null), put: vi.fn(async (key: string, value: string) => { values.set(key, value); }), delete: vi.fn(async (key: string) => { values.delete(key); }) };
  return { storage, values, kv, ledger: new ContainmentEffectLedger(storage, kv) };
}
function tuple(effect = "containment:v1:intake/acme/repo/123"): OwnerTuple {
  return { repo: "acme/repo", job_id: "123", path: "intake", event_id: "delivery-1", reservation_epoch: null, effect_id: effect, owner: "owner-a", token: "token-a", lease_epoch: 1, drain_owner: null, drain_lease_epoch: null, caller_nonce: "0123456789abcdef0123456789abcdef" };
}
function deps(ledger: ContainmentEffectLedger, t: OwnerTuple) {
  let claimed = false;
  return {
    ledger, tuple: t, opts: { jobId: t.job_id }, provider: "fake", resource_id: `job:${t.repo}/${t.job_id}`, idempotency_key: t.effect_id,
    claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; },
    drive: async () => ({ resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "receipt-1", provider_signature: "sig-1" }),
  };
}

describe("canonical containment effect route", () => {
  it("refuses an unauthorized mirror and never drives", async () => {
    const { ledger, kv } = make(); const t = tuple(); let drives = 0; let reads = 0;
    const originalGet = kv.get;
    kv.get.mockImplementation(async key => { const raw = await originalGet(key); reads++; return raw && reads === 2 ? `${raw}tampered` : raw; });
    const result = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => { drives++; return undefined; } });
    expect(["unauthorized", "mirror_tampered"]).toContain(result.status); expect(drives).toBe(0);
  });

  it("losing claim creates no owner artifact or provider call", async () => {
    const { ledger, storage } = make(); let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), claim: async () => false, drive: async () => { drives++; return undefined; } });
    expect(result.status).toBe("claim_refused"); expect(drives).toBe(0); expect(storage.map.size).toBe(0);
  });

  it("aborts before DRIVING when the pre-drive guard refuses", async () => {
    const { ledger, storage, values } = make(); let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), beforeDrive: async () => false, drive: async () => { drives++; return undefined; } });
    expect(result.status).toBe("before_drive_refused"); expect(drives).toBe(0); expect(storage.map.size).toBe(0); expect(values.size).toBe(0);
  });

  it("returns a complete identity-bound receipt", async () => {
    const { ledger } = make(); const t = tuple();
    const result = await runCanonicalEffect(deps(ledger, t));
    expect(result.status).toBe("committed"); if (result.status !== "committed") return;
    expect(result.receipt).toMatchObject({ repo: t.repo, job_id: t.job_id, path: t.path, event_id: t.event_id, effect_id: t.effect_id, resource_id: `job:${t.repo}/${t.job_id}`, idempotency_key: t.effect_id, trusted: true });
    expect(result.receipt.receipt_sha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it("retries committed finalization without another provider drive", async () => {
    const { ledger } = make(); const t = tuple(); let drives = 0; let finalizations = 0;
    const first = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "r", provider_signature: "s" }; }, finalize: async () => { finalizations++; return false; } });
    const second = await runCanonicalEffect({ ...deps(ledger, t), claim: async () => { throw new Error("claim must not repeat"); }, drive: async () => { drives++; return undefined; }, finalize: async () => { finalizations++; return true; } });
    expect(first.status).toBe("committed"); expect(second.status).toBe("committed"); expect(drives).toBe(1); expect(finalizations).toBe(2);
  });

  it("turns a provider crash into UNKNOWN and never drives it twice", async () => {
    const { ledger } = make(); const t = tuple(); let drives = 0;
    const first = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => { drives++; throw new Error("crash"); } });
    const second = await runCanonicalEffect({ ...deps(ledger, t), claim: async () => { throw new Error("claim must not repeat"); }, drive: async () => { drives++; return undefined; } });
    expect(first.status).toBe("unknown_terminal"); expect(second.status).toBe("unknown_terminal"); expect(drives).toBe(1);
  });

  it("treats a no-effect provider refusal after DRIVING as UNKNOWN", async () => {
    const { ledger, storage } = make(); const t = tuple();
    const result = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => ({ status: "refused" as const, no_effect: true as const }) });
    expect(result.status).toBe("unknown_terminal");
    const active = [...storage.map.values()].find((v: any) => v && v.state === "DRIVING") as any;
    expect(active?.state).toBe("DRIVING");
  });

  it("releases a claim when canonical prepare is already busy", async () => {
    let released = 0; let claimed = 0;
    const ledger = {
      observe: async () => ({ kind: "unknown", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "UNKNOWN" }),
      prepare: async () => ({ kind: "busy", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "PREPARED" }),
    } as any;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), claim: async () => { claimed++; return true; }, release: async () => { released++; } });
    expect(result.status).toBe("busy"); expect(claimed).toBe(1); expect(released).toBe(1);
  });

  it("releases a claim when canonical acquire is already busy", async () => {
    let released = 0;
    const ledger = {
      observe: async () => ({ kind: "unknown", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "UNKNOWN" }),
      prepare: async () => ({ kind: "prepared", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "PREPARED" }),
      acquire: async () => ({ kind: "busy", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "CLAIM_ACQUIRED" }),
    } as any;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), release: async () => { released++; } });
    expect(result.status).toBe("busy"); expect(released).toBe(1);
  });

  it("orders redrive release, claim, reservation eligibility, then drive", async () => {
    const events: string[] = []; const { ledger } = make(); const t = await redriveOwnerTuple("acme/repo", "123", "containment:v1:redrive:acme/repo/123", "owner", "token", 7);
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim: async () => { events.push("release"); }, claim: async () => { events.push("claim"); return true; }, afterClaim: async () => { events.push("eligible"); return true; }, drive: async () => { events.push("drive"); return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "r", provider_signature: "s" }; } });
    expect(result.status).toBe("committed"); expect(events).toEqual(["release", "claim", "eligible", "drive"]);
  });

  it("orders orphan mutation before claim and eligibility", async () => {
    const events: string[] = []; const { ledger } = make(); const t = await redriveOwnerTuple("acme/repo", "123", "containment:v1:redrive:acme/repo/123", "owner", "token", 7);
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim: async () => { events.push("mutation"); }, claim: async () => { events.push("claim"); return true; }, afterClaim: async () => { events.push("eligible"); return true; }, drive: async () => { events.push("drive"); return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "r", provider_signature: "s" }; } });
    expect(result.status).toBe("committed"); expect(events).toEqual(["mutation", "claim", "eligible", "drive"]);
  });

  it("derives retry-stable nonces from the full logical attempt", async () => {
    const first = await intakeOwnerTuple("acme/repo", "123", "effect", "event");
    const same = await intakeOwnerTuple("acme/repo", "123", "effect", "event");
    const reclaimed = await redriveOwnerTuple("acme/repo", "123", "effect", "owner-b", "token-b", 2);
    expect(first.caller_nonce).toBe(same.caller_nonce); expect(reclaimed.caller_nonce).not.toBe(first.caller_nonce);
  });

  it("allows only one concurrent winner for an effect tuple", async () => {
    const { ledger } = make(); const t = tuple(); let claimed = false; let drives = 0;
    const common = { ...deps(ledger, t), claim: async () => { if (claimed) return false; claimed = true; return true; }, drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: `r-${drives}`, provider_signature: "s" }; } };
    const results = await Promise.all([runCanonicalEffect(common), runCanonicalEffect(common)]);
    expect(results.filter(x => x.status === "committed")).toHaveLength(1); expect(drives).toBe(1);
  });
});
