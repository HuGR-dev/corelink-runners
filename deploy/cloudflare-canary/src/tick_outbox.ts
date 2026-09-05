import type { AckVerifier, TickConfig } from "./config";

export const TICK_DEADLINE_MS = 60_000;
const text = new TextEncoder();
const stateKey = "state";
const envelopeFields = [
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
const ackFields = [
  "ack_version",
  "event_id",
  "producer_seq",
  "payload_digest",
  "source",
  "service",
  "application",
  "key_id",
  "credential_epoch",
  "monitor_rearm_tuple_digest",
  "ingest_commit_id",
  "committed_at",
  "signer_key_id",
  "signer_epoch",
] as const;
const recoveryFields = [
  "recovery_version",
  "event_id",
  "producer_seq",
  "payload_digest",
  "source",
  "service",
  "application",
  "key_id",
  "credential_epoch",
  "original_monitor_rearm_tuple_digest",
  "ingest_commit_id",
  "original_ack_digest",
  "revocation_record_digest",
  "signer_rotation_manifest_digest",
  "signer_manifest_generation",
  "signer_manifest_witness_root_digest",
  "current_monitor_rearm_tuple_digest",
  "recovery_signer_key_id",
  "recovery_signer_epoch",
  "issued_at",
] as const;

export interface TickEnvelope {
  event_id: string;
  producer_seq: number;
  payload_digest: string;
  source: string;
  service: string;
  application: string;
  key_id: string;
  credential_epoch: string;
  monitor_rearm_tuple_digest: string;
  occurred_at: number;
  signature: string;
}
export interface AckToken {
  ack_version: string;
  event_id: string;
  producer_seq: number;
  payload_digest: string;
  source: string;
  service: string;
  application: string;
  key_id: string;
  credential_epoch: string;
  monitor_rearm_tuple_digest: string;
  ingest_commit_id: string;
  committed_at: number;
  signer_key_id: string;
  signer_epoch: string;
  signature: string;
}
export interface AckRecovery {
  recovery_version: string;
  event_id: string;
  producer_seq: number;
  payload_digest: string;
  source: string;
  service: string;
  application: string;
  key_id: string;
  credential_epoch: string;
  original_monitor_rearm_tuple_digest: string;
  ingest_commit_id: string;
  original_ack_digest: string;
  revocation_record_digest: string;
  signer_rotation_manifest_digest: string;
  signer_manifest_generation: number;
  signer_manifest_witness_root_digest: string;
  current_monitor_rearm_tuple_digest: string;
  recovery_signer_key_id: string;
  recovery_signer_epoch: string;
  issued_at: number;
  signature: string;
}
type Head = {
  envelope: TickEnvelope;
  enqueuedAt: number;
  originalAck?: AckToken;
};
type State = {
  seq: number;
  head?: Head;
  terminal?: "ACKED" | "TIMED_OUT" | "CONFIG_UNAVAILABLE";
};

const record = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === "object" && !Array.isArray(value));
const nonempty = (value: unknown) =>
  typeof value === "string" && value.length > 0;
const digest = (value: unknown) =>
  typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const positive = (value: unknown) =>
  typeof value === "number" && Number.isSafeInteger(value) && value > 0;
const timestamp = (value: unknown) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const canonical = (value: object, fields: readonly string[]) =>
  JSON.stringify(
    fields.map((field) => (value as Record<string, unknown>)[field]),
  );
function strict(
  value: unknown,
  fields: readonly string[],
  checks: Record<string, (value: unknown) => boolean>,
): boolean {
  return (
    record(value) &&
    Object.keys(value).length === fields.length + 1 &&
    Object.keys(value).every(
      (key) => key === "signature" || fields.includes(key),
    ) &&
    typeof value.signature === "string" &&
    fields.every((field) => checks[field]!(value[field]))
  );
}
const ackChecks = {
  ack_version: (v: unknown) => v === "1",
  event_id: nonempty,
  producer_seq: positive,
  payload_digest: digest,
  source: nonempty,
  service: nonempty,
  application: nonempty,
  key_id: nonempty,
  credential_epoch: nonempty,
  monitor_rearm_tuple_digest: digest,
  ingest_commit_id: nonempty,
  committed_at: timestamp,
  signer_key_id: nonempty,
  signer_epoch: nonempty,
};
const recoveryChecks = {
  recovery_version: (v: unknown) => v === "1",
  event_id: nonempty,
  producer_seq: positive,
  payload_digest: digest,
  source: nonempty,
  service: nonempty,
  application: nonempty,
  key_id: nonempty,
  credential_epoch: nonempty,
  original_monitor_rearm_tuple_digest: digest,
  ingest_commit_id: nonempty,
  original_ack_digest: digest,
  revocation_record_digest: digest,
  signer_rotation_manifest_digest: digest,
  signer_manifest_generation: positive,
  signer_manifest_witness_root_digest: digest,
  current_monitor_rearm_tuple_digest: digest,
  recovery_signer_key_id: nonempty,
  recovery_signer_epoch: nonempty,
  issued_at: timestamp,
};
async function hash(value: string): Promise<string> {
  return [
    ...new Uint8Array(
      await crypto.subtle.digest("SHA-256", text.encode(value)),
    ),
  ]
    .map((n) => n.toString(16).padStart(2, "0"))
    .join("");
}
async function hmac(value: string, key: string): Promise<string> {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    text.encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return [
    ...new Uint8Array(
      await crypto.subtle.sign("HMAC", cryptoKey, text.encode(value)),
    ),
  ]
    .map((n) => n.toString(16).padStart(2, "0"))
    .join("");
}
async function bodyUntil(
  response: Response,
  deadline: number,
): Promise<string | undefined> {
  const remaining = deadline - Date.now();
  if (remaining <= 0) return undefined;
  const reader = response.body?.getReader();
  if (!reader) return "";
  const decoder = new TextDecoder();
  let body = "";
  let timedOut = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const timeout = new Promise<undefined>((resolve) => {
      timer = setTimeout(() => {
        timedOut = true;
        resolve(undefined);
      }, remaining);
    });
    while (true) {
      const next = await Promise.race([reader.read(), timeout]);
      if (next === undefined || timedOut) {
        void reader.cancel().catch(() => undefined);
        return undefined;
      }
      if (next.done) return body + decoder.decode();
      body += decoder.decode(next.value, { stream: true });
    }
  } catch {
    return "";
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    reader.releaseLock();
  }
}

async function beforeDeadline<T>(
  operation: () => Promise<T>,
  deadline: number,
): Promise<T | undefined> {
  const remaining = deadline - Date.now();
  if (remaining <= 0) return undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      operation(),
      new Promise<undefined>((resolve) => {
        timer = setTimeout(() => resolve(undefined), remaining);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
const sameHead = (state: State | undefined, head: Head) =>
  state?.head?.envelope.event_id === head.envelope.event_id &&
  state.head.envelope.producer_seq === head.envelope.producer_seq &&
  state.head.enqueuedAt === head.enqueuedAt;

export async function validAck(
  ack: AckToken,
  head: Head,
  verifier: AckVerifier | undefined,
  now: number,
): Promise<"valid" | "revoked" | "invalid"> {
  const envelope = head.envelope;
  if (
    !verifier ||
    !strict(ack, ackFields, ackChecks) ||
    ack.event_id !== envelope.event_id ||
    ack.producer_seq !== envelope.producer_seq ||
    ack.payload_digest !== envelope.payload_digest ||
    ack.source !== envelope.source ||
    ack.service !== envelope.service ||
    ack.application !== envelope.application ||
    ack.key_id !== envelope.key_id ||
    ack.credential_epoch !== envelope.credential_epoch ||
    ack.monitor_rearm_tuple_digest !== envelope.monitor_rearm_tuple_digest ||
    ack.committed_at < head.enqueuedAt ||
    ack.committed_at > now ||
    now >= head.enqueuedAt + TICK_DEADLINE_MS
  )
    return "invalid";
  return (
    (await beforeDeadline(
      () =>
        verifier.verify(
          canonical(ack, ackFields),
          ack.signature,
          ack.signer_key_id,
          ack.signer_epoch,
        ),
      head.enqueuedAt + TICK_DEADLINE_MS,
    )) ?? "invalid"
  );
}

export async function validRecovery(
  recovery: AckRecovery,
  head: Head,
  original: AckToken,
  verifier: AckVerifier | undefined,
): Promise<boolean> {
  const envelope = head.envelope;
  if (
    !verifier ||
    !strict(recovery, recoveryFields, recoveryChecks) ||
    recovery.event_id !== envelope.event_id ||
    recovery.producer_seq !== envelope.producer_seq ||
    recovery.payload_digest !== envelope.payload_digest ||
    recovery.source !== envelope.source ||
    recovery.service !== envelope.service ||
    recovery.application !== envelope.application ||
    recovery.key_id !== envelope.key_id ||
    recovery.credential_epoch !== envelope.credential_epoch ||
    recovery.original_monitor_rearm_tuple_digest !==
      envelope.monitor_rearm_tuple_digest ||
    recovery.current_monitor_rearm_tuple_digest !==
      envelope.monitor_rearm_tuple_digest ||
    recovery.ingest_commit_id !== original.ingest_commit_id ||
    recovery.original_ack_digest !==
      (await hash(
        JSON.stringify([
          ...JSON.parse(canonical(original, ackFields)),
          original.signature,
        ]),
      ))
  )
    return false;
  return (
    (await beforeDeadline(
      () =>
        verifier.verify(
          canonical(recovery, recoveryFields),
          recovery.signature,
          recovery.recovery_signer_key_id,
          recovery.recovery_signer_epoch,
        ),
      head.enqueuedAt + TICK_DEADLINE_MS,
    )) === "valid"
  );
}

export class CanaryTickOutbox {
  constructor(private readonly state: DurableObjectState) {}

  async enqueueAndDrain(
    config: TickConfig | null,
    now: number,
    configInvalid = false,
  ): Promise<string> {
    const head = await this.reserve(config, now, configInvalid);
    if (!head || !config) return "tick config unavailable";
    const deadline = head.enqueuedAt + TICK_DEADLINE_MS;
    const remaining = deadline - Date.now();
    if (remaining <= 0) return this.terminal(head, "TIMED_OUT", Date.now());
    let response: Response;
    try {
      response = await fetch(config.ingestUrl, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(head.envelope),
        signal: AbortSignal.timeout(remaining),
      });
    } catch {
      return "tick head pending: transmit failed";
    }
    const body = await bodyUntil(response, deadline);
    if (body === undefined) return "tick head pending: deadline elapsed";
    let candidate: unknown = null;
    try {
      candidate = response.ok ? JSON.parse(body) : null;
    } catch {
      /* invalid token remains pending */
    }
    if (
      !response.ok ||
      !sameHead(await this.state.storage.get<State>(stateKey), head)
    )
      return "tick head pending: invalid ACK";
    const observed = config.trustedNow?.();
    if (strict(candidate, ackFields, ackChecks)) {
      const status =
        observed === undefined
          ? "invalid"
          : await validAck(
              candidate as AckToken,
              head,
              config.ackVerifier,
              observed,
            );
      if (status === "valid") return this.terminal(head, "ACKED", Date.now());
      if (status === "revoked") {
        await this.persistRecovery(head, candidate as AckToken);
        return "tick head pending: ACK_RECOVERY";
      }
    }
    if (
      head.originalAck &&
      strict(candidate, recoveryFields, recoveryChecks) &&
      (await validRecovery(
        candidate as AckRecovery,
        head,
        head.originalAck,
        config.recoveryVerifier,
      ))
    )
      return this.terminal(
        head,
        Date.now() < deadline ? "ACKED" : "TIMED_OUT",
        Date.now(),
      );
    return "tick head pending: invalid ACK";
  }

  private async reserve(
    config: TickConfig | null,
    now: number,
    configInvalid: boolean,
  ): Promise<Head | undefined> {
    return this.state.blockConcurrencyWhile(async () => {
      const current = (await this.state.storage.get<State>(stateKey)) ?? {
        seq: 0,
      };
      if (current.head) return current.head;
      if (!config) {
        current.terminal = "CONFIG_UNAVAILABLE";
        await this.state.storage.put(stateKey, current);
        return undefined;
      }
      const seq = current.seq + 1;
      const kind = configInvalid ? "CANARY_CONFIG_INVALID" : "canary-tick";
      const unsigned = {
        event_id: `${kind}-${seq}`,
        producer_seq: seq,
        payload_digest: await hash(`${kind}:${seq}:${now}`),
        source: config.source,
        service: config.service,
        application: config.application,
        key_id: config.keyId,
        credential_epoch: config.credentialEpoch,
        monitor_rearm_tuple_digest: config.monitorRearmTupleDigest,
        occurred_at: now,
      };
      const head: Head = {
        envelope: {
          ...unsigned,
          signature: await hmac(
            canonical(unsigned, envelopeFields),
            config.envelopeHmacKey,
          ),
        },
        enqueuedAt: now,
      };
      await this.state.storage.transaction(async (txn) => {
        await txn.put(stateKey, { seq, head });
        await txn.setAlarm(now + TICK_DEADLINE_MS);
      });
      return head;
    });
  }

  private async persistRecovery(
    head: Head,
    originalAck: AckToken,
  ): Promise<void> {
    await this.state.blockConcurrencyWhile(async () => {
      await this.state.storage.transaction(async (txn) => {
        const current = await txn.get<State>(stateKey);
        if (sameHead(current, head) && !current!.head!.originalAck) {
          current!.head!.originalAck = originalAck;
          await txn.put(stateKey, current!);
        }
      });
    });
  }
  private async terminal(
    head: Head,
    terminal: State["terminal"],
    now: number,
  ): Promise<string> {
    let won = false;
    await this.state.blockConcurrencyWhile(async () => {
      await this.state.storage.transaction(async (txn) => {
        const current = await txn.get<State>(stateKey);
        if (
          !sameHead(current, head) ||
          (terminal === "ACKED" &&
            (now >= head.enqueuedAt + TICK_DEADLINE_MS ||
              Date.now() >= head.enqueuedAt + TICK_DEADLINE_MS))
        )
          return;
        current!.head = undefined;
        current!.terminal = terminal;
        await txn.put(stateKey, current!);
        await txn.deleteAlarm();
        won = true;
      });
    });
    return won ? `tick terminal: ${terminal}` : "tick head changed";
  }
  async alarm(): Promise<void> {
    const head = await this.state.blockConcurrencyWhile(async () => {
      const current = await this.state.storage.get<State>(stateKey);
      if (!current?.head) return undefined;
      const deadline = current.head.enqueuedAt + TICK_DEADLINE_MS;
      if (Date.now() < deadline) {
        await this.state.storage.setAlarm(deadline);
        return undefined;
      }
      return current.head;
    });
    if (head) await this.terminal(head, "TIMED_OUT", Date.now());
  }
}
