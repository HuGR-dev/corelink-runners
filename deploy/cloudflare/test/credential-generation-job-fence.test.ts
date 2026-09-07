import { describe, expect, it } from "vitest";
import { CredentialObligationAuthority } from "../src/lib/credential_obligation_authority";
import type { AuthorityStorage } from "../src/lib/authority_storage";

function storage() {
  const map = new Map<string, unknown>(); let tail = Promise.resolve();
  const copy = <T>(v: T): T => v === undefined ? v : structuredClone(v);
  const base = { get: async <T>(k: string) => copy(map.get(k)) as T | undefined, put: async (k: string, v: unknown) => { map.set(k, copy(v)); }, delete: async (k: string) => { map.delete(k); }, list: async <T>(o: { prefix?: string; startAfter?: string; limit?: number }) => new Map([...map].filter(([k]) => k.startsWith(o.prefix ?? "") && (!o.startAfter || k > o.startAfter)).sort().slice(0, o.limit ?? Infinity).map(([k, v]) => [k, copy(v) as T])) };
  return { map, storage: { ...base, transaction: async <T>(fn: (s: typeof base) => Promise<T>) => { const wait = tail; let release!: () => void; tail = new Promise<void>(r => { release = r; }); await wait; try { const snapshot = new Map([...map].map(([k, v]) => [k, copy(v)])); const tx = { ...base, get: async <V>(k: string) => copy(snapshot.get(k)) as V | undefined, put: async (k: string, v: unknown) => { snapshot.set(k, copy(v)); }, delete: async (k: string) => { snapshot.delete(k); }, list: async <V>(o: { prefix?: string; startAfter?: string; limit?: number }) => new Map([...snapshot].filter(([key]) => key.startsWith(o.prefix ?? "") && (!o.startAfter || key > o.startAfter)).sort().slice(0, o.limit ?? Infinity).map(([key, v]) => [key, copy(v) as V])) }; const result = await fn(tx); map.clear(); for (const [k, v] of snapshot) map.set(k, copy(v)); return result; } finally { release(); } } } as unknown as AuthorityStorage };
}
const identity = (jobId: string, patId: string, generation?: string, tenant = "tenant-a") => ({ jobId, tenant, patId, ...(generation === undefined ? {} : { lifecycleGeneration: generation }) });

describe("credential generation job fence acceptance", () => {
  it("closes every generation for one job, preserves the fence across restart, and leaves another job unaffected", async () => {
    const f = storage(); const authority = new CredentialObligationAuthority(f.storage);
    await authority.registerCredential(identity("job", "legacy")); await authority.registerCredential(identity("job", "gen1", "1")); await authority.registerCredential(identity("job", "gen2", "2")); await authority.registerCredential(identity("other", "other2", "2"));
    expect(await authority.closeJobCredentials("job")).toEqual({ known: true });
    const restarted = new CredentialObligationAuthority(f.storage);
    expect((await restarted.revocationRequestedCredentials()).records).toEqual([identity("job", "gen1", "1"), identity("job", "gen2", "2"), identity("job", "legacy")]);
    await expect(restarted.registerCredential(identity("job", "late", "3"))).rejects.toThrow();
    expect((await restarted.pendingCredentials({ kind: "job", jobId: "other" })).records).toEqual([identity("other", "other2", "2")]);
  });

  it("round-trips an exact generation and PAT through request and confirmation without rebinding", async () => {
    const f = storage(); const authority = new CredentialObligationAuthority(f.storage); const record = identity("job", "pat", "2");
    await authority.registerCredential(record); await authority.requestCredentialRevocation(record); const restarted = new CredentialObligationAuthority(f.storage);
    await restarted.confirmCredentialRevoked(record);
    await expect(restarted.confirmCredentialRevoked(identity("job", "pat", "3"))).rejects.toThrow();
    await expect(restarted.requestCredentialRevocation(identity("job", "pat", "1"))).rejects.toThrow();
    expect((await restarted.pendingCredentials({ kind: "all" })).records).toEqual([]);
  });

  it("rejects malformed identities, generations, and stored records without mutation or completion", async () => {
    const f = storage(); const authority = new CredentialObligationAuthority(f.storage);
    for (const bad of [identity("", "pat"), identity("job", "pat", "01"), identity("job", "pat", "9223372036854775808")]) await expect(authority.registerCredential(bad)).rejects.toThrow();
    const key = "credential-obligation:job:tenant-a:bad"; await f.storage.put(key, { schema_version: 1, jobId: "job", tenant: "tenant-a", patId: "bad", lifecycleGeneration: "01", status: "registered" });
    await expect(new CredentialObligationAuthority(f.storage).pendingCredentials({ kind: "all" })).rejects.toThrow();
  });
});
