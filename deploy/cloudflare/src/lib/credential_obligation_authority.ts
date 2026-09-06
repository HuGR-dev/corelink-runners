import type { CredentialIdentity, CredentialPage, CredentialSelection } from "./credential_authority_contract";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

/** T8-W5: domain-owned storage logic. ContainmentDO retains RPC wiring. */
export class CredentialObligationAuthority {
  constructor(private readonly storage: AuthorityStorage) {}
  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> {
    return this.storage.transaction(fn);
  }

  private static credentialKey(identity: CredentialIdentity): string {
    return `credential-obligation:${encodeURIComponent(identity.jobId)}:${encodeURIComponent(identity.tenant)}:${encodeURIComponent(identity.patId)}`;
  }

  async registerCredential(identity: CredentialIdentity): Promise<void> {
    if (!identity.jobId || !identity.tenant || !identity.patId) throw new Error("invalid credential identity");
    await this.tx(async s => {
      const key = CredentialObligationAuthority.credentialKey(identity);
      const existing = await s.get(key) as (CredentialIdentity & { schema_version?: number; status?: string }) | undefined;
      if (existing !== undefined) {
        if (existing.schema_version !== 1 || existing.jobId !== identity.jobId || existing.tenant !== identity.tenant || existing.patId !== identity.patId || CredentialObligationAuthority.credentialKey(existing as CredentialIdentity) !== key || existing.status !== "registered" && existing.status !== "revoke_requested" && existing.status !== "revoked") throw new Error("credential obligation identity conflict");
        return;
      }
      await s.put(key, { schema_version: 1, ...identity, status: "registered" });
    });
  }

  async requestCredentialRevocation(identity: CredentialIdentity): Promise<void> {
    await this.tx(async s => {
      const key = CredentialObligationAuthority.credentialKey(identity);
      const raw = await s.get(key) as Partial<CredentialIdentity> & { schema_version?: number; status?: string } | undefined;
      if (!raw || raw.schema_version !== 1 || raw.jobId !== identity.jobId || raw.tenant !== identity.tenant || raw.patId !== identity.patId || key !== CredentialObligationAuthority.credentialKey(raw as CredentialIdentity)) throw new Error("credential obligation missing or divergent");
      if (raw.status === "registered") await s.put(key, { schema_version: 1, ...identity, status: "revoke_requested" });
      else if (raw.status !== "revoke_requested" && raw.status !== "revoked") throw new Error("credential obligation status invalid");
    });
  }

  async revocationRequestedCredentials(cursor?: string): Promise<CredentialPage> {
    return this.pendingCredentials({ kind: "all" }, cursor, "revoke_requested");
  }

  async pendingCredentials(selection: CredentialSelection, cursor?: string, requestedStatus?: string): Promise<CredentialPage> {
    const page = await this.storage.list({ prefix: "credential-obligation:", ...(cursor ? { startAfter: cursor } : {}), limit: 101 });
    const entries = [...page.entries()];
    const records: CredentialIdentity[] = [];
    for (const [key, raw] of entries) {
      const value = raw as Partial<CredentialIdentity> & { schema_version?: number; status?: string };
      if (value.schema_version !== 1 || typeof value.jobId !== "string" || value.jobId === "" || typeof value.tenant !== "string" || value.tenant === "" || typeof value.patId !== "string" || value.patId === "" || (value.status !== "registered" && value.status !== "revoke_requested" && value.status !== "revoked") || key !== CredentialObligationAuthority.credentialKey(value as CredentialIdentity)) throw new Error("malformed credential obligation");
      const matches = selection.kind === "all" || (selection.kind === "job" ? value.jobId === selection.jobId : value.tenant === selection.tenant);
      if (matches && value.status !== "revoked" && (!requestedStatus || value.status === requestedStatus)) records.push({ jobId: value.jobId, tenant: value.tenant, patId: value.patId });
      if (!key.startsWith("credential-obligation:")) throw new Error("credential obligation key divergent");
    }
    const complete = entries.length < 101;
    return complete ? { records, complete } : { records, cursor: entries.at(-1)?.[0], complete: false };
  }

  async confirmCredentialRevoked(identity: CredentialIdentity): Promise<void> {
    const key = CredentialObligationAuthority.credentialKey(identity);
    await this.tx(async s => {
      const raw = await s.get(key) as Partial<CredentialIdentity> & { schema_version?: number; status?: string } | undefined;
      if (!raw || raw.schema_version !== 1 || raw.jobId !== identity.jobId || raw.tenant !== identity.tenant || raw.patId !== identity.patId) throw new Error("credential obligation missing or divergent");
      if (raw.status !== "revoke_requested" && raw.status !== "revoked") throw new Error("credential obligation status invalid");
      if (raw.status === "revoke_requested") await s.put(key, { schema_version: 1, ...identity, status: "revoked", confirmed_at_ms: Date.now() });
    });
  }
}
