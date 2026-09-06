import { describe, expect, it } from "vitest";
import { CredentialObligationAuthority } from "../src/lib/credential_obligation_authority";
import type { AuthorityStorage } from "../src/lib/authority_storage";

function storage(): AuthorityStorage {
  const map = new Map<string, unknown>();
  let queue = Promise.resolve();
  let failNextTransaction = false;
  const clone = <T>(value: T): T => value === undefined ? value : structuredClone(value);
  const base = {
    get: async <T>(key: string) => clone(map.get(key)) as T | undefined,
    put: async (key: string, value: unknown) => { map.set(key, clone(value)); },
    delete: async (key: string) => { map.delete(key); },
    list: async <T>(options: { prefix?: string; startAfter?: string; limit?: number }) => {
      const keys = [...map.keys()].filter(key => key.startsWith(options.prefix ?? "")).filter(key => !options.startAfter || key > options.startAfter).sort().slice(0, options.limit ?? Infinity);
      return new Map(keys.map(key => [key, clone(map.get(key)) as T]));
    },
  };
  return { ...base, failNext: () => { failNextTransaction = true; }, transaction: async <T>(fn: (s: typeof base) => Promise<T>) => {
    const run = queue.then(async () => {
      const snapshot = new Map([...map].map(([key, value]) => [key, clone(value)]));
      const tx = { ...base, get: async <V>(key: string) => clone(snapshot.get(key)) as V | undefined, put: async (key: string, value: unknown) => { snapshot.set(key, clone(value)); }, delete: async (key: string) => { snapshot.delete(key); }, list: async <V>(options: { prefix?: string; startAfter?: string; limit?: number }) => {
        const keys = [...snapshot.keys()].filter(key => key.startsWith(options.prefix ?? "")).filter(key => !options.startAfter || key > options.startAfter).sort().slice(0, options.limit ?? Infinity);
        return new Map(keys.map(key => [key, clone(snapshot.get(key)) as V]));
      } };
      const result = await fn(tx);
      if (failNextTransaction) { failNextTransaction = false; throw new Error("injected transaction failure"); }
      map.clear(); for (const [key, value] of snapshot) map.set(key, clone(value));
      return result;
    });
    queue = run.then(() => undefined, () => undefined);
    return run;
  } } as unknown as AuthorityStorage & { failNext(): void };
}
const tenant = "tenant-a";
const identity = (jobId: string, patId: string, lifecycleGeneration?: string, identityTenant = tenant) => ({ jobId, tenant: identityTenant, patId, ...(lifecycleGeneration === undefined ? {} : { lifecycleGeneration }) });

describe("credential lifecycle generation floor", () => {
  it("closes old generations, survives restart, and leaves newer registrations live", async () => {
    const s = storage(); const first = new CredentialObligationAuthority(s);
    await first.registerCredential(identity("old", "pat-a", "1"));
    await first.registerCredential(identity("new", "pat-b", "2"));
    await first.closeTenantCredentials(tenant, "1");
    await expect(first.registerCredential(identity("late", "pat-c", "1"))).rejects.toThrow();
    const restarted = new CredentialObligationAuthority(s);
    expect((await restarted.revocationRequestedCredentials()).records).toEqual([identity("late", "pat-c", "1"), identity("old", "pat-a", "1")]);
    expect((await restarted.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "1" })).records).toEqual([identity("late", "pat-c", "1"), identity("old", "pat-a", "1")]);
    expect((await restarted.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "2" })).records).toEqual([identity("late", "pat-c", "1"), identity("new", "pat-b", "2"), identity("old", "pat-a", "1")]);
  });

  it("refuses exact PAT generation rebinding and preserves legacy zero shape", async () => {
    const s = storage(); const authority = new CredentialObligationAuthority(s);
    await authority.registerCredential(identity("job", "pat", "2"));
    await expect(authority.registerCredential(identity("job", "pat", "3"))).rejects.toThrow();
    await authority.registerCredential(identity("legacy", "pat-legacy"));
    expect((await authority.pendingCredentials({ kind: "all" })).records).toContainEqual(identity("legacy", "pat-legacy"));
    await expect(authority.requestCredentialRevocation(identity("job", "pat", "1"))).rejects.toThrow();
  });

  it("honors an explicit tenant generation bound and keeps a higher floor monotonic", async () => {
    const s = storage(); const authority = new CredentialObligationAuthority(s);
    await authority.registerCredential(identity("old", "pat-old", "1"));
    await authority.registerCredential(identity("new", "pat-new", "2"));
    await authority.closeTenantCredentials(tenant, "2");
    await authority.closeTenantCredentials(tenant, "1");
    expect((await authority.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "1" })).records).toEqual([identity("old", "pat-old", "1")]);
    expect((await authority.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "2" })).records).toHaveLength(2);
  });

  it("fails closed for a floor missing its generation", async () => {
    const s = storage(); await s.put("credential-tenant-floor:tenant-a", { schema_version: 1, tenant });
    const authority = new CredentialObligationAuthority(s);
    await expect(authority.pendingCredentials({ kind: "tenant", tenant })).rejects.toThrow("malformed credential tenant floor");
  });

  it("pages more than 202 mixed records without omissions or duplicates after restart", async () => {
    const s = storage(); const authority = new CredentialObligationAuthority(s);
    for (let i = 0; i < 210; i++) await authority.registerCredential(identity(`job-${String(i).padStart(3, "0")}`, `pat-${i}`, i % 4 === 1 ? "1" : "2", i % 2 ? tenant : "tenant-b"));
    await authority.closeTenantCredentials(tenant, "1");
    const found: string[] = []; let cursor: string | undefined;
    do {
      const page = await new CredentialObligationAuthority(s).pendingCredentials({ kind: "tenant", tenant, throughGeneration: "1" }, cursor);
      found.push(...page.records.map(record => record.patId)); cursor = page.cursor;
    } while (cursor);
    expect(found).toHaveLength(53); expect(new Set(found).size).toBe(found.length);
    expect(found).toEqual(Array.from({ length: 53 }, (_, i) => `pat-${i * 4 + 1}`));
  });

  it("serializes floor/register orders and rolls back injected transaction failures", async () => {
    const s = storage() as AuthorityStorage & { failNext(): void }; const authority = new CredentialObligationAuthority(s);
    s.failNext(); await expect(authority.closeTenantCredentials(tenant, "1")).rejects.toThrow("injected transaction failure");
    await authority.registerCredential(identity("new", "pat-new", "2"));
    await expect(authority.registerCredential(identity("late", "pat-old", "1"))).resolves.toBeUndefined();
    await authority.closeTenantCredentials(tenant, "1");
    await expect(authority.registerCredential(identity("late-2", "pat-old-2", "1"))).rejects.toThrow();
    expect((await authority.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "2" })).records).toEqual([identity("late-2", "pat-old-2", "1"), identity("late", "pat-old", "1"), identity("new", "pat-new", "2")]);
  });
});
