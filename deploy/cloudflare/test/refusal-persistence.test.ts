import { describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { acquireConcurrencySlot, ConcurrencySlotsDO, type Env } from "../src/index";

function storage() {
  const data = new Map<string, unknown>();
  let failSlotsWrite = false;
  const impl = {
    get: async <T>(key: string) => data.get(key) as T | undefined,
    put: async (key: string, value: unknown) => {
      if (failSlotsWrite && key === "slots") throw new Error("slots write failed");
      data.set(key, value);
    },
    delete: async (key: string) => void data.delete(key),
    list: async () => new Map(),
    transaction: async <T>(fn: (tx: any) => Promise<T>) => {
      const before = new Map(data);
      try {
        return await fn(impl);
      } catch (error) {
        data.clear();
        for (const [key, value] of before) data.set(key, value);
        throw error;
      }
    },
  };
  return { impl: { ...impl, map: data }, failSlotsWrite: () => { failSlotsWrite = true; } };
}

describe("refusal persistence boundary", () => {
  it("does not spend bounded fail-open budget when refusal persistence fails", async () => {
    const s = storage();
    const doInst = new ConcurrencySlotsDO({ storage: s.impl } as never, {} as never);
    await doInst.acquire("tenant", "held", 1, 1, 10_000_000);
    const before = [...((s.impl as any).map?.get("slots") ?? [])];
    s.failSlotsWrite();
    const spendAdmissionBudget = vi.fn(async () => ({ admitted: true as const }));
    const env = {
      CONCURRENCY_SLOTS: { idFromName: () => "global", get: () => doInst },
      CONTAINMENT: { idFromName: () => "global", get: () => ({ spendAdmissionBudget }) },
    } as unknown as Env;
    const result = await acquireConcurrencySlot(env, "blocked", { tenant: "tenant", maxConcurrency: 1 } as never, "repo");
    expect(result).toEqual({ admitted: false, reason: "slot_refusal_unavailable" });
    expect(spendAdmissionBudget).not.toHaveBeenCalled();
    expect((s.impl as any).map?.get("slots")).toEqual(before);
    expect([...((s.impl as any).map?.keys() ?? [])].some((key) => String(key).startsWith("slot-refusal:v1:"))).toBe(false);
  });
});
