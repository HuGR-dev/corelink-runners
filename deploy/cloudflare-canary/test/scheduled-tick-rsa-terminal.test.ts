import { afterEach, describe, expect, it, vi } from "vitest";
import type { TickConfig } from "../src/config";
import { CanaryTickOutbox } from "../src/tick_outbox";
// @ts-ignore Runtime-only bridge imports the real Node RSA-PSS monitor modules.
import { makeRsaSigner, createTerminal } from "./monitor-rsa-bridge.mjs";

function state(): DurableObjectState {
  const values = new Map<string, unknown>();
  const storage = {
    get: async <T>(key: string) => values.get(key) as T | undefined,
    put: async (key: string, value: unknown) => void values.set(key, value),
    setAlarm: async () => undefined,
    deleteAlarm: async () => undefined,
  };
  const durable = {
    storage: { ...storage, transaction: async <T>(fn: (txn: typeof storage) => Promise<T>) => fn(storage) },
    blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn(),
  } as unknown as DurableObjectState;
  Object.assign(durable as object, { testValues: values });
  return durable;
}

describe("cross-runtime RSA-PSS historical terminal", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it.each([false, true])("drains a late terminal for %s periodic kind and permits a successor", async (configInvalid) => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const durable = state();
    const outbox = new CanaryTickOutbox(durable);
    const { signer, verifier } = makeRsaSigner();
    const config: TickConfig = {
      ingestUrl: "https://monitor.invalid/ingest", source: "canary", service: "canary", application: "corelink",
      keyId: "tick-key", credentialEpoch: "1", monitorRearmTupleDigest: "a".repeat(64), envelopeHmacKey: "envelope",
      ackVerifier: verifier, trustedNow: () => 61_001,
    };
    let calls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (_url, init) => {
      const envelope = JSON.parse(String(init?.body));
      const ackFields = {
        ack_version: "1", event_id: envelope.event_id, producer_seq: envelope.producer_seq,
        payload_digest: envelope.payload_digest, source: envelope.source, service: envelope.service,
        application: envelope.application, key_id: envelope.key_id, credential_epoch: envelope.credential_epoch,
        monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest, ingest_commit_id: `commit-${++calls}`,
        committed_at: 1_001, signer_key_id: signer.identity.keyId, signer_epoch: signer.identity.epoch,
      };
      const terminal = await createTerminal(ackFields, signer);
      return new Response(JSON.stringify(terminal), { status: 200 });
    });
    expect(await outbox.enqueueAndDrain(config, 1_000, 1_000, configInvalid)).toBe("tick terminal: HISTORICAL_NO_STATE");
    expect(await outbox.enqueueAndDrain(config, 1_001, 1_001, configInvalid)).toBe("tick terminal: HISTORICAL_NO_STATE");
    expect(calls).toBe(2);
  });

  it("rejects nested hash, signer epoch, and terminal signature mutations", async () => {
    const { signer } = makeRsaSigner();
    const fields = {
      ack_version: "1", event_id: "e", producer_seq: 1, payload_digest: "a".repeat(64), source: "s", service: "s",
      application: "a", key_id: "k", credential_epoch: "1", monitor_rearm_tuple_digest: "b".repeat(64),
      ingest_commit_id: "c", committed_at: 1_001, signer_key_id: signer.identity.keyId, signer_epoch: signer.identity.epoch,
    };
    const terminal = await createTerminal(fields, signer);
    const head = { envelope: { kind: "canary-tick", ...fields, occurred_at: 1_000, scheduled_for: 1_000, version: "1", signature: "x" }, enqueuedAt: 1_000 } as never;
    const { validHistoricalTerminal } = await import("../src/tick_outbox");
    const verifier = { verify: async () => "invalid" as const };
    expect(await validHistoricalTerminal({ ...terminal, signer_epoch: "old" }, head, verifier, 61_001)).toBe("invalid");
    expect(await validHistoricalTerminal({ ...terminal, ack: { ...terminal.ack, signature: "mutated" } }, head, verifier, 61_001)).toBe("invalid");
    expect(await validHistoricalTerminal({ ...terminal, signature: "mutated" }, head, verifier, 61_001)).toBe("invalid");
    expect(await validHistoricalTerminal(terminal, head, { verify: async () => "revoked" }, 61_001)).toBe("revoked");
  });
});
