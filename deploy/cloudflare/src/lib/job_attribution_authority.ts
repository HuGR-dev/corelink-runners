import { decodeJobAttribution, type JobAttribution } from "./job_attribution";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

/** T4-W1: domain-owned storage logic. ContainmentDO retains RPC wiring. */
export class JobAttributionAuthority {
  constructor(private readonly storage: AuthorityStorage) {}
  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> { return this.storage.transaction(fn); }

async readJobAttribution(key: string): Promise<string | null> { throw new Error("T4-W1 extraction pending"); }

async putJobAttributionIfAbsent(key: string, value: string): Promise<string> { throw new Error("T4-W1 extraction pending"); }

async deleteJobAttribution(key: string): Promise<void> { throw new Error("T4-W1 extraction pending"); }

async listJobAttributions(
    tenantId: string,
    cursor?: string,
  ): Promise<{ records: JobAttribution[]; cursor?: string; complete: boolean }> { throw new Error("T4-W1 extraction pending"); }
}
