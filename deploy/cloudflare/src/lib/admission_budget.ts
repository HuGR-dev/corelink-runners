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

export type AdmissionBudgetVerdict =
  | { admitted: true }
  | { admitted: false; reason: "slot_failopen_budget_exhausted" | "slot_failopen_budget_unreadable" };

export async function spendAdmissionBudget(
  _storage: AdmissionBudgetStorage,
  _nowMs: number,
): Promise<AdmissionBudgetVerdict> {
  throw new Error("T8-W1 implementation pending");
}
