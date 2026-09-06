import type { StashedCred, StashRecord } from "../lib.js";

export const CRED_STASH_CLOSED_KEY = "closedUntilMs";

/** Call under the stash DO's concurrency gate. An absolute grant cannot be extended by transport delay. */
export async function stashCredential(storage: any, ticket: string, cred: StashedCred, ttlMs: number, absoluteExpiresAtMs?: number): Promise<string> {
  const [existing, closed] = await Promise.all([storage.get("rec"), storage.get(CRED_STASH_CLOSED_KEY)]) as [StashRecord | undefined, number | undefined];
  const now = Date.now();
  if (absoluteExpiresAtMs !== undefined && (!Number.isSafeInteger(absoluteExpiresAtMs) || absoluteExpiresAtMs <= now)) {
    throw new Error("credential deadline must be finite and in the future");
  }
  if (closed !== undefined) throw new Error("credential lease is closed");
  if (existing && now <= existing.expiresMs) {
    if (absoluteExpiresAtMs !== undefined && absoluteExpiresAtMs < existing.expiresMs) {
      await storage.put("rec", { ...existing, expiresMs: absoluteExpiresAtMs });
      await storage.setAlarm(absoluteExpiresAtMs);
    }
    return existing.ticket;
  }
  const expiresMs = Math.min(now + ttlMs, absoluteExpiresAtMs ?? Infinity);
  await storage.put("rec", { ticket, cred, expiresMs });
  await storage.setAlarm(expiresMs);
  return ticket;
}

/** A completed DevEnv keeps a tombstone until PAT expiry so a timed-out, late stash cannot reopen it. */
export async function wipeCredential(storage: any, absoluteExpiresAtMs?: number): Promise<void> {
  if (absoluteExpiresAtMs === undefined) {
    await Promise.all([storage.deleteAll(), storage.deleteAlarm()]);
    return;
  }
  if (!Number.isSafeInteger(absoluteExpiresAtMs) || absoluteExpiresAtMs < 0) throw new Error("invalid credential deadline");
  const existing = await storage.get(CRED_STASH_CLOSED_KEY) as number | undefined;
  const until = Math.max(existing ?? 0, absoluteExpiresAtMs);
  // Persist denial first. Even if deletion fails, redeem must not return a PAT.
  await storage.put(CRED_STASH_CLOSED_KEY, until);
  await storage.delete("rec");
  await storage.setAlarm(Math.max(Date.now() + 1, until));
}


/** An already queued, older alarm cannot remove a newer closure before its deadline. */
export async function expireCredential(storage: any): Promise<void> {
  const [closedUntil, record] = await Promise.all([storage.get(CRED_STASH_CLOSED_KEY), storage.get("rec")]) as [number | undefined, StashRecord | undefined];
  const currentDeadline = Math.max(closedUntil ?? 0, record?.expiresMs ?? 0);
  if (Date.now() < currentDeadline) {
    await storage.setAlarm(currentDeadline);
    return;
  }
  await storage.deleteAll();
}
