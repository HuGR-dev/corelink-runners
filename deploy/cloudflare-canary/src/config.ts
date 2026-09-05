/** Strict containment configuration.  Only the literal strings 0 and 1 are
 * valid; callers must make invalid configuration visible rather than silently
 * treating it as a healthy disabled state. */
export type ProbeFlag =
  | { valid: true; enabled: boolean }
  | { valid: false; reason: string };

export function parseProbeFlag(value: string | undefined): ProbeFlag {
  if (value === "0") return { valid: true, enabled: false };
  if (value === "1") return { valid: true, enabled: true };
  return { valid: false, reason: "FABRIC_PROBES_ENABLED must be exactly string 0 or 1" };
}

export interface TickConfig {
  ingestUrl: string;
  source: string;
  service: string;
  application: string;
  keyId: string;
  credentialEpoch: string;
  monitorRearmTupleDigest: string;
  envelopeHmacKey: string;
  ackHmacKey: string;
}

/** There are intentionally no endpoint, identity, digest, or key defaults.
 * They are supplied only by the later monitor/owner binding. */
export function tickConfig(env: {
  CANARY_TICK_INGEST_URL?: string; CANARY_TICK_SOURCE?: string; CANARY_TICK_SERVICE?: string;
  CANARY_TICK_APPLICATION?: string; CANARY_TICK_KEY_ID?: string; CANARY_TICK_CREDENTIAL_EPOCH?: string;
  CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST?: string; CANARY_TICK_ENVELOPE_HMAC_KEY?: string; CANARY_TICK_ACK_HMAC_KEY?: string;
}): TickConfig | null {
  const fields = [
    "CANARY_TICK_INGEST_URL", "CANARY_TICK_SOURCE", "CANARY_TICK_SERVICE",
    "CANARY_TICK_APPLICATION", "CANARY_TICK_KEY_ID", "CANARY_TICK_CREDENTIAL_EPOCH",
    "CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST", "CANARY_TICK_ENVELOPE_HMAC_KEY",
    "CANARY_TICK_ACK_HMAC_KEY",
  ] as const;
  if (fields.some((field) => !env[field])) return null;
  return {
    ingestUrl: env.CANARY_TICK_INGEST_URL!, source: env.CANARY_TICK_SOURCE!,
    service: env.CANARY_TICK_SERVICE!, application: env.CANARY_TICK_APPLICATION!,
    keyId: env.CANARY_TICK_KEY_ID!, credentialEpoch: env.CANARY_TICK_CREDENTIAL_EPOCH!,
    monitorRearmTupleDigest: env.CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST!,
    envelopeHmacKey: env.CANARY_TICK_ENVELOPE_HMAC_KEY!, ackHmacKey: env.CANARY_TICK_ACK_HMAC_KEY!,
  };
}
