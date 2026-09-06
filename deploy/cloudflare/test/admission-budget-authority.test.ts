import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));
import {
  ADMISSION_BUDGET_KEY,
  spendAdmissionBudget,
  type AdmissionBudgetRecord,
  type AdmissionBudgetStorage,
} from "../src/lib/admission_budget";
import { acquireConcurrencySlot, ContainmentDO, type Env } from "../src/index";

class Storage implements AdmissionBudgetStorage {
  value: AdmissionBudgetRecord | undefined;
  failRead = false;
  failWrite = false;

  async get<T>(_key: string): Promise<T | undefined> {
    if (this.failRead) throw new Error("read failed");
    return this.value as T | undefined;
  }

  async put<T>(_key: string, value: T): Promise<void> {
    if (this.failWrite) throw new Error("write failed");
    this.value = value as AdmissionBudgetRecord;
  }

  transaction<T>(fn: (storage: Storage) => Promise<T>): Promise<T> {
    const run = this.queue.then(() => fn(this));
    this.queue = run.then(() => undefined, () => undefined);
    return run;
  }

  private queue: Promise<void> = Promise.resolve();
}

function containment(storage: Storage): ContainmentDO {
  return new ContainmentDO({ storage } as never, {} as Env);
}

function failingSlotEnv(storage?: Storage): Env {
  const authority = storage && containment(storage);
  return {
    CONCURRENCY_SLOTS: {
      idFromName: () => "global",
      get: () => ({ acquire: async () => { throw new Error("slot authority failed"); } }),
    } as never,
    CONTAINMENT: authority
      ? ({ idFromName: () => "global", get: () => authority } as never)
      : undefined,
  } as Env;
}

const mint = { tenant: "tenant", maxConcurrency: 1 } as never;

describe("global rolling admission budget", () => {
  it("commits five spends and refuses the sixth", async () => {
    const storage = new Storage();
    const verdicts = [];
    for (let i = 0; i < 6; i++) verdicts.push(await spendAdmissionBudget(storage, 1_000));
    expect(verdicts.slice(0, 5).every((verdict) => verdict.admitted)).toBe(true);
    expect(verdicts[5]).toEqual({ admitted: false, reason: "slot_failopen_budget_exhausted" });
    expect(storage.value?.spends_ms).toHaveLength(5);
  });

  it("uses a rolling boundary and survives a fresh helper call", async () => {
    const storage = new Storage();
    for (let i = 0; i < 5; i++) expect((await spendAdmissionBudget(storage, 1 + i)).admitted).toBe(true);
    expect((await spendAdmissionBudget(storage, 70_000)).admitted).toBe(true);
    expect(storage.value?.spends_ms).toEqual([70_000]);
  });

  it("refuses backwards, corrupt, and invalid clocks without resetting state", async () => {
    const storage = new Storage();
    expect((await spendAdmissionBudget(storage, 1_000)).admitted).toBe(true);
    const before = storage.value;
    expect(await spendAdmissionBudget(storage, 999)).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
    storage.value = { schema_version: 1, spends_ms: [2_000, Number.NaN], last_observed_ms: 2_000 };
    expect(await spendAdmissionBudget(storage, 2_000)).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
    storage.value = { schema_version: 1, spends_ms: [2_001], last_observed_ms: 2_000 };
    expect(await spendAdmissionBudget(storage, 2_001)).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
    expect(await spendAdmissionBudget(storage, 2_001.5)).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
    expect(before).not.toBeUndefined();
  });

  it.each(["read", "write"] as const)("refuses when authority %s fails", async (failure) => {
    const storage = new Storage();
    if (failure === "read") storage.failRead = true;
    else storage.failWrite = true;
    expect(await spendAdmissionBudget(storage, 1_000)).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
  });

  it("treats a missing durable record as a fresh budget", async () => {
    const storage = new Storage();
    expect(await storage.get(ADMISSION_BUDGET_KEY)).toBeUndefined();
    expect(await spendAdmissionBudget(storage, 1_000)).toEqual({ admitted: true });
  });

  it("bounds 100 concurrent production slot failures to five starts", async () => {
    const storage = new Storage();
    const env = failingSlotEnv(storage);
    const verdicts = await Promise.all(Array.from({ length: 100 }, (_, i) =>
      acquireConcurrencySlot(env, `job-${i}`, mint, "acme/api")));
    expect(verdicts.filter((verdict) => verdict.admitted)).toHaveLength(5);
    expect(verdicts.filter((verdict) => !verdict.admitted)).toHaveLength(95);
    expect(storage.value?.spends_ms).toHaveLength(5);
  });

  it("refuses production slot failures when the ContainmentDO binding is missing", async () => {
    const verdict = await acquireConcurrencySlot(failingSlotEnv(), "job", mint, "acme/api");
    expect(verdict).toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
  });

  it("refuses production slot failures when the authority cannot read or write", async () => {
    const readStorage = new Storage();
    readStorage.failRead = true;
    expect(await acquireConcurrencySlot(failingSlotEnv(readStorage), "read", mint, "acme/api"))
      .toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
    const writeStorage = new Storage();
    writeStorage.failWrite = true;
    expect(await acquireConcurrencySlot(failingSlotEnv(writeStorage), "write", mint, "acme/api"))
      .toEqual({ admitted: false, reason: "slot_failopen_budget_unreadable" });
  });

  it("keeps the budget across a ContainmentDO restart and rolls exactly at 60 seconds", async () => {
    const storage = new Storage();
    const first = containment(storage);
    vi.spyOn(Date, "now").mockReturnValue(0);
    for (let i = 0; i < 5; i++) expect((await first.spendAdmissionBudget()).admitted).toBe(true);
    expect((await first.spendAdmissionBudget()).admitted).toBe(false);
    const restarted = containment(storage);
    vi.spyOn(Date, "now").mockReturnValue(59_999);
    expect((await restarted.spendAdmissionBudget()).admitted).toBe(false);
    vi.spyOn(Date, "now").mockReturnValue(60_000);
    expect((await restarted.spendAdmissionBudget()).admitted).toBe(true);
    expect(storage.value?.spends_ms).toEqual([60_000]);
    vi.restoreAllMocks();
  });

  it("does not spend fail-open budget for a clean at-capacity refusal", async () => {
    const storage = new Storage();
    const env = {
      ...failingSlotEnv(storage),
      CONCURRENCY_SLOTS: {
        idFromName: () => "global",
        get: () => ({ acquire: async () => ({ admitted: false, reason: "over_fleet_cap" }) }),
      },
    } as Env;
    expect(await acquireConcurrencySlot(env, "at-cap", mint, "acme/api"))
      .toEqual({ admitted: false, reason: "over_fleet_cap" });
    expect(storage.value).toBeUndefined();
  });
});
