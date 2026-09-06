import { vi } from "vitest";
import { ContainmentDO, ConcurrencySlotsDO } from "../../src/index";

function clone<T>(value: T): T {
  return value === undefined ? value : structuredClone(value);
}

class TransactionStorage {
  constructor(private readonly values: Map<string, unknown>) {}
  async get<T>(key: string): Promise<T | undefined> { return clone(this.values.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.values.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.values.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> {
    return new Map([...this.values]
      .filter(([key]) => key.startsWith(opts.prefix ?? ""))
      .map(([key, value]) => [key, clone(value) as T]));
  }
  async transaction<T>(fn: (storage: TransactionStorage) => Promise<T>): Promise<T> {
    return fn(this);
  }
}

export class WorkerAuthorityStorage {
  readonly values = new Map<string, unknown>();
  private tail: Promise<void> = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.values.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.values.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.values.delete(key); }
  async list<T>(opts: { prefix?: string } = {}): Promise<Map<string, T>> {
    return new Map([...this.values]
      .filter(([key]) => key.startsWith(opts.prefix ?? ""))
      .map(([key, value]) => [key, clone(value) as T]));
  }
  async transaction<T>(fn: (storage: TransactionStorage) => Promise<T>): Promise<T> {
    let resolveResult!: (value: T | PromiseLike<T>) => void;
    let rejectResult!: (reason?: unknown) => void;
    const result = new Promise<T>((resolve, reject) => {
      resolveResult = resolve;
      rejectResult = reject;
    });
    const run = this.tail.then(async () => {
      const snapshot = new Map([...this.values].map(([key, value]) => [key, clone(value)]));
      try {
        const value = await fn(new TransactionStorage(snapshot));
        this.values.clear();
        for (const [key, entry] of snapshot) this.values.set(key, entry);
        resolveResult(value);
      } catch (error) {
        rejectResult(error);
      }
    });
    this.tail = run.then(() => undefined, () => undefined);
    return result;
  }
}

export function makeWorkerAuthorities(runnerJobPats: unknown) {
  const containmentStorage = new WorkerAuthorityStorage();
  const containment = new ContainmentDO(
    { storage: containmentStorage } as never,
    { RUNNER_JOB_PATS: runnerJobPats } as never,
  );
  const slotsStorage = new WorkerAuthorityStorage();
  const slots = new ConcurrencySlotsDO({ storage: slotsStorage } as never, {} as never);
  const acquireCalls: unknown[][] = [];
  const releaseCalls: unknown[][] = [];
  const slotsAuthority = {
    acquire: vi.fn(async (...args: unknown[]) => {
      acquireCalls.push(args);
      return slots.acquire(...(args as [string, string, number, number, number]));
    }),
    release: vi.fn(async (...args: unknown[]) => {
      releaseCalls.push(args);
      return slots.release(...(args as [string]));
    }),
  };
  return {
    CONTAINMENT: {
      idFromName: vi.fn((name: string) => name),
      get: vi.fn(() => containment),
    },
    CONCURRENCY_SLOTS: {
      idFromName: vi.fn((name: string) => name),
      get: vi.fn(() => slotsAuthority),
      storage: slotsStorage,
    },
    containment,
    slots,
    slotsAuthority,
    acquireCalls,
    releaseCalls,
    containmentStorage,
    slotsStorage,
  };
}
