import { describe, expect, it } from "vitest";
import { TenantSuspensionAuthority, TenantSuspensionIdentityConflict, type TenantSuspensionInput } from "../src/lib/tenant_suspension_authority";
import type { AuthorityStorage } from "../src/lib/authority_storage";

function storage() {
  const map = new Map<string, unknown>(); let queue = Promise.resolve(); let fail = false; let transactions = 0; let failAt = 0;
  const copy = <T>(value: T): T => value === undefined ? value : structuredClone(value);
  const base = {
    get: async <T>(key: string) => copy(map.get(key)) as T | undefined,
    put: async (key: string, value: unknown) => { map.set(key, copy(value)); },
    delete: async (key: string) => { map.delete(key); },
    list: async <T>(o: { prefix?: string; startAfter?: string; limit?: number }) => {
      const keys = [...map.keys()].filter(k => k.startsWith(o.prefix ?? "")).filter(k => !o.startAfter || k > o.startAfter).sort().slice(0, o.limit ?? Infinity);
      return new Map(keys.map(k => [k, copy(map.get(k)) as T]));
    },
  };
  const result = { ...base, map, failNext: () => { fail = true; }, failOnTransaction: (number: number) => { failAt = number; }, transactionCount: () => transactions, transaction: async <T>(fn: (s: typeof base) => Promise<T>) => {
    const run = queue.then(async () => {
      transactions++;
      const snapshot = new Map([...map].map(([k, v]) => [k, copy(v)]));
      const tx = { ...base, get: async <V>(k: string) => copy(snapshot.get(k)) as V | undefined, put: async (k: string, v: unknown) => { snapshot.set(k, copy(v)); }, delete: async (k: string) => { snapshot.delete(k); }, list: async <V>(o: { prefix?: string; startAfter?: string; limit?: number }) => new Map([...snapshot.keys()].filter(k => k.startsWith(o.prefix ?? "")).filter(k => !o.startAfter || k > o.startAfter).sort().slice(0, o.limit ?? Infinity).map(k => [k, copy(snapshot.get(k)) as V])) };
      const value = await fn(tx); if (fail || transactions === failAt) { fail = false; failAt = 0; throw new Error("injected rollback"); }
      map.clear(); for (const [k, v] of snapshot) map.set(k, copy(v)); return value;
    }); queue = run.then(() => undefined, () => undefined); return run;
  } };
  return result as unknown as AuthorityStorage & { failNext(): void; failOnTransaction(number: number): void; transactionCount(): number; map: Map<string, unknown> };
}
const input: TenantSuspensionInput = { event_id: "event-1", tenant_id: "tenant-1", lifecycle_generation: "3" };
const cursor = "credential-obligation:a:tenant:pat";

describe("durable tenant suspension event receipt", () => {
  it("persists before floor work and repairs floor after restart", async () => {
    const s = storage(); const authority = new TenantSuspensionAuthority(s);
    await s.put("credential-obligation:a:tenant-1:pat", { schema_version: 1, jobId: "a", tenant: "tenant-1", patId: "pat", status: "registered" });
    s.failNext(); await expect(authority.begin(input)).rejects.toThrow("injected rollback");
    expect(s.map.has("tenant-suspension-event:event-1")).toBe(false);
    await expect(new TenantSuspensionAuthority(s).begin(input)).resolves.toEqual({ complete: false });
    expect(s.map.get("credential-tenant-floor:tenant-1")).toMatchObject({ revokedThrough: "3" });
  });

  it("rejects event collisions without changing the floor", async () => {
    const s = storage(); const authority = new TenantSuspensionAuthority(s);
    await authority.begin(input);
    await expect(authority.begin({ ...input, lifecycle_generation: "4" })).rejects.toBeInstanceOf(TenantSuspensionIdentityConflict);
    expect(s.map.get("credential-tenant-floor:tenant-1")).toMatchObject({ revokedThrough: "3" });
  });

  it("uses atomic checkpoint CAS and monotonic completion", async () => {
    const s = storage(); const authority = new TenantSuspensionAuthority(s); await authority.begin(input);
    expect(await authority.checkpoint(input, undefined, cursor, false)).toBe(true);
    expect(await authority.checkpoint(input, undefined, "credential-obligation:b", true)).toBe(false);
    expect(await authority.checkpoint(input, cursor, undefined, true)).toBe(true);
    expect(await authority.checkpoint(input, cursor, undefined, true)).toBe(true);
    expect((await new TenantSuspensionAuthority(s).begin(input)).complete).toBe(true);
  });

  it("rolls back checkpoint writes and refuses corrupt receipt replay", async () => {
    const s = storage(); const authority = new TenantSuspensionAuthority(s); await authority.begin(input);
    s.failNext(); await expect(authority.checkpoint(input, undefined, cursor, false)).rejects.toThrow("injected rollback");
    expect(await authority.checkpoint(input, undefined, cursor, false)).toBe(true);
    await s.put("tenant-suspension-event:event-1", { schema_version: 1, event_id: "event-1", tenant_id: "tenant-1", lifecycle_generation: "bad", status: "requested" });
    await expect(new TenantSuspensionAuthority(s).begin(input)).rejects.toThrow();
  });

  it("retains a requested receipt when the following floor transaction crashes", async () => {
    const s = storage(); const authority = new TenantSuspensionAuthority(s);
    const next = { event_id: "event-2", tenant_id: "tenant-2", lifecycle_generation: "7" };
    s.failOnTransaction(s.transactionCount() + 2);
    await expect(authority.begin(next)).rejects.toThrow("injected rollback");
    expect(s.map.get("tenant-suspension-event:event-2")).toEqual({ schema_version: 1, ...next, status: "requested" });
    expect(s.map.has("credential-tenant-floor:tenant-2")).toBe(false);
    const restarted = new TenantSuspensionAuthority(s);
    await expect(restarted.begin(next)).resolves.toEqual({ complete: false });
    expect(s.map.get("tenant-suspension-event:event-2")).toEqual({ schema_version: 1, ...next, status: "requested" });
    expect(s.map.get("credential-tenant-floor:tenant-2")).toMatchObject({ revokedThrough: "7" });
  });
});
