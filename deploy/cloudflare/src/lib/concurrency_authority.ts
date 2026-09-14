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
const HOLDERS_PREFIX = "slot-holders:v1:";
const MAX_HOLDERS = 64;

interface SlotHolders {
  key: string;
  holders: string[];
  legacy: boolean;
}

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

function validatePreparationId(preparationId: string | undefined): void {
  if (preparationId !== undefined && (typeof preparationId !== "string" || preparationId.length === 0 || preparationId.length > 256)) invalid("preparationId");
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

function holdersKey(jobId: string): string {
  return HOLDERS_PREFIX + encodeURIComponent(jobId);
}

function holdersValue(value: unknown): SlotHolders | null {
  if (value === undefined) return null;
  if (typeof value !== "object" || value === null) invalid("stored slot holders");
  const item = value as Record<string, unknown>;
  if (typeof item.key !== "string" || item.key.length === 0 || !Array.isArray(item.holders) || typeof item.legacy !== "boolean") invalid("stored slot holders");
  if (item.holders.some((holder) => typeof holder !== "string" || holder.length === 0 || holder.length > 256)) invalid("stored holder");
  const holders = item.holders as string[];
  if (holders.length > MAX_HOLDERS || new Set(holders).size !== holders.length) invalid("stored holder count");
  return { key: item.key, holders, legacy: item.legacy };
}

async function readSlots(tx: AuthorityTransaction): Promise<SlotRecord[]> {
  return slotsValue(await tx.get<unknown>(SLOTS_KEY));
}

export class ConcurrencyAuthority {
  constructor(private readonly storage: AuthorityStorage) {}

  async acquire(key: string, jobId: string, perKeyCap: number, fleetCap: number, nowMs: number, ttlMs: number, preparationId?: string): Promise<{ admitted: boolean; reason?: string }> {
    validateInputs(key, jobId, perKeyCap, fleetCap, nowMs, ttlMs);
    validatePreparationId(preparationId);
    let refusalDecision = false;
    try {
      return await this.storage.transaction(async (tx) => {
        const slots = await readSlots(tx);
        const existing = slots.find((slot) => slot.expiresMs > nowMs && slot.jobId === jobId);
        if (existing && existing.key !== key) return { admitted: false, reason: "job_id_key_conflict" };
        const existingHolders = existing ? holdersValue(await tx.get<unknown>(holdersKey(jobId))) : null;
        if (existing && existingHolders && existingHolders.key !== key) return { admitted: false, reason: "job_id_key_conflict" };
        const decision = decideSlotAcquire(slots, key, jobId, perKeyCap, fleetCap, nowMs, ttlMs);
        const expiredJobIds = new Set(slots.filter((slot) => slot.expiresMs <= nowMs).map((slot) => slot.jobId));
        let nextHolders: SlotHolders | undefined;
        if (decision.admitted) {
          const holders = existingHolders ?? { key, holders: [], legacy: existing !== undefined || preparationId === undefined };
          holders.holders = [...holders.holders];
          if (preparationId === undefined) holders.legacy = true;
          if (preparationId !== undefined) {
            if (!holders.holders.includes(preparationId)) {
              if (holders.holders.length >= MAX_HOLDERS) return { admitted: false, reason: "preparation_holder_limit" };
              holders.holders.push(preparationId);
            }
          }
          nextHolders = holders;
        }
        refusalDecision = !decision.admitted;
        if (!decision.admitted) {
          const keyName = refusalKey(jobId);
          const prior = refusalValue(await tx.get<unknown>(keyName), jobId);
          if (!prior) await tx.put(keyName, { job_id: jobId, state: "refused_at_ceiling", reason: decision.reason ?? "refused_at_ceiling", recorded_at_ms: nowMs });
        }
        for (const expiredJobId of expiredJobIds) await tx.delete(holdersKey(expiredJobId));
        if (decision.admitted) {
          await tx.put(SLOTS_KEY, decision.slots);
          await tx.put(holdersKey(jobId), nextHolders!);
        } else {
          await tx.put(SLOTS_KEY, decision.slots);
        }
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
      const expiredJobIds = new Set(slots.filter((slot) => slot.expiresMs <= nowMs).map((slot) => slot.jobId));
      await tx.put(SLOTS_KEY, slots.filter((slot) => slot.expiresMs > nowMs && slot.jobId !== jobId));
      for (const expiredJobId of expiredJobIds) await tx.delete(holdersKey(expiredJobId));
      await tx.delete(holdersKey(jobId));
    });
  }

  async releasePreparation(jobId: string, preparationId: string): Promise<boolean> {
    if (typeof jobId !== "string" || jobId.length === 0) invalid("jobId");
    if (typeof preparationId !== "string" || preparationId.length === 0) invalid("preparationId");
    validatePreparationId(preparationId);
    return this.storage.transaction(async (tx) => {
      const holders = holdersValue(await tx.get<unknown>(holdersKey(jobId)));
      if (!holders || !holders.holders.includes(preparationId)) return false;
      const slots = await readSlots(tx);
      const slot = slots.find((candidate) => candidate.jobId === jobId);
      if (!slot) return false;
      if (slot.key !== holders.key) invalid("slot holder key mismatch");
      const remaining = holders.holders.filter((holder) => holder !== preparationId);
      if (remaining.length > 0 || holders.legacy) {
        await tx.put(holdersKey(jobId), { ...holders, holders: remaining });
        return true;
      }
      await tx.put(SLOTS_KEY, slots.filter((slot) => slot.jobId !== jobId));
      await tx.delete(holdersKey(jobId));
      return true;
    });
  }

  async prune(nowMs: number): Promise<number> {
    numberInput(nowMs, "nowMs");
    return this.storage.transaction(async (tx) => {
      const slots = await readSlots(tx);
      const live = slots.filter((slot) => slot.expiresMs > nowMs);
      await tx.put(SLOTS_KEY, live);
      for (const slot of slots) if (slot.expiresMs <= nowMs) await tx.delete(holdersKey(slot.jobId));
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
