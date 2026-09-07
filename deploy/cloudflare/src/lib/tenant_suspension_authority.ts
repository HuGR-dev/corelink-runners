import { CredentialObligationAuthority } from "./credential_obligation_authority";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

export interface TenantSuspensionInput {
  event_id: string;
  tenant_id: string;
  lifecycle_generation: string;
}
export interface TenantSuspensionReceipt {
  complete: boolean;
  cursor?: string;
}
export class TenantSuspensionIdentityConflict extends Error {
  readonly name = "TenantSuspensionIdentityConflict";
}
type StoredReceipt = TenantSuspensionInput & { schema_version: 1; status: "requested" | "complete"; cursor?: string };

export class TenantSuspensionAuthority {
  constructor(private readonly storage: AuthorityStorage) {}

  private key(eventId: string): string { return `tenant-suspension-event:${encodeURIComponent(eventId)}`; }
  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> { return this.storage.transaction(fn); }

  private static validate(input: unknown): TenantSuspensionInput {
    const value = input as Partial<TenantSuspensionInput> | undefined;
    if (!value || !/^[\x21-\x7e]{1,256}$/.test(value.event_id ?? "") || !/^[\x21-\x7e]{1,256}$/.test(value.tenant_id ?? "") || typeof value.lifecycle_generation !== "string" || !/^(0|[1-9][0-9]*)$/.test(value.lifecycle_generation) || value.lifecycle_generation.length > 19 || BigInt(value.lifecycle_generation) > I64_MAX) throw new Error("invalid tenant suspension identity");
    return { event_id: value.event_id!, tenant_id: value.tenant_id!, lifecycle_generation: value.lifecycle_generation };
  }

  private static validateCursor(cursor: unknown): void {
    if (cursor !== undefined && (typeof cursor !== "string" || cursor.length > 512 || !cursor.startsWith("credential-obligation:"))) throw new Error("invalid tenant suspension cursor");
  }

  private static record(key: string, raw: unknown): StoredReceipt {
    if (!raw || typeof raw !== "object") throw new Error("malformed tenant suspension receipt");
    const value = raw as Partial<StoredReceipt>;
    const identity = TenantSuspensionAuthority.validate(value);
    if (value.schema_version !== 1 || value.status !== "requested" && value.status !== "complete" || key !== `tenant-suspension-event:${encodeURIComponent(identity.event_id)}`) throw new Error("malformed tenant suspension receipt");
    TenantSuspensionAuthority.validateCursor(value.cursor);
    return { ...identity, schema_version: 1, status: value.status, ...(value.cursor === undefined ? {} : { cursor: value.cursor }) };
  }

  async begin(input: TenantSuspensionInput): Promise<TenantSuspensionReceipt> {
    const identity = TenantSuspensionAuthority.validate(input);
    const key = this.key(identity.event_id);
    const prior = await this.tx(async s => {
      const raw = await s.get(key);
      if (raw === undefined) {
        await s.put(key, { schema_version: 1, ...identity, status: "requested" });
        return { ...identity, schema_version: 1 as const, status: "requested" as const };
      }
      const record = TenantSuspensionAuthority.record(key, raw);
      if (record.tenant_id !== identity.tenant_id || record.lifecycle_generation !== identity.lifecycle_generation) throw new TenantSuspensionIdentityConflict("tenant suspension event identity conflict");
      return record;
    });
    await new CredentialObligationAuthority(this.storage).closeTenantCredentials(identity.tenant_id, identity.lifecycle_generation);
    return prior.status === "complete" ? { complete: true, ...(prior.cursor === undefined ? {} : { cursor: prior.cursor }) } : { complete: false, ...(prior.cursor === undefined ? {} : { cursor: prior.cursor }) };
  }

  async checkpoint(input: TenantSuspensionInput, expectedCursor: string | undefined, nextCursor: string | undefined, complete: boolean): Promise<boolean> {
    const identity = TenantSuspensionAuthority.validate(input);
    TenantSuspensionAuthority.validateCursor(expectedCursor); TenantSuspensionAuthority.validateCursor(nextCursor);
    if (typeof complete !== "boolean") throw new Error("invalid tenant suspension completion");
    return this.tx(async s => {
      const key = this.key(identity.event_id);
      const raw = await s.get(key);
      if (raw === undefined) throw new Error("tenant suspension event missing");
      const record = TenantSuspensionAuthority.record(key, raw);
      if (record.tenant_id !== identity.tenant_id || record.lifecycle_generation !== identity.lifecycle_generation) throw new TenantSuspensionIdentityConflict("tenant suspension event identity conflict");
      if (record.cursor !== expectedCursor) return false;
      if (record.status === "complete") return complete ? true : false;
      await s.put(key, { ...record, status: complete ? "complete" : "requested", ...(nextCursor === undefined ? {} : { cursor: nextCursor }) });
      return true;
    });
  }
}
const I64_MAX = 9_223_372_036_854_775_807n;

