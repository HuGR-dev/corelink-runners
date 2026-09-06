export interface RetryEpochAuthorityRpc {
  record(jobId: string, epochId: string, legacyFloor?: number): Promise<{ attempts: number; recorded: boolean }>;
  read(jobId: string): Promise<number>;
}

export interface RetryEpochAuthorityStub {
  recordRetry(jobId: string, epochId: string, legacyFloor?: number): Promise<{ attempts: number; recorded: boolean }>;
  readRetry(jobId: string): Promise<number>;
}

/** Explicit RPC adapter: the storage authority exports recordRetry/readRetry. */
export function retryEpochClient(getStub: () => RetryEpochAuthorityStub): RetryEpochAuthorityRpc {
  return {
    record: (...args) => getStub().recordRetry(...args),
    read: (...args) => getStub().readRetry(...args),
  };
}
