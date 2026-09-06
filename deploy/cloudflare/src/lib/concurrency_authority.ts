import { decideSlotAcquire, type SlotRecord } from "../lib";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

export interface SlotRefusal {
  job_id: string;
  state: "refused_at_ceiling";
  reason: string;
  recorded_at_ms: number;
}

const SLOTS_KEY = "slots";
const REFUSAL_PREFIX = "slot-refusal:v1:";

function invalid(message: string): never {
  throw new Error(`concurrency authority invalid input: ${message}`);
}

function numberInput(value: number, name: string): void {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) invalid(name);
}

function validateInputs(key: string, jobId: string, perKeyCap: number, fleetCap: number, nowMs: number, ttlMs: number): void {
  if (typeof key !== "string" || key.length === 0) invalid("key");
  if (typeof jobId !== "string" || jobId.length === 0) invalid("jobId");
  for (const [value, name] of [[perKeyCap, "perKeyCap"], [fleetCap, "fleetCap"], [nowMs, "nowMs"], [ttlMs, "ttlMs"]] as const) numberInput(value, name);
  if (!Number.isSafeInteger(nowMs + ttlMs)) invalid("nowMs + ttlMs");
}

function slotsValue(value: unknown): SlotRecord[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) invalid("stored slots");
  const slots = value.map((raw) => {
    if (typeof raw !== "object" || raw === null) invalid("stored slot record");
    const item = raw as Record<string, unknown>;
    if (typeof item.key !== "string" || item.key.length === 0) invalid("stored slot key");
    if (typeof item.jobId !== "string" || item.jobId.length === 0) invalid("stored slot jobId");
    if (typeof item.expiresMs !== "number" || !Number.isSafeInteger(item.expiresMs) || item.expiresMs < 0) invalid("stored slot expiry");
    return { key: item.key, jobId: item.jobId, expiresMs: item.expiresMs } satisfies SlotRecord;
  });
  const jobs = new Set<string>();
  for (const slot of slots) if (jobs.has(slot.jobId)) invalid("duplicate stored slot jobId"); else jobs.add(slot.jobId);
  return slots;
}

function refusalValue(value: unknown, jobId: string): SlotRefusal | null {
  if (value === undefined) return null;
  if (typeof value !== "object" || value === null) invalid("stored refusal");
  const item = value as Record<string, unknown>;
  if (item.job_id !== jobId || item.state !== "refused_at_ceiling" || typeof item.reason !== "string" || typeof item.recorded_at_ms !== "number" || !Number.isSafeInteger(item.recorded_at_ms) || item.recorded_at_ms < 0) invalid("stored refusal");
  return { job_id: item.job_id, state: "refused_at_ceiling", reason: item.reason, recorded_at_ms: item.recorded_at_ms };
}

function refusalKey(jobId: string): string {
  return REFUSAL_PREFIX + encodeURIComponent(jobId);
}

async function readSlots(tx: AuthorityTransaction): Promise<SlotRecord[]> {
  return slotsValue(await tx.get<unknown>(SLOTS_KEY));
}

export class ConcurrencyAuthority {
  constructor(private readonly storage: AuthorityStorage) {}

  async acquire(key: string, jobId: string, perKeyCap: number, fleetCap: number, nowMs: number, ttlMs: number): Promise<{ admitted: boolean; reason?: string }> {
    validateInputs(key, jobId, perKeyCap, fleetCap, nowMs, ttlMs);
    let refusalDecision = false;
    try {
      return await this.storage.transaction(async (tx) => {
        const slots = await readSlots(tx);
        const existing = slots.find((slot) => slot.expiresMs > nowMs && slot.jobId === jobId);
        if (existing && existing.key !== key) return { admitted: false, reason: "job_id_key_conflict" };
        const decision = decideSlotAcquire(slots, key, jobId, perKeyCap, fleetCap, nowMs, ttlMs);
        refusalDecision = !decision.admitted;
        if (!decision.admitted) {
          const keyName = refusalKey(jobId);
          const prior = refusalValue(await tx.get<unknown>(keyName), jobId);
          if (!prior) await tx.put(keyName, { job_id: jobId, state: "refused_at_ceiling", reason: decision.reason ?? "refused_at_ceiling", recorded_at_ms: nowMs });
        }
        await tx.put(SLOTS_KEY, decision.slots);
        return { admitted: decision.admitted, reason: decision.reason };
      });
    } catch (error) {
      if (refusalDecision) return { admitted: false, reason: "slot_refusal_unavailable" };
      throw error;
    }
  }

  async release(jobId: string, nowMs: number): Promise<void> {
    if (typeof jobId !== "string" || jobId.length === 0) invalid("jobId");
    numberInput(nowMs, "nowMs");
    await this.storage.transaction(async (tx) => {
      const slots = await readSlots(tx);
      await tx.put(SLOTS_KEY, slots.filter((slot) => slot.expiresMs > nowMs && slot.jobId !== jobId));
    });
  }

  async prune(nowMs: number): Promise<number> {
    numberInput(nowMs, "nowMs");
    return this.storage.transaction(async (tx) => {
      const slots = await readSlots(tx);
      const live = slots.filter((slot) => slot.expiresMs > nowMs);
      await tx.put(SLOTS_KEY, live);
      return slots.length - live.length;
    });
  }

  async renew(jobId: string, nowMs: number, ttlMs: number): Promise<boolean> {
    if (typeof jobId !== "string" || jobId.length === 0) invalid("jobId");
    numberInput(nowMs, "nowMs");
    numberInput(ttlMs, "ttlMs");
    if (!Number.isSafeInteger(nowMs + ttlMs)) invalid("nowMs + ttlMs");
    return this.storage.transaction(async (tx) => {
      const slots = await readSlots(tx);
      const index = slots.findIndex((slot) => slot.jobId === jobId);
      if (index < 0) return false;
      const renewed = slots.slice();
      renewed[index] = { ...renewed[index], expiresMs: Math.max(renewed[index].expiresMs, nowMs + ttlMs) };
      await tx.put(SLOTS_KEY, renewed);
      return true;
    });
  }

  async getRefusal(jobId: string): Promise<SlotRefusal | null> {
    if (typeof jobId !== "string" || jobId.length === 0) invalid("jobId");
    return refusalValue(await this.storage.get<unknown>(refusalKey(jobId)), jobId);
  }
}
