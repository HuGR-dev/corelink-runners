import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

/** Durable, idempotent retry-attempt authority (AU3.21). */
export interface RetryEpochRecord {
  schema_version: 1;
  jobId: string;
  epochId: string;
}

export class RetryEpochAuthority {
  constructor(private readonly storage: AuthorityStorage) {}

  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> {
    return this.storage.transaction(fn);
  }

  private static validateId(value: unknown, name: string): asserts value is string {
    if (typeof value !== "string" || value.length === 0 || value.length > 256) {
      throw new Error(`invalid retry ${name}`);
    }
  }

  private static countKey(jobId: string): string {
    return `retry-count:v1:${encodeURIComponent(jobId)}`;
  }

  private static epochKey(jobId: string, epochId: string): string {
    return `retry-epoch:v1:${encodeURIComponent(jobId)}:${encodeURIComponent(epochId)}`;
  }

  private static validateCount(raw: unknown): number {
    if (!Number.isSafeInteger(raw) || (raw as number) < 0) {
      throw new Error("malformed retry count");
    }
    return raw as number;
  }

  private static validateRecord(key: string, jobId: string, epochId: string, raw: unknown): RetryEpochRecord {
    const record = raw as Partial<RetryEpochRecord> | undefined;
    if (record?.schema_version !== 1 || record.jobId !== jobId || record.epochId !== epochId ||
      key !== RetryEpochAuthority.epochKey(jobId, epochId)) {
      throw new Error("malformed retry epoch record");
    }
    RetryEpochAuthority.validateId(record.jobId, "job identity");
    RetryEpochAuthority.validateId(record.epochId, "epoch identity");
    return record as RetryEpochRecord;
  }

  async record(jobId: string, epochId: string, legacyFloor = 0): Promise<{ attempts: number; recorded: boolean }> {
    RetryEpochAuthority.validateId(jobId, "job identity");
    RetryEpochAuthority.validateId(epochId, "epoch identity");
    if (!Number.isSafeInteger(legacyFloor) || legacyFloor < 0) throw new Error("invalid retry legacy floor");
    const countKey = RetryEpochAuthority.countKey(jobId);
    const epochKey = RetryEpochAuthority.epochKey(jobId, epochId);

    return this.tx(async (storage) => {
      const rawCount = await storage.get(countKey);
      const rawEpoch = await storage.get(epochKey);
      const storedCount = rawCount === undefined ? 0 : RetryEpochAuthority.validateCount(rawCount);
      const count = Math.max(storedCount, legacyFloor);

      if (rawEpoch !== undefined) {
        RetryEpochAuthority.validateRecord(epochKey, jobId, epochId, rawEpoch);
        if (rawCount === undefined || count > storedCount) await storage.put(countKey, count);
        return { attempts: count, recorded: false };
      }

      if (count >= Number.MAX_SAFE_INTEGER) throw new Error("retry count overflow");
      const attempts = count + 1;
      await storage.put(countKey, attempts);
      await storage.put(epochKey, { schema_version: 1, jobId, epochId } satisfies RetryEpochRecord);
      return { attempts, recorded: true };
    });
  }

  async read(jobId: string): Promise<number> {
    RetryEpochAuthority.validateId(jobId, "job identity");
    const raw = await this.storage.get(RetryEpochAuthority.countKey(jobId));
    return raw === undefined ? 0 : RetryEpochAuthority.validateCount(raw);
  }
}
