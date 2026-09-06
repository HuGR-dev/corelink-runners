import { decodeJobAttribution, type JobAttribution } from "./job_attribution";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

/** T4-W1: domain-owned storage logic. ContainmentDO retains RPC wiring. */
export class JobAttributionAuthority {
  constructor(private readonly storage: AuthorityStorage) {}
  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> { return this.storage.transaction(fn); }

async readJobAttribution(key: string): Promise<string | null> {
    return (await this.storage.get<string>(key)) ?? null;
  }

async putJobAttributionIfAbsent(key: string, value: string): Promise<string> {
    return this.tx(async s => {
      const existing = await s.get<string>(key);
      if (existing !== undefined) return existing;
      await s.put(key, value);
      return value;
    });
  }

async deleteJobAttribution(key: string): Promise<void> {
    await this.storage.delete(key);
  }

async listJobAttributions(
    tenantId: string,
    cursor?: string,
  ): Promise<{ records: JobAttribution[]; cursor?: string; complete: boolean }> {
    if (!tenantId) throw new Error("tenant id required");
    const page = await this.storage.list<string>({
      prefix: "job-attribution:",
      ...(cursor ? { startAfter: cursor } : {}),
      limit: 101,
    });
    const entries = [...page.entries()];
    const records: JobAttribution[] = [];
    for (const [key, raw] of entries) {
      const jobId = key.slice("job-attribution:".length);
      const record = decodeJobAttribution(raw, jobId);
      if (record.tenant === tenantId) records.push(record);
    }
    const lastScanned = entries.at(-1)?.[0];
    const complete = entries.length < 101;
    return complete
      ? { records, complete }
      : { records, cursor: lastScanned, complete: false };
  }
}
