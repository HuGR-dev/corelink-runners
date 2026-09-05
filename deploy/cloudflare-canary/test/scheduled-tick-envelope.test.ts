import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseProbeFlag, type TickConfig } from "../src/config";
import { CanaryTickOutbox } from "../src/tick_outbox";

const ENVELOPE_FIELDS = [
  "event_id",
  "producer_seq",
  "payload_digest",
  "source",
  "service",
  "application",
  "key_id",
  "credential_epoch",
  "monitor_rearm_tuple_digest",
  "occurred_at",
] as const;
const TUPLE_DIGEST = "b".repeat(64);
const HMAC_KEY = "scheduled-tick-test-key";

const config: TickConfig = {
  ingestUrl: "https://monitor.invalid/ticks",
  source: "canary-tick",
  service: "corelink-canary",
  application: "corelink-runners",
  keyId: "tick-key-current",
  credentialEpoch: "epoch-7",
  monitorRearmTupleDigest: TUPLE_DIGEST,
  envelopeHmacKey: HMAC_KEY,
  trustedNow: () => 20_000,
  ackVerifier: { verify: async () => "valid" },
};

function durableState(): DurableObjectState {
  const values = new Map<string, unknown>();
  let queue = Promise.resolve();
  const copy = <T>(value: T): T =>
    value === undefined ? value : structuredClone(value);
  const storage = {
    get: async <T>(key: string) => copy(values.get(key)) as T | undefined,
    put: async (key: string, value: unknown) => values.set(key, copy(value)),
    setAlarm: async () => undefined,
    deleteAlarm: async () => undefined,
  };
  return {
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
}

function hex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

async function sha256(value: string): Promise<string> {
  return hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)));
}

async function hmac(value: string, key: string): Promise<string> {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return hex(
    await crypto.subtle.sign(
      "HMAC",
      cryptoKey,
      new TextEncoder().encode(value),
    ),
  );
}

function canonicalEnvelope(value: Record<string, unknown>): string {
  return JSON.stringify(ENVELOPE_FIELDS.map((field) => value[field]));
}

function ackFor(envelope: Record<string, unknown>): Record<string, unknown> {
  return {
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
    ingest_commit_id: `commit-${envelope.producer_seq}`,
    committed_at: envelope.occurred_at,
    signer_key_id: "monitor-signer",
    signer_epoch: "1",
    signature: "monitor-signature",
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("scheduled tick canonical envelope", () => {
  it("emits exact authenticated bytes bound to domain, key epoch, tuple and nonce", async () => {
    const requests: Record<string, unknown>[] = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(async (_input, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      requests.push(body);
      return new Response(JSON.stringify(ackFor(body)), { status: 200 });
    });

    const now = 12_345;
    vi.setSystemTime(now);
    const result = await new CanaryTickOutbox(durableState()).enqueueAndDrain(
      config,
      now,
    );
    expect(result).toBe("tick terminal: ACKED");
    expect(requests).toHaveLength(1);

    const envelope = requests[0]!;
    expect(Object.keys(envelope)).toEqual([...ENVELOPE_FIELDS, "signature"]);
    expect(envelope).toMatchObject({
      event_id: "canary-tick-1",
      producer_seq: 1,
      source: config.source,
      service: config.service,
      application: config.application,
      key_id: config.keyId,
      credential_epoch: config.credentialEpoch,
      monitor_rearm_tuple_digest: TUPLE_DIGEST,
      occurred_at: now,
    });
    expect(JSON.stringify(envelope)).not.toContain(HMAC_KEY);
    expect(envelope.payload_digest).toBe(
      await sha256(`canary-tick:1:${now}`),
    );

    const expectedCanonical = canonicalEnvelope(envelope);
    expect(expectedCanonical).toBe(
      JSON.stringify([
        "canary-tick-1",
        1,
        envelope.payload_digest,
        config.source,
        config.service,
        config.application,
        config.keyId,
        config.credentialEpoch,
        TUPLE_DIGEST,
        now,
      ]),
    );
    expect(envelope.signature).toBe(await hmac(expectedCanonical, HMAC_KEY));
    expect(envelope.signature).not.toBe(
      await hmac(
        canonicalEnvelope({ ...envelope, producer_seq: 2 }),
        HMAC_KEY,
      ),
    );
  });

  it("advances sequence only after terminal heads and authenticates invalid config separately", async () => {
    const requests: Record<string, unknown>[] = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(async (_input, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      requests.push(body);
      return new Response(JSON.stringify(ackFor(body)), { status: 200 });
    });
    const outbox = new CanaryTickOutbox(durableState());

    expect(parseProbeFlag("0")).toEqual({ valid: true, enabled: false });
    expect(parseProbeFlag("invalid")).toMatchObject({ valid: false });
    await outbox.enqueueAndDrain(config, 1_000);
    vi.setSystemTime(2_000);
    await outbox.enqueueAndDrain(config, 2_000, true);

    expect(requests.map((body) => body.event_id)).toEqual([
      "canary-tick-1",
      "CANARY_CONFIG_INVALID-2",
    ]);
    expect(requests.map((body) => body.producer_seq)).toEqual([1, 2]);
    for (const body of requests) {
      expect(Object.keys(body)).toEqual([...ENVELOPE_FIELDS, "signature"]);
      expect(body.signature).toBe(
        await hmac(canonicalEnvelope(body), HMAC_KEY),
      );
      expect(body.monitor_rearm_tuple_digest).toBe(TUPLE_DIGEST);
    }
  });
});
