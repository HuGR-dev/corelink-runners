import { describe, expect, it, vi } from "vitest";
import { ContainmentEffectLedger, type OwnerTuple } from "../src/containment_effect_ledger";
import { containmentSpawnActiveKey, containmentSpawnAttemptKey, drainOwnerTuple, intakeOwnerTuple, redriveOwnerTuple, runCanonicalEffect } from "../src/containment_effect_route";

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
function drainTuple(base = tuple()): OwnerTuple { return { ...base, path: "drain", drain_owner: base.owner, drain_lease_epoch: base.lease_epoch }; }
function legacyPermit(t: OwnerTuple, permitId: string) { return { permit_id: permitId, issued_to_owner: t.owner, issued_to_epoch: t.lease_epoch }; }
function deps(ledger: ContainmentEffectLedger, t: OwnerTuple) {
  let claimed = false;
  const source = ledger as any;
  return {
    ledger: source.ownerPrepare ? source : {
      ownerPrepare: source.prepare.bind(source), ownerAcquire: source.acquire.bind(source), ownerMirror: source.mirror.bind(source),
      ownerConfirm: source.confirm.bind(source), ownerBegin: source.beginEffect.bind(source), ownerBind: source.bind.bind(source),
      ownerMarkDriving: source.markDriving.bind(source), ownerCommit: source.commitEffect.bind(source), ownerObserve: source.observe.bind(source), ownerAbort: source.abort.bind(source), ownerFreeze: source.freezeUnknown.bind(source),
    }, tuple: t, opts: { jobId: t.job_id }, provider: "fake", resource_id: `job:${t.repo}/${t.job_id}`, idempotency_key: t.effect_id,
    claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; },
    drive: async () => ({ resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "receipt-1", provider_signature: "sig-1" }),
  };
}

describe("canonical containment effect route", () => {
  it("treats a same-tuple ABORTED_PRE_EFFECT tombstone as terminal before claim or preparation", async () => {
    const { ledger } = make(); const t = tuple();
    const request = { schema_version: 1 as const, tuple: t, caller_nonce: t.caller_nonce };
    expect((await ledger.prepare(request)).kind).toBe("prepared");
    expect((await ledger.abort(request, t.owner, t.token)).kind).toBe("aborted");
    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => true); const drive = vi.fn(async () => undefined);

    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim, claim, drive });

    expect(result).toMatchObject({ status: "unknown_terminal" });
    expect(result).not.toHaveProperty("retryable");
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled(); expect(drive).not.toHaveBeenCalled();
  });

  it("keeps live DRIVING retryable while the original provider can still commit", async () => {
    vi.useFakeTimers(); vi.setSystemTime(5_000);
    let release!: () => void; let signal!: () => void;
    const providerGate = new Promise<void>(resolve => { release = resolve; });
    const providerEntered = new Promise<void>(resolve => { signal = resolve; });
    const { ledger } = make(); const t = tuple();
    const firstDrive = vi.fn(async () => {
      signal(); await providerGate;
      return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "live-receipt", provider_signature: "live-signature" };
    });
    try {
      const first = runCanonicalEffect({ ...deps(ledger, t), drive: firstDrive });
      await providerEntered;
      const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => { throw new Error("must not replace live DRIVING owner"); });
      const duplicateDrive = vi.fn(async () => undefined);
      const concurrent = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim, claim, drive: duplicateDrive });

      expect(concurrent).toMatchObject({ status: "unknown_terminal", retryable: true });
      expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled(); expect(duplicateDrive).not.toHaveBeenCalled();
      release();
      expect((await first).status).toBe("committed");
      expect(firstDrive).toHaveBeenCalledTimes(1);
    } finally {
      release(); vi.useRealTimers();
    }
  });

  it.each(["start proof", "binding", "mirror"] as const)("fails closed without owners when a %s sidecar survives", async sidecar => {
    const { ledger, storage, values } = make(); const t = tuple();
    const activeKey = containmentSpawnActiveKey(t);
    const suffix = activeKey.slice("containment:v1:spawn-active:".length);
    if (sidecar === "start proof") storage.map.set(`containment:v1:effect-start:${suffix}`, { stale: true });
    else if (sidecar === "binding") values.set(`containment:v1:effect-binding:${suffix}`, "stale");
    else values.set(`containment:v1:spawn-mirror:${suffix}`, "stale");
    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => true); const drive = vi.fn(async () => ({ resource_id: "resource", receipt_id: "r", provider_signature: "s" }));

    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim, claim, drive });

    expect(result).toMatchObject({ status: "unknown_terminal" });
    expect(result).not.toHaveProperty("retryable");
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled(); expect(drive).not.toHaveBeenCalled();
  });

  it.each(["permit", "start proof", "binding", "mirror"] as const)("does not retry malformed complete DRIVING %s evidence", async kind => {
    const { ledger, storage, values } = make(); const t = tuple();
    const failedProvider = await runCanonicalEffect({ ...deps(ledger, t), drive: async () => { throw new Error("provider response lost"); } });
    expect(failedProvider).toMatchObject({ status: "unknown_terminal", retryable: true });
    const attemptKey = containmentSpawnAttemptKey(t);
    if (kind === "permit") {
      const attempt = storage.map.get(attemptKey) as any;
      attempt.permit.permit_id = `${attempt.permit_id}-mismatch`;
      storage.map.set(attemptKey, attempt);
    } else {
      const activeKey = containmentSpawnActiveKey(t);
      const suffix = activeKey.slice("containment:v1:spawn-active:".length);
      if (kind === "start proof") storage.map.set(`containment:v1:effect-start:${suffix}`, { corrupted: true });
      else if (kind === "binding") values.set(`containment:v1:effect-binding:${suffix}`, "corrupted");
      else values.set(`containment:v1:spawn-mirror:${suffix}`, "corrupted");
    }
    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => { throw new Error("must not reacquire malformed DRIVING owner"); });
    const retry = await runCanonicalEffect({ ...deps(ledger, t), beforeClaim, claim, drive: async () => undefined });

    expect(retry).toMatchObject({ status: "unknown_terminal" });
    expect(retry).not.toHaveProperty("retryable");
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled();
  });

  it.each(["PERMIT_ISSUED", "BOUND"] as const)("rejects a nested/top permit mismatch in %s before retry preparation", async state => {
    const { ledger, storage } = make(); const t = tuple();
    const base = deps(ledger, t);
    const first = state === "PERMIT_ISSUED"
      ? await runCanonicalEffect({ ...base, beforeBegin: async () => false })
      : await runCanonicalEffect({ ...base, ledger: { ...base.ledger, ownerMarkDriving: async () => { throw new Error("crash after bind"); } } });
    expect(first.status).toBe(state === "PERMIT_ISSUED" ? "before_drive_refused" : "unavailable");
    const attemptKey = containmentSpawnAttemptKey(t);
    const attempt = storage.map.get(attemptKey) as any;
    expect(attempt.state).toBe(state);
    attempt.permit.permit_id = `${attempt.permit_id}-mismatch`;
    storage.map.set(attemptKey, attempt);

    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => true);
    const retry = await runCanonicalEffect({ ...base, beforeClaim, claim });

    expect(retry).toMatchObject({ status: "unknown_terminal" });
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled();
  });

  it.each(["intake", "redrive", "drain without strict admission"] as const)("does not replace a reaped predecessor on %s", async path => {
    const { ledger } = make(); const base = tuple();
    const t = path === "intake" ? base : path === "redrive"
      ? { ...base, path: "redrive" as const, reservation_epoch: 3, drain_owner: base.owner, drain_lease_epoch: base.lease_epoch }
      : drainTuple(base);
    const common = deps(ledger, t);
    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => true);
    const result = await runCanonicalEffect({
      ...common,
      ledger: { ...common.ledger, ownerObserve: async () => ({ kind: "reaped_predecessor", schema_version: 1, tuple_digest: "", attempt_key: "a", active_pointer_key: "p", permit: null, proof: null, state: "ABORTED_PRE_EFFECT" }) as any },
      allowFencedDrainPredecessor: true,
      ...(path === "drain without strict admission" ? {} : { admit: async () => true }),
      beforeClaim, claim,
    });

    expect(result).toMatchObject({ status: "unknown_terminal" });
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled();
  });

  it("refuses an unauthorized mirror and never drives", async () => {
    const { ledger, kv, values } = make(); const t = tuple(); let drives = 0; let reads = 0;
    let legacyPermits = 0;
    kv.get.mockImplementation(async key => { const raw = values.get(key) ?? null; reads++; return key.includes("spawn-mirror:") && raw && reads >= 3 ? `${raw}tampered` : raw; });
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeConfirm: async () => { legacyPermits++; return null; }, drive: async () => { drives++; return undefined; } });
    expect(["unauthorized", "mirror_tampered"]).toContain(result.status); expect(drives).toBe(0); expect(legacyPermits).toBe(0);
  });

  it("losing claim creates no owner artifact or provider call", async () => {
    const { ledger, storage } = make(); let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), claim: async () => false, drive: async () => { drives++; return undefined; } });
    expect(result.status).toBe("claim_refused"); expect(drives).toBe(0); expect(storage.map.size).toBe(0);
  });

  it("abandons pre-claim preparation when the external claim is refused", async () => {
    const { ledger } = make(); let abandoned = 0;
    const result = await runCanonicalEffect({
      ...deps(ledger, tuple()),
      claim: async () => false,
      abandonPreparation: async () => { abandoned++; },
    });
    expect(result.status).toBe("claim_refused"); expect(abandoned).toBe(1);
  });

  it("retains preparation once DRIVING authorized the provider", async () => {
    const { ledger } = make(); let abandoned = 0;
    const result = await runCanonicalEffect({
      ...deps(ledger, tuple()),
      abandonPreparation: async () => { abandoned++; },
    });
    expect(result.status).toBe("committed"); expect(abandoned).toBe(0);
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
    const result = await runCanonicalEffect({ ...deps(ledger, tuple()), beforeConfirm: async () => { legacyPermits++; return null; }, claim: async () => { claimed++; return true; }, release: async () => { released++; } });
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
    const result = await runCanonicalEffect({ ...deps(ledger, t), admit: async () => { events.push("eligible"); return true; }, beforeClaim: async () => { events.push("release"); }, claim: async () => { events.push("claim"); return true; }, drive: async () => { events.push("drive"); return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "r", provider_signature: "s" }; } });
    expect(result.status).toBe("committed"); expect(events).toEqual(["eligible", "release", "claim", "drive"]);
  });

  it("orders orphan mutation before claim and eligibility", async () => {
    const events: string[] = []; const { ledger } = make(); const t = await redriveOwnerTuple("acme/repo", "123", "containment:v1:redrive:acme/repo/123", "owner", "token", 7);
    const result = await runCanonicalEffect({ ...deps(ledger, t), admit: async () => { events.push("eligible"); return true; }, beforeClaim: async () => { events.push("mutation"); }, claim: async () => { events.push("claim"); return true; }, drive: async () => { events.push("drive"); return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "r", provider_signature: "s" }; } });
    expect(result.status).toBe("committed"); expect(events).toEqual(["eligible", "mutation", "claim", "drive"]);
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
    const { ledger } = make(); const t = await drainOwnerTuple("acme/repo", "123", "containment:v1:drain", "event-drain", "lease-owner", 4);
    expect(t).toMatchObject({ path: "drain", owner: "lease-owner", drain_owner: "lease-owner", lease_epoch: 4, drain_lease_epoch: 4 });
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
      beforeConfirm: async permitId => { events.push("legacy-permit"); return legacyPermit(t, permitId); },
      beforeBegin: async permit => { events.push(`before-begin:${permit.permit_id}`); return true; } };
    const result = await runCanonicalEffect(route);
    expect(result.status).toBe("committed"); if (result.status !== "committed") return;
    expect(events.slice(0, 5)).toEqual(["prepare", "acquire", "mirror", "legacy-permit", "owner-confirm"]);
    expect(events[5]).toMatch(/^before-begin:containment:v1:legacy-permit:/);
    expect(result.receipt.permit_id).toMatch(/^containment:v1:legacy-permit:/);
  });

  it("fails closed when legacy permit issuance returns null", async () => {
    const { ledger, storage, values } = make(); const t = drainTuple();
    let claimed = false; let released = 0; let drives = 0;
    const result = await runCanonicalEffect({ ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; },
      beforeConfirm: async () => undefined, drive: async () => { drives++; return undefined; } });
    expect(result.status).toBe("unavailable");
    expect([...storage.map.values()].every((value: any) => value?.permit_id === null && value?.state !== "PERMIT_ISSUED")).toBe(true);
    expect([...values.values()].every(raw => JSON.parse(raw).permit_id === null)).toBe(true);
    expect(released).toBe(1); expect(drives).toBe(0);
  });

  it("retries a throw-before-commit with the same deterministic permit ID", async () => {
    const { ledger } = make(); const t = drainTuple(); let drives = 0; let calls = 0; let candidate = "";
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeConfirm: async permitId => { candidate = permitId; calls++; if (calls === 1) throw new Error("response lost before commit"); return legacyPermit(t, permitId); }, drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "retry", provider_signature: "sig" }; } });
    expect(result.status).toBe("committed"); expect(calls).toBe(2); expect(drives).toBe(1); if (result.status === "committed") expect(result.receipt.permit_id).toBe(candidate);
  });

  it("uses the same persisted permit after a legacy response loss", async () => {
    const { ledger } = make(); const t = drainTuple(); let drives = 0; let calls = 0; let persisted: ReturnType<typeof legacyPermit> | null = null;
    const result = await runCanonicalEffect({ ...deps(ledger, t), beforeConfirm: async permitId => { calls++; persisted = persisted ?? legacyPermit(t, permitId); if (calls === 1) throw new Error("response lost after commit"); return persisted; }, drive: async () => { drives++; return { resource_id: `job:${t.repo}/${t.job_id}`, receipt_id: "readback", provider_signature: "sig" }; } });
    expect(result.status).toBe("committed"); if (result.status !== "committed") return;
    expect(result.receipt.permit_id).toBe(persisted!.permit_id); expect(drives).toBe(1);
  });

  it("freezes ambiguous legacy response and blocks a cross-lease drive", async () => {
    const { ledger, storage } = make(); const t = drainTuple(); let drives = 0; let firstRelease = 0;
    const first = await runCanonicalEffect({ ...deps(ledger, t), beforeConfirm: async () => { throw new Error("ambiguous"); }, release: async () => { firstRelease++; }, drive: async () => { drives++; return undefined; } });
    expect(first.status).toBe("unknown_terminal");
    const attempt = storage.map.get(containmentSpawnAttemptKey(t)) as any;
    const active = storage.map.get(containmentSpawnActiveKey(t)) as any;
    expect(attempt).toBeDefined(); expect(active).toBeDefined();
    expect(attempt.state).toBe("UNKNOWN"); expect(active.state).toBe("UNKNOWN");
    expect(attempt.tuple).toEqual(t); expect(active.tuple).toEqual(t);
    expect(attempt.caller_nonce).toBe(t.caller_nonce); expect(active.caller_nonce).toBe(t.caller_nonce);
    expect(attempt.permit_id).toBeNull(); expect(attempt.binding_id).toBeNull(); expect(attempt.effect_start_proof_id).toBeNull(); expect(attempt.effect_started).toBe(false);
    expect(active.permit_id).toBeNull(); expect(active.binding_id).toBeNull(); expect(active.effect_start_proof_id).toBeNull();
    const next = { ...t, owner: "drain:other", token: "drain-other", lease_epoch: 3, caller_nonce: "1234567890abcdef1234567890abcdef" };
    const beforeClaim = vi.fn(async () => {}); const claim = vi.fn(async () => { throw new Error("must not claim frozen owner"); });
    const second = await runCanonicalEffect({ ...deps(ledger, next), beforeClaim, claim, beforeConfirm: async () => { throw new Error("must not be reached"); }, drive: async () => { drives++; return undefined; } });
    expect(second.status).toBe("unknown_terminal"); expect(second).not.toHaveProperty("retryable");
    expect(drives).toBe(0); expect(firstRelease).toBe(0);
    expect(beforeClaim).not.toHaveBeenCalled(); expect(claim).not.toHaveBeenCalled();
  });

  it("does not claim an unverified freeze and retains the current claim", async () => {
    const { ledger } = make(); const t = drainTuple(); let released = 0;
    const base = deps(ledger, t);
    const result = await runCanonicalEffect({ ...base, release: async () => { released++; },
      ledger: { ...base.ledger, ownerFreeze: async () => ({ kind: "unknown", state: "UNKNOWN", schema_version: 1, tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null }) } as any,
      beforeConfirm: async () => { throw new Error("ambiguous"); }, drive: async () => undefined });
    expect(result.status).toBe("unavailable"); expect(released).toBe(0);
  });

  it("freezes mismatched retry permits without downstream effect", async () => {
    for (const mismatch of [
      (permit: ReturnType<typeof legacyPermit>) => ({ ...permit, permit_id: `${permit.permit_id}-wrong` }),
      (permit: ReturnType<typeof legacyPermit>) => ({ ...permit, issued_to_owner: "other-owner" }),
      (permit: ReturnType<typeof legacyPermit>) => ({ ...permit, issued_to_epoch: permit.issued_to_epoch + 1 }),
    ]) {
      const { ledger } = make(); const t = drainTuple(); let calls = 0; let drives = 0; let released = 0;
      const result = await runCanonicalEffect({ ...deps(ledger, t), release: async () => { released++; }, beforeConfirm: async permitId => { calls++; if (calls === 1) throw new Error("response lost"); return mismatch(legacyPermit(t, permitId)); }, drive: async () => { drives++; return undefined; } });
      expect(result.status).toBe("unknown_terminal"); expect(calls).toBe(2); expect(drives).toBe(0); expect(released).toBe(0);
    }
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
    const { ledger, storage } = make(); const t = tuple(); let claimed = false; let released = 0; let claims = 0; let calls = 0;
    const makeAttempt = () => ({ ...deps(ledger, t), claim: async () => !claimed && (claimed = true), release: async () => { claimed = false; released++; },
      admit: async () => { calls++; if (calls === 2) throw new Error("reservation unavailable"); return false; },
      beforeClaim: async () => { claims++; } });
    expect((await runCanonicalEffect(makeAttempt())).status).toBe("busy");
    expect((await runCanonicalEffect(makeAttempt())).status).toBe("unavailable");
    expect(claims).toBe(0); expect(released).toBe(0); expect(storage.map.size).toBe(0);
  });
});
