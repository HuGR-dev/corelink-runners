import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CanaryTickOutbox } from "../src/tick_outbox";
import type { TickConfig } from "../src/config";
import { ACK_FIELDS, signedToken } from "./scheduled-tick-fixtures";

const config: TickConfig = {
  ingestUrl: "https://monitor.invalid",
  source: "canary",
  service: "canary",
  application: "corelink",
  keyId: "lane",
  credentialEpoch: "1",
  monitorRearmTupleDigest: "a".repeat(64),
  envelopeHmacKey: "fixture",
  trustedNow: () => 1_000,
};
function state(): {
  durable: DurableObjectState;
  values: Map<string, unknown>;
  alarms: number[];
} {
  const values = new Map<string, unknown>();
  const alarms: number[] = [];
  let queue = Promise.resolve();
  const copy = <T>(value: T): T =>
    value === undefined ? value : structuredClone(value);
  const storage = {
    get: async <T>(key: string) => copy(values.get(key)) as T | undefined,
    put: async (key: string, value: unknown) => {
      values.set(key, copy(value));
    },
    setAlarm: async (at: number) => {
      alarms.push(at);
    },
    deleteAlarm: async () => undefined,
  };
  const durable = {
    storage: {
      ...storage,
      transaction: async <T>(fn: (txn: DurableObjectStorage) => Promise<T>) =>
        fn(storage as unknown as DurableObjectStorage),
    },
    blockConcurrencyWhile: <T>(fn: () => Promise<T>) => {
      const run = queue.then(fn);
      queue = run.then(
        () => undefined,
        () => undefined,
      );
      return run;
    },
  } as unknown as DurableObjectState;
  return { durable, values, alarms };
}
describe("scheduled tick order", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
  });
  afterEach(() => {
    vi.useRealTimers();
  });
  it("does not emit a successor while its durable head lacks a verified ACK", async () => {
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async () => new Response("{}", { status: 200 }));
    const fixture = state();
    const outbox = new CanaryTickOutbox(fixture.durable);
    await outbox.enqueueAndDrain(config, 1_000, 1_000);
    await outbox.enqueueAndDrain(config, 1_001, 1_001);
    const ids = fetcher.mock.calls.map(
      ([, init]) => JSON.parse(String(init?.body)).event_id,
    );
    expect(ids).toEqual(["canary-tick-1", "canary-tick-1"]);
    fetcher.mockRestore();
  });

  it("serializes competing enqueue calls so durable capacity remains one", async () => {
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async () => new Response("{}", { status: 200 }));
    const fixture = state();
    const outbox = new CanaryTickOutbox(fixture.durable);
    await Promise.all([
      outbox.enqueueAndDrain(config, 1_000, 1_000),
      outbox.enqueueAndDrain(config, 1_001, 1_001),
    ]);
    const ids = fetcher.mock.calls.map(
      ([, init]) => JSON.parse(String(init?.body)).event_id,
    );
    expect(ids).toEqual(["canary-tick-1", "canary-tick-1"]);
    const bodies = fetcher.mock.calls.map(([, init]) => String(init?.body));
    expect(bodies[0]).toBe(bodies[1]);
    expect(fixture.alarms).toEqual([61_000]);
    expect(fixture.values.get("state")).toMatchObject({
      seq: 1,
      head: { envelope: { event_id: "canary-tick-1", producer_seq: 1 } },
    });
    fetcher.mockRestore();
  });

  it("cannot let a stale in-flight response clear a successor head", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const values = new Map<string, unknown>();
    let release!: () => void;
    let releaseReadyResolve!: () => void;
    const releaseReady = new Promise<void>((resolve) => {
      releaseReadyResolve = resolve;
    });
    let releaseTransaction!: () => void;
    let transactionStarted!: () => void;
    const transactionReady = new Promise<void>((resolve) => {
      transactionStarted = resolve;
    });
    const transactionGate = new Promise<void>((resolve) => {
      releaseTransaction = resolve;
    });
    let pauseTransaction = false;
    const copy = <T>(value: T): T =>
      value === undefined ? value : structuredClone(value);
    const storage = {
      get: async <T>(key: string) => copy(values.get(key)) as T | undefined,
      put: async (key: string, value: unknown) => {
        values.set(key, copy(value));
      },
      setAlarm: async () => undefined,
      deleteAlarm: async () => undefined,
    };
    const durable = {
      storage: {
        ...storage,
        transaction: async <T>(
          fn: (txn: DurableObjectStorage) => Promise<T>,
        ) => {
          if (pauseTransaction) {
            transactionStarted();
            await transactionGate;
          }
          return fn(storage as unknown as DurableObjectStorage);
        },
      },
      blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn(),
    } as unknown as DurableObjectState;
    let verifier: Awaited<ReturnType<typeof signedToken>>["verifier"];
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async (_url, init) => {
        const e = JSON.parse(String(init?.body));
        const signed = await signedToken(
          {
            ack_version: "1",
            event_id: e.event_id,
            producer_seq: e.producer_seq,
            payload_digest: e.payload_digest,
            source: e.source,
            service: e.service,
            application: e.application,
            key_id: e.key_id,
            credential_epoch: e.credential_epoch,
            monitor_rearm_tuple_digest: e.monitor_rearm_tuple_digest,
            ingest_commit_id: "commit",
            committed_at: 1_001,
            signer_key_id: "ack-signer",
            signer_epoch: "4",
          },
          ACK_FIELDS,
          "ack-signer",
          "4",
        );
        verifier = signed.verifier;
        return new Promise<Response>((resolve) => {
          release = () =>
            resolve(
              new Response(JSON.stringify(signed.token), { status: 200 }),
            );
          releaseReadyResolve();
        });
      });
    const outbox = new CanaryTickOutbox(durable);
    const inFlight = outbox.enqueueAndDrain(
      {
        ...config,
        get ackVerifier() {
          return verifier;
        },
        trustedNow: () => 1_001,
      },
      1_000,
      1_000,
    );
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    await releaseReady;
    pauseTransaction = true;
    release();
    await transactionReady;
    const original = (await storage.get<{
      head: { envelope: Record<string, unknown> };
    }>("state"))!.head;
    await storage.put("state", {
      seq: 2,
      head: {
        envelope: {
          ...original.envelope,
          event_id: "canary-tick-2",
          producer_seq: 2,
        },
        enqueuedAt: 1_001,
      },
    });
    releaseTransaction();
    expect(await inFlight).toContain("head changed");
    expect(
      (await storage.get<{ head: { envelope: { event_id: string } } }>(
        "state",
      ))!.head.envelope.event_id,
    ).toBe("canary-tick-2");
    fetcher.mockRestore();
    vi.useRealTimers();
  });

  it("persists TIMED_OUT when the terminal CAS reaches the exact deadline", async () => {
    const values = new Map<string, unknown>();
    let transactions = 0;
    const clone = <T>(value: T): T =>
      value === undefined ? value : structuredClone(value);
    const storage = {
      get: async <T>(key: string) => clone(values.get(key)) as T | undefined,
      put: async (key: string, value: unknown) => {
        values.set(key, clone(value));
      },
      setAlarm: async () => undefined,
      deleteAlarm: async () => undefined,
    };
    const durable = {
      storage: {
        ...storage,
        transaction: async <T>(
          fn: (txn: DurableObjectStorage) => Promise<T>,
        ) => {
          transactions += 1;
          if (transactions === 2) vi.setSystemTime(61_000);
          return fn(storage as unknown as DurableObjectStorage);
        },
      },
      blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn(),
    } as unknown as DurableObjectState;
    let verifier: Awaited<ReturnType<typeof signedToken>>["verifier"];
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async (_url, init) => {
        const e = JSON.parse(String(init?.body));
        const signed = await signedToken(
          {
            ack_version: "1",
            event_id: e.event_id,
            producer_seq: e.producer_seq,
            payload_digest: e.payload_digest,
            source: e.source,
            service: e.service,
            application: e.application,
            key_id: e.key_id,
            credential_epoch: e.credential_epoch,
            monitor_rearm_tuple_digest: e.monitor_rearm_tuple_digest,
            ingest_commit_id: "commit",
            committed_at: 1_001,
            signer_key_id: "ack-signer",
            signer_epoch: "4",
          },
          ACK_FIELDS,
          "ack-signer",
          "4",
        );
        verifier = signed.verifier;
        return new Response(JSON.stringify(signed.token), { status: 200 });
      });
    const outbox = new CanaryTickOutbox(durable);
    const result = await outbox.enqueueAndDrain(
      {
        ...config,
        get ackVerifier() {
          return verifier;
        },
        trustedNow: () => 1_001,
      },
      1_000,
      1_000,
    );
    expect(result).toContain("TIMED_OUT");
    expect(
      (await storage.get<{ head?: unknown; terminal?: string }>("state"))?.head,
    ).toBeUndefined();
    expect((await storage.get<{ terminal?: string }>("state"))?.terminal).toBe(
      "TIMED_OUT",
    );
    fetcher.mockRestore();
  });
});
