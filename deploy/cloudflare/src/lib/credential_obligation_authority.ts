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

  private static floorKey(tenant: string): string {
    return `credential-tenant-floor:${encodeURIComponent(tenant)}`;
  }

  private static generation(value: unknown): string {
    if (value === undefined) return "0";
    if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || value.length > 19 || BigInt(value) > I64_MAX) throw new Error("invalid lifecycle generation");
    return value;
  }

  private static validIdentity(identity: unknown): identity is CredentialIdentity {
    const value = identity as Partial<CredentialIdentity> | undefined;
    return typeof value?.jobId === "string" && value.jobId !== "" &&
      typeof value.tenant === "string" && value.tenant !== "" &&
      typeof value.patId === "string" && value.patId !== "" && (value.lifecycleGeneration === undefined || CredentialObligationAuthority.generation(value.lifecycleGeneration) === value.lifecycleGeneration);
  }

  private static identityMatches(a: CredentialIdentity, b: CredentialIdentity): boolean {
    return a.jobId === b.jobId && a.tenant === b.tenant && a.patId === b.patId && CredentialObligationAuthority.generation(a.lifecycleGeneration) === CredentialObligationAuthority.generation(b.lifecycleGeneration);
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
    CredentialObligationAuthority.generation(value.lifecycleGeneration);
    return value as CredentialIdentity & { status: string };
  }

  private static validateFloor(key: string, tenant: string, raw: unknown): string | undefined {
    if (raw === undefined) return undefined;
    const value = raw as { schema_version?: number; tenant?: string; revokedThrough?: unknown };
    if (value.schema_version !== 1 || value.tenant !== tenant || typeof value.revokedThrough !== "string" || key !== CredentialObligationAuthority.floorKey(tenant)) throw new Error("malformed credential tenant floor");
    return CredentialObligationAuthority.generation(value.revokedThrough);
  }

  async registerCredential(identity: CredentialIdentity): Promise<void> {
    if (!CredentialObligationAuthority.validIdentity(identity)) throw new Error("invalid credential identity");
    const fenced = await this.tx(async s => {
      const fence = await s.get(CredentialObligationAuthority.fenceKey(identity.jobId));
      CredentialObligationAuthority.validateFence(CredentialObligationAuthority.fenceKey(identity.jobId), identity.jobId, fence);
      const floor = CredentialObligationAuthority.validateFloor(CredentialObligationAuthority.floorKey(identity.tenant), identity.tenant, await s.get(CredentialObligationAuthority.floorKey(identity.tenant)));
      const key = CredentialObligationAuthority.credentialKey(identity);
      const existing = await s.get(key);
      if (existing !== undefined) {
        const record = CredentialObligationAuthority.validateCredential(key, existing);
        if (!CredentialObligationAuthority.identityMatches(record, identity)) throw new Error("credential identity generation conflict");
        const covered = floor !== undefined && BigInt(CredentialObligationAuthority.generation(record.lifecycleGeneration)) <= BigInt(floor);
        if (record.status === "registered" && (fence !== undefined || covered)) {
          await s.put(key, { schema_version: 1, ...identity, status: "revoke_requested" });
          return true;
        }
        if (record.status !== "registered") throw new Error("credential obligation already terminal or requested");
        return false;
      }
      const covered = floor !== undefined && BigInt(CredentialObligationAuthority.generation(identity.lifecycleGeneration)) <= BigInt(floor);
      await s.put(key, { schema_version: 1, ...identity, status: fence !== undefined || covered ? "revoke_requested" : "registered" });
      return fence !== undefined || covered;
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

  async closeTenantCredentials(tenant: string, throughGeneration: string): Promise<void> {
    if (typeof tenant !== "string" || tenant === "") throw new Error("invalid tenant identity");
    if (typeof throughGeneration !== "string") throw new Error("invalid lifecycle generation");
    const through = CredentialObligationAuthority.generation(throughGeneration);
    await this.tx(async s => {
      const floorKey = CredentialObligationAuthority.floorKey(tenant);
      const current = CredentialObligationAuthority.validateFloor(floorKey, tenant, await s.get(floorKey));
      if (current === undefined || BigInt(through) > BigInt(current)) await s.put(floorKey, { schema_version: 1, tenant, revokedThrough: through });
    });
  }

  async requestCredentialRevocation(identity: CredentialIdentity): Promise<void> {
    if (!CredentialObligationAuthority.validIdentity(identity)) throw new Error("invalid credential identity");
    await this.tx(async s => {
      const key = CredentialObligationAuthority.credentialKey(identity);
      const raw = await s.get(key);
      if (raw === undefined) throw new Error("credential obligation missing or divergent");
      const record = CredentialObligationAuthority.validateCredential(key, raw);
      if (!CredentialObligationAuthority.identityMatches(record, identity)) throw new Error("credential obligation missing or divergent");
      if (record.status === "registered") await s.put(key, { schema_version: 1, ...record, status: "revoke_requested" });
      else if (record.status !== "revoke_requested" && record.status !== "revoked") throw new Error("credential obligation status invalid");
    });
  }

  async revocationRequestedCredentials(cursor?: string): Promise<CredentialPage> {
    return this.pendingCredentials({ kind: "all" }, cursor, "revoke_requested");
  }

  async pendingCredentials(selection: CredentialSelection, cursor?: string, requestedStatus?: string): Promise<CredentialPage> {
    if (selection.kind === "tenant") CredentialObligationAuthority.validateFloor(CredentialObligationAuthority.floorKey(selection.tenant), selection.tenant, await this.storage.get(CredentialObligationAuthority.floorKey(selection.tenant)));
    const page = await this.storage.list({ prefix: "credential-obligation:", ...(cursor ? { startAfter: cursor } : {}), limit: 101 });
    const entries = [...page.entries()];
    const records: CredentialIdentity[] = [];
    for (const [key, raw] of entries) {
      const value = CredentialObligationAuthority.validateCredential(key, raw);
      const fenceKey = CredentialObligationAuthority.fenceKey(value.jobId);
      const fence = await this.storage.get(fenceKey);
      CredentialObligationAuthority.validateFence(fenceKey, value.jobId, fence);
      const floor = CredentialObligationAuthority.validateFloor(CredentialObligationAuthority.floorKey(value.tenant), value.tenant, await this.storage.get(CredentialObligationAuthority.floorKey(value.tenant)));
      const through = selection.kind === "tenant" && selection.throughGeneration !== undefined ? CredentialObligationAuthority.generation(selection.throughGeneration) : floor;
      const covered = through !== undefined && BigInt(CredentialObligationAuthority.generation(value.lifecycleGeneration)) <= BigInt(through);
      const matches = selection.kind === "all" || (selection.kind === "job" ? value.jobId === selection.jobId : value.tenant === selection.tenant && (selection.throughGeneration === undefined || BigInt(CredentialObligationAuthority.generation(value.lifecycleGeneration)) <= BigInt(CredentialObligationAuthority.generation(selection.throughGeneration))));
      const fencedRegistered = requestedStatus === "revoke_requested" && value.status === "registered" && (fence !== undefined || covered);
      if (matches && value.status !== "revoked" && ((!requestedStatus || value.status === requestedStatus) || fencedRegistered)) {
        records.push(value.lifecycleGeneration === undefined ? { jobId: value.jobId, tenant: value.tenant, patId: value.patId } : { jobId: value.jobId, tenant: value.tenant, patId: value.patId, lifecycleGeneration: value.lifecycleGeneration });
      }
    }
    const complete = entries.length < 101;
    return complete ? { records, complete } : { records, cursor: entries.at(-1)?.[0], complete: false };
  }

  async confirmCredentialRevoked(identity: CredentialIdentity): Promise<void> {
    if (!CredentialObligationAuthority.validIdentity(identity)) throw new Error("invalid credential identity");
    const key = CredentialObligationAuthority.credentialKey(identity);
    await this.tx(async s => {
      const raw = await s.get(key);
      if (raw === undefined) throw new Error("credential obligation missing or divergent");
      const record = CredentialObligationAuthority.validateCredential(key, raw);
      if (!CredentialObligationAuthority.identityMatches(record, identity)) throw new Error("credential obligation missing or divergent");
      if (record.status !== "revoke_requested" && record.status !== "revoked") throw new Error("credential obligation status invalid");
      if (record.status === "revoke_requested") await s.put(key, { schema_version: 1, ...record, status: "revoked", confirmed_at_ms: Date.now() });
    });
  }
}

const I64_MAX = 9_223_372_036_854_775_807n;
