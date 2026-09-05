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
  const source = ledger as any;
  return {
    ledger: source.ownerPrepare ? source : {
      ownerPrepare: source.prepare.bind(source), ownerAcquire: source.acquire.bind(source), ownerMirror: source.mirror.bind(source),
      ownerConfirm: source.confirm.bind(source), ownerBegin: source.beginEffect.bind(source), ownerBind: source.bind.bind(source),
      ownerMarkDriving: source.markDriving.bind(source), ownerCommit: source.commitEffect.bind(source), ownerObserve: source.observe.bind(source), ownerAbort: source.abort.bind(source),
    }, tuple: t, opts: { jobId: t.job_id }, provider: "fake", resource_id: `job:${t.repo}/${t.job_id}`, idempotency_key: t.effect_id,
    claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; },
    drive: async () => ({ resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "receipt-1", provider_signature: "sig-1" }),
  };
}

describe("canonical containment effect route", () => {
  it("refuses an unauthorized mirror and never drives", async () => {
    const { ledger, kv } = make(); const t = tuple(); let drives = 0; let reads = 0;
    let legacyPermits = 0;
    const originalGet = kv.get;
    kv.get.mockImplementation(async key => { const raw = await originalGet(key); reads++; return raw && reads === 2 ? `${raw}tampered` : raw; });
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeConfirm: async () => { legacyPermits++; return "must-not-issue"; }, drive: async () => { drives++; return undefined; } });
    expect(["unauthorized", "mirror_tampered"]).toContain(result.status); expect(drives).toBe(0); expect(legacyPermits).toBe(0);
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
    let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => { drives++; return { status: "refused" as const, no_effect: true as const }; } });
    expect(result.status).toBe("unknown_terminal");
    const active = [...storage.map.values()].find((v: any) => v && v.state === "DRIVING") as any;
    expect(active?.state).toBe("DRIVING");
    const retry = await runCanonicalEffect({ ...deps(ledger, t), claim: async () => { throw new Error("must not reclaim DRIVING"); }, drive: async () => { drives++; return undefined; } });
    expect(retry.status).toBe("unknown_terminal"); expect(drives).toBe(1);
  });

  it("releases a claim when canonical prepare is already busy", async () => {
    let released = 0; let claimed = 0;
    let legacyPermits = 0;
    const ledger = {
      ownerObserve: async () => ({ kind: "unknown", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "UNKNOWN" }),
      ownerPrepare: async () => ({ kind: "busy", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "PREPARED" }),
    } as any;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), beforeConfirm: async () => { legacyPermits++; return "must-not-issue"; }, claim: async () => { claimed++; return true; }, release: async () => { released++; } });
    expect(result.status).toBe("busy"); expect(claimed).toBe(1); expect(released).toBe(1); expect(legacyPermits).toBe(0);
  });

  it("releases a claim when canonical acquire is already busy", async () => {
    let released = 0;
    const ledger = {
      ownerObserve: async () => ({ kind: "unknown", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "UNKNOWN" }),
      ownerPrepare: async () => ({ kind: "prepared", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "PREPARED" }),
      ownerAcquire: async () => ({ kind: "busy", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: "CLAIM_ACQUIRED" }),
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

  it("releases a permit claim after beforeBegin refusal/throw and retries", async () => {
    const { ledger } = make(); const t = tuple(); let claimed = false; let released = 0; let before = 0; let drives = 0;
    const common = () => ({ ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; },
      beforeBegin: async () => { before++; if (before === 1) return false; if (before === 2) throw new Error("transient"); return true; },
      drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "retry", provider_signature: "sig" }; } });
    expect((await runCanonicalEffect(common())).status).toBe("before_drive_refused");
    expect((await runCanonicalEffect(common())).status).toBe("unavailable");
    expect((await runCanonicalEffect(common())).status).toBe("committed");
    expect(released).toBe(2); expect(drives).toBe(1);
  });

  it("does not resume a pre-effect tuple when this invocation loses the external claim", async () => {
    const { ledger, storage } = make(); const t = tuple(); let claimed = false; let released = 0; let drives = 0;
    const first = { ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; }, beforeBegin: async () => false };
    expect((await runCanonicalEffect(first)).status).toBe("before_drive_refused");
    const snapshot = JSON.stringify([...storage.map.entries()]);
    const second = { ...deps(ledger, t), claim: async () => false, release: async () => { released++; }, drive: async () => { drives++; return undefined; } };
    expect((await runCanonicalEffect(second)).status).toBe("claim_refused");
    expect(JSON.stringify([...storage.map.entries()])).toBe(snapshot); expect(released).toBe(1); expect(drives).toBe(0);
  });

  it("persists a legacy drain permit before canonical PERMIT_ISSUED and binds its id", async () => {
    const { ledger } = make(); const t = { ...tuple(), path: "drain" as const, owner: "drain:lease-owner", token: "drain-token", lease_epoch: 4 };
    const events: string[] = []; const base = deps(ledger, t);
    const originalConfirm = (base.ledger as any).ownerConfirm;
    const originalPrepare = (base.ledger as any).ownerPrepare; const originalAcquire = (base.ledger as any).ownerAcquire; const originalMirror = (base.ledger as any).ownerMirror;
    const route = { ...base, ledger: {
      ...base.ledger,
      ownerPrepare: async (...args: any[]) => { const result = await originalPrepare(...args); events.push("prepare"); return result; },
      ownerAcquire: async (...args: any[]) => { const result = await originalAcquire(...args); events.push("acquire"); return result; },
      ownerMirror: async (...args: any[]) => { const result = await originalMirror(...args); events.push("mirror"); return result; },
      ownerConfirm: async (...args: any[]) => { events.push("owner-confirm"); return originalConfirm(...args); },
    },
      beforeConfirm: async () => { events.push("legacy-permit"); return "legacy-permit-1"; },
      beforeBegin: async permit => { events.push(`before-begin:${permit.permit_id}`); return true; } };
    const result = await runCanonicalEffect(route);
    expect(result.status).toBe("committed"); if (result.status !== "committed") return;
    expect(events.slice(0, 6)).toEqual(["prepare", "acquire", "mirror", "legacy-permit", "owner-confirm", "before-begin:legacy-permit-1"]);
    expect(result.receipt.permit_id).toBe("legacy-permit-1");
  });

  it("fails closed when legacy permit issuance returns null", async () => {
    const { ledger, storage, values } = make(); const t = { ...tuple(), path: "drain" as const };
    let claimed = false; let released = 0; let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; },
      beforeConfirm: async () => undefined, drive: async () => { drives++; return undefined; } });
    expect(result.status).toBe("unavailable");
    expect([...storage.map.values()].every((value: any) => value?.permit_id === null && value?.state !== "PERMIT_ISSUED")).toBe(true);
    expect([...values.values()].every(raw => JSON.parse(raw).permit_id === null)).toBe(true);
    expect(released).toBe(1); expect(drives).toBe(0);
  });

  it("resumes from BOUND after a mark-driving crash without a second begin", async () => {
    const { ledger } = make(); const t = tuple(); let claimed = false; let releases = 0; let markCalls = 0; let drives = 0;
    const base = deps(ledger, t);
    const first = { ...base, claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; releases++; },
      ledger: { ...base.ledger, ownerMarkDriving: async (...args: any[]) => { markCalls++; if (markCalls === 1) throw new Error("crash after bind"); return (base.ledger as any).ownerMarkDriving(...args); },
      }, drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "bound-retry", provider_signature: "sig" }; } };
    expect((await runCanonicalEffect(first)).status).toBe("unavailable");
    const second = { ...base, ledger: first.ledger, claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; releases++; },
      drive: first.drive };
    expect((await runCanonicalEffect(second)).status).toBe("committed");
    expect(markCalls).toBe(2); expect(releases).toBe(1); expect(drives).toBe(1);
  });

  it("releases claim on afterClaim refusal and throw before owner mutation", async () => {
    const { ledger, storage } = make(); const t = tuple(); let claimed = false; let released = 0; let calls = 0;
    const makeAttempt = () => ({ ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; },
      afterClaim: async () => { calls++; if (calls === 2) throw new Error("reservation unavailable"); return false; } });
    expect((await runCanonicalEffect(makeAttempt())).status).toBe("busy");
    expect((await runCanonicalEffect(makeAttempt())).status).toBe("unavailable");
    expect(released).toBe(2); expect(storage.map.size).toBe(0);
  });
});
