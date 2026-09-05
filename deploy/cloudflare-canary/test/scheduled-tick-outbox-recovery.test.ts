import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CanaryTickOutbox } from "../src/tick_outbox";
import type { TickConfig } from "../src/config";

const config: TickConfig = {
  ingestUrl: "https://monitor.invalid/ingest",
  source: "canary",
  service: "canary",
  application: "corelink",
  keyId: "tick-key",
  credentialEpoch: "1",
  monitorRearmTupleDigest: "a".repeat(64),
  envelopeHmacKey: "envelope",
  trustedNow: () => 1_000,
};
function state(): DurableObjectState {
  const values = new Map<string, unknown>();
  const storage = {
    get: async <T>(key: string) => values.get(key) as T | undefined,
    put: async (key: string, value: unknown) => {
      values.set(key, value);
    },
    setAlarm: async () => undefined,
    deleteAlarm: async () => undefined,
  };
  const durable = {
    storage: {
      ...storage,
      transaction: async <T>(fn: (txn: DurableObjectStorage) => Promise<T>) =>
        fn(storage as unknown as DurableObjectStorage),
    },
    blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn(),
  } as unknown as DurableObjectState;
  Object.assign(durable as object, { testValues: values });
  return durable;
}

function terminal(durable: DurableObjectState): string | undefined {
  return ((durable as unknown as { testValues: Map<string, { terminal?: string }> }).testValues.get("state"))?.terminal;
}

describe("scheduled tick durable outbox", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });
  it("holds the write-ahead head across a failed send and retries the exact event", async () => {
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    const calls: unknown[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = vi.fn(async (_url, init) => {
      calls.push(JSON.parse(String(init?.body)));
      throw new Error("down");
    });
    try {
      expect(await outbox.enqueueAndDrain(config, 1_000)).toContain("pending");
      expect(await outbox.enqueueAndDrain(config, 1_001)).toContain("pending");
      expect(calls).toHaveLength(2);
      expect((calls[1] as { event_id: string }).event_id).toBe(
        (calls[0] as { event_id: string }).event_id,
      );
      expect((calls[1] as { producer_seq: number }).producer_seq).toBe(1);
    } finally {
      globalThis.fetch = original;
    }
  });

  it("does not create a head or fetch while the monitor binding is absent", async () => {
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    const fetcher = vi.spyOn(globalThis, "fetch");
    expect(await outbox.enqueueAndDrain(null, 1_000)).toContain(
      "config unavailable",
    );
    expect(fetcher).not.toHaveBeenCalled();
    fetcher.mockRestore();
  });

  it("makes malformed probe configuration an authenticated, distinct signal when a fixture capability is injected", async () => {
    const outbox = new CanaryTickOutbox(state());
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response("{}", { status: 200 }));
    await outbox.enqueueAndDrain(config, 1_000, true);
    expect(JSON.parse(String(fetcher.mock.calls[0]?.[1]?.body)).event_id).toBe(
      "CANARY_CONFIG_INVALID-1",
    );
    fetcher.mockRestore();
  });

  it("bounds a hung response body at the original 60-second deadline", async () => {
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    const body = new Response(
      new ReadableStream({
        pull: () => new Promise<void>(() => undefined),
        cancel: () => new Promise<void>(() => undefined),
      }),
      { status: 200 },
    );
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(body);
    const pending = outbox.enqueueAndDrain(config, 1_000);
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    await vi.advanceTimersByTimeAsync(60_000);
    await expect(pending).resolves.toContain("TIMED_OUT");
    expect(terminal(durable)).toBe("TIMED_OUT");
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({
      signal: expect.any(AbortSignal),
    });
  });

  it("passes an abort deadline to a fetch that otherwise never settles", async () => {
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    let aborted = false;
    const controller = new AbortController();
    const timeout = vi
      .spyOn(AbortSignal, "timeout")
      .mockImplementation((ms: number) => {
        return controller.signal;
      });
    const fetcher = vi.spyOn(globalThis, "fetch").mockImplementation(
      (_url, init) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener(
            "abort",
            () => {
              aborted = true;
              reject(new Error("aborted"));
            },
            { once: true },
          );
        }),
    );
    const pending = outbox.enqueueAndDrain(config, 1_000);
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    expect(fetcher).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(60_000);
    controller.abort();
    await expect(pending).resolves.toContain("TIMED_OUT");
    expect(terminal(durable)).toBe("TIMED_OUT");
    expect(aborted).toBe(true);
    fetcher.mockRestore();
    timeout.mockRestore();
  });

  it("never authorizes an ACK observed at the deadline", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const trustedNow = 61_000;
    const outbox = new CanaryTickOutbox(state());
    const dynamic = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async (_url, init) => {
        const envelope = JSON.parse(String(init?.body));
        const signed = await import("./scheduled-tick-fixtures").then(
          ({ signedToken, ACK_FIELDS }) =>
            signedToken(
              {
                ack_version: "1",
                event_id: envelope.event_id,
                producer_seq: envelope.producer_seq,
                payload_digest: envelope.payload_digest,
                source: envelope.source,
                service: envelope.service,
                application: envelope.application,
                key_id: envelope.key_id,
                credential_epoch: envelope.credential_epoch,
                monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest,
                ingest_commit_id: "commit",
                committed_at: 1_001,
                signer_key_id: "ack-signer",
                signer_epoch: "4",
              },
              ACK_FIELDS,
              "ack-signer",
              "4",
            ),
        );
        config.ackVerifier = signed.verifier;
        return new Response(JSON.stringify(signed.token), { status: 200 });
      });
    config.trustedNow = () => trustedNow;
    expect(await outbox.enqueueAndDrain(config, 1_000)).toContain(
      "invalid ACK",
    );
    vi.setSystemTime(61_000);
    expect(await outbox.enqueueAndDrain(config, 61_000)).toContain("TIMED_OUT");
    dynamic.mockRestore();
    config.trustedNow = () => 1_000;
    delete config.ackVerifier;
    vi.useRealTimers();
  });

  it("bounds a verifier that never resolves at the same original deadline", async () => {
    const verifier = {
      verify: vi.fn(async () => await new Promise<"valid">(() => undefined)),
    };
    const boundedConfig = {
      ...config,
      ackVerifier: verifier,
      trustedNow: () => 1_001,
    };
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async (_url, init) => {
        const e = JSON.parse(String(init?.body));
        return new Response(
          JSON.stringify({
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
            signature: "fixture",
          }),
          { status: 200 },
        );
      });
    const timeout = vi
      .spyOn(AbortSignal, "timeout")
      .mockReturnValue(new AbortController().signal);
    const pending = outbox.enqueueAndDrain(boundedConfig, 1_000);
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    await vi.waitFor(() => expect(verifier.verify).toHaveBeenCalled());
    const outcome = Promise.race([
      pending,
      new Promise<string>((resolve) =>
        setTimeout(() => resolve("timeout"), 60_001),
      ),
    ]);
    await vi.advanceTimersByTimeAsync(60_001);
    expect(await outcome).not.toBe("timeout");
    expect(terminal(durable)).toBe("TIMED_OUT");
    expect(timeout.mock.calls[0]?.[0]).toBeGreaterThan(0);
    expect(timeout.mock.calls[0]?.[0]).toBeLessThanOrEqual(60_000);
  });

  it("reschedules an early alarm and closes the hole at the original deadline", async () => {
    vi.useFakeTimers();
    const values = new Map<string, unknown>();
    const alarms: number[] = [];
    const storage = {
      get: async <T>(key: string) => values.get(key) as T | undefined,
      put: async (key: string, value: unknown) => {
        values.set(key, value);
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
      blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn(),
    } as unknown as DurableObjectState;
    const outbox = new CanaryTickOutbox(durable);
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("offline"));
    await outbox.enqueueAndDrain(config, 1_000);
    vi.setSystemTime(10_000);
    await outbox.alarm();
    expect(alarms.at(-1)).toBe(61_000);
    vi.setSystemTime(61_000);
    await outbox.alarm();
    expect(
      (await storage.get<{ head?: unknown; terminal?: string }>("state"))
        ?.terminal,
    ).toBe("TIMED_OUT");
    vi.restoreAllMocks();
    vi.useRealTimers();
  });
});
