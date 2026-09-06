import { describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { acquireConcurrencySlot, type Env } from "../src/index";
import { ConcurrencyAuthority } from "../src/lib/concurrency_authority";

function storage() {
  const data = new Map<string, unknown>();
  let failRefusalWrite = false;
  const impl = {
    get: async <T>(key: string) => data.get(key) as T | undefined,
    put: async (key: string, value: unknown) => {
      if (failRefusalWrite && key.startsWith("slot-refusal:v1:")) throw new Error("refusal write failed");
      data.set(key, value);
    },
    delete: async (key: string) => void data.delete(key),
    list: async () => new Map(),
    transaction: async <T>(fn: (tx: any) => Promise<T>) => fn(impl),
  };
  return { impl, authority: new ConcurrencyAuthority(impl as any), failRefusalWrite: () => { failRefusalWrite = true; } };
}

describe("refusal persistence boundary", () => {
  it("does not spend bounded fail-open budget when refusal persistence fails", async () => {
    const s = storage();
    const now = Date.now();
    await s.authority.acquire("tenant", "held", 1, 1, now, 10_000_000);
    s.failRefusalWrite();
    const spendAdmissionBudget = vi.fn(async () => ({ admitted: true as const }));
    const env = {
      CONCURRENCY_SLOTS: {
        idFromName: () => "global",
        get: () => ({ acquire: (...args: any[]) => s.authority.acquire(args[0], args[1], args[2], args[3], Date.now(), args[4]) }),
      },
      CONTAINMENT: { idFromName: () => "global", get: () => ({ spendAdmissionBudget }) },
    } as unknown as Env;
    const result = await acquireConcurrencySlot(env, "blocked", { tenant: "tenant", maxConcurrency: 1 } as never, "repo");
    expect(result).toEqual({ admitted: false, reason: "slot_refusal_unavailable" });
    expect(spendAdmissionBudget).not.toHaveBeenCalled();
  });
});
