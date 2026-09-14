/** T8-W1 root-frozen boundary: one global rolling budget on ContainmentDO.
 * Call within its existing storage transaction. Never use KV or process memory
 * as authority. Commit the spend before returning admitted. Maximum five
 * spends in (nowMs - 60_000, nowMs]; absent/unreadable authority or failed write
 * refuses. A missing record in healthy authoritative storage is a fresh budget.
 * Invalid durable state, invalid time or backwards time refuses without reset.
 */
export interface AdmissionBudgetStorage {
  get<T>(key: string): Promise<T | undefined>;
  put<T>(key: string, value: T): Promise<void>;
}

export const ADMISSION_BUDGET_KEY = "admission-budget:v1";
export const ADMISSION_BUDGET_WINDOW_MS = 60_000;
export const ADMISSION_BUDGET_MAX_SPENDS = 5;

export interface AdmissionBudgetRecord {
  schema_version: 1;
  spends_ms: number[];
  last_observed_ms: number;
}

export type AdmissionBudgetVerdict =
  | { admitted: true }
  | { admitted: false; reason: "slot_failopen_budget_exhausted" | "slot_failopen_budget_unreadable" };

export async function spendAdmissionBudget(
  storage: AdmissionBudgetStorage,
  nowMs: number,
): Promise<AdmissionBudgetVerdict> {
  if (!Number.isSafeInteger(nowMs) || nowMs < 0) {
    return { admitted: false, reason: "slot_failopen_budget_unreadable" };
  }

  try {
    const stored = await storage.get<AdmissionBudgetRecord>(ADMISSION_BUDGET_KEY);
    if (stored !== undefined) {
      if (stored === null || typeof stored !== "object" || stored.schema_version !== 1
        || !Array.isArray(stored.spends_ms) || stored.spends_ms.length > ADMISSION_BUDGET_MAX_SPENDS
        || !Number.isSafeInteger(stored.last_observed_ms) || stored.last_observed_ms < 0
        || nowMs < stored.last_observed_ms) {
        return { admitted: false, reason: "slot_failopen_budget_unreadable" };
      }
      let previous = -Infinity;
      for (const timestamp of stored.spends_ms) {
        if (!Number.isSafeInteger(timestamp) || timestamp < 0 || timestamp > nowMs
          || timestamp > stored.last_observed_ms || timestamp < previous) {
          return { admitted: false, reason: "slot_failopen_budget_unreadable" };
        }
        previous = timestamp;
      }
    }

    const priorSpends = stored?.spends_ms ?? [];
    const cutoff = nowMs - ADMISSION_BUDGET_WINDOW_MS;
    const activeSpends = priorSpends.filter((timestamp) => timestamp > cutoff);
    if (activeSpends.length >= ADMISSION_BUDGET_MAX_SPENDS) {
      return { admitted: false, reason: "slot_failopen_budget_exhausted" };
    }

    await storage.put<AdmissionBudgetRecord>(ADMISSION_BUDGET_KEY, {
      schema_version: 1,
      spends_ms: [...activeSpends, nowMs],
      last_observed_ms: nowMs,
    });
    return { admitted: true };
  } catch {
    return { admitted: false, reason: "slot_failopen_budget_unreadable" };
  }
}
