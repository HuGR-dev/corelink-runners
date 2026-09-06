import { describe, expect, it } from "vitest";
import {
  ADMISSION_BUDGET_KEY,
  spendAdmissionBudget,
  type AdmissionBudgetRecord,
  type AdmissionBudgetStorage,
} from "../src/lib/admission_budget";

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
}

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
});
