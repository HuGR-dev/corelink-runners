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

  private static fenceKey(jobId: string): string {
    return `credential-job-fence:${encodeURIComponent(jobId)}`;
  }

  private static validIdentity(identity: unknown): identity is CredentialIdentity {
    const value = identity as Partial<CredentialIdentity> | undefined;
    return typeof value?.jobId === "string" && value.jobId !== "" &&
      typeof value.tenant === "string" && value.tenant !== "" &&
      typeof value.patId === "string" && value.patId !== "";
  }

  private static validateFence(key: string, jobId: string, raw: unknown): void {
    const fence = raw as { schema_version?: number; jobId?: string; status?: string } | undefined;
    if (fence === undefined) return;
    if (fence.schema_version !== 1 || fence.jobId !== jobId || fence.status !== "closed" || key !== CredentialObligationAuthority.fenceKey(jobId)) {
      throw new Error("malformed credential job fence");
    }
  }

  private static validateCredential(key: string, raw: unknown): CredentialIdentity & { status: string } {
    const value = raw as Partial<CredentialIdentity> & { schema_version?: number; status?: string };
    if (value.schema_version !== 1 || !CredentialObligationAuthority.validIdentity(value) ||
      ((value as { status?: string }).status !== "registered" && (value as { status?: string }).status !== "revoke_requested" && (value as { status?: string }).status !== "revoked") ||
      key !== CredentialObligationAuthority.credentialKey(value)) throw new Error("malformed credential obligation");
    return value as CredentialIdentity & { status: string };
  }

  async registerCredential(identity: CredentialIdentity): Promise<void> {
    if (!CredentialObligationAuthority.validIdentity(identity)) throw new Error("invalid credential identity");
    const fenced = await this.tx(async s => {
      const fence = await s.get(CredentialObligationAuthority.fenceKey(identity.jobId));
      CredentialObligationAuthority.validateFence(CredentialObligationAuthority.fenceKey(identity.jobId), identity.jobId, fence);
      const key = CredentialObligationAuthority.credentialKey(identity);
      const existing = await s.get(key);
      if (existing !== undefined) {
        const record = CredentialObligationAuthority.validateCredential(key, existing);
        if (record.status === "registered" && fence !== undefined) {
          await s.put(key, { schema_version: 1, ...identity, status: "revoke_requested" });
          return true;
        }
        if (record.status !== "registered") throw new Error("credential obligation already terminal or requested");
        return false;
      }
      await s.put(key, { schema_version: 1, ...identity, status: fence === undefined ? "registered" : "revoke_requested" });
      return fence !== undefined;
    });
    if (fenced) throw new Error("credential job is closed");
  }

  async closeJobCredentials(jobId: string): Promise<{ known: boolean }> {
    if (typeof jobId !== "string" || jobId === "") throw new Error("invalid job identity");
    return this.tx(async s => {
      const fenceKey = CredentialObligationAuthority.fenceKey(jobId);
      const existingFence = await s.get(fenceKey);
      CredentialObligationAuthority.validateFence(fenceKey, jobId, existingFence);
      const prefix = `credential-obligation:${encodeURIComponent(jobId)}:`;
      const page = await s.list({ prefix, limit: 101 });
      let known = false;
      for (const [key, raw] of page.entries()) {
        const record = CredentialObligationAuthority.validateCredential(key, raw);
        if (record.jobId !== jobId) throw new Error("credential obligation job key divergent");
        known = true;
      }
      if (existingFence === undefined) {
        await s.put(fenceKey, { schema_version: 1, jobId, status: "closed", closed_at_ms: Date.now() });
      }
      return { known };
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
      const value = CredentialObligationAuthority.validateCredential(key, raw);
      const fenceKey = CredentialObligationAuthority.fenceKey(value.jobId);
      const fence = await this.storage.get(fenceKey);
      CredentialObligationAuthority.validateFence(fenceKey, value.jobId, fence);
      const matches = selection.kind === "all" || (selection.kind === "job" ? value.jobId === selection.jobId : value.tenant === selection.tenant);
      const fencedRegistered = requestedStatus === "revoke_requested" && value.status === "registered" && fence !== undefined;
      if (matches && value.status !== "revoked" && ((!requestedStatus || value.status === requestedStatus) || fencedRegistered)) records.push({ jobId: value.jobId, tenant: value.tenant, patId: value.patId });
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
