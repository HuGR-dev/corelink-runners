import { describe, expect, it } from "vitest";
import { CredentialObligationAuthority } from "../src/lib/credential_obligation_authority";
import type { AuthorityStorage } from "../src/lib/authority_storage";

function storage(): AuthorityStorage {
  const map = new Map<string, unknown>();
  const base = {
    get: async <T>(key: string) => map.get(key) as T | undefined,
    put: async (key: string, value: unknown) => { map.set(key, value); },
    delete: async (key: string) => { map.delete(key); },
    list: async <T>(options: { prefix?: string; startAfter?: string; limit?: number }) => {
      const keys = [...map.keys()].filter(key => key.startsWith(options.prefix ?? "")).filter(key => !options.startAfter || key > options.startAfter).sort().slice(0, options.limit ?? Infinity);
      return new Map(keys.map(key => [key, map.get(key) as T]));
    },
  };
  return { ...base, transaction: async <T>(fn: (s: typeof base) => Promise<T>) => fn(base) } as unknown as AuthorityStorage;
}
const tenant = "tenant-a";
const identity = (jobId: string, patId: string, lifecycleGeneration?: string) => ({ jobId, tenant, patId, ...(lifecycleGeneration === undefined ? {} : { lifecycleGeneration }) });

describe("credential lifecycle generation floor", () => {
  it("closes old generations, survives restart, and leaves newer registrations live", async () => {
    const s = storage(); const first = new CredentialObligationAuthority(s);
    await first.registerCredential(identity("old", "pat-a", "1"));
    await first.registerCredential(identity("new", "pat-b", "2"));
    await first.closeTenantCredentials(tenant, "1");
    await expect(first.registerCredential(identity("late", "pat-c", "1"))).rejects.toThrow();
    const restarted = new CredentialObligationAuthority(s);
    expect((await restarted.revocationRequestedCredentials()).records).toEqual(expect.arrayContaining([identity("old", "pat-a", "1"), identity("late", "pat-c", "1")]));
    expect((await restarted.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "1" })).records).toEqual(expect.arrayContaining([identity("old", "pat-a", "1"), identity("late", "pat-c", "1")]));
    expect((await restarted.pendingCredentials({ kind: "tenant", tenant, throughGeneration: "2" })).records).toEqual(expect.arrayContaining([identity("old", "pat-a", "1"), identity("new", "pat-b", "2"), identity("late", "pat-c", "1")]));
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
});
