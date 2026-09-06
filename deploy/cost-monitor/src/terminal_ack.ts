import { createHash } from "node:crypto";
import {
  type AckFields,
  type AckToken,
  type AsyncSigner,
  type PublicSigningIdentity,
  canonicalAckPayload,
  verifyAck,
  verifyOrderedFields,
} from "./acks.js";

export const TERMINAL_VERSION = "1" as const;
const TERMINAL_FIELDS = [
  "terminal_version", "terminal", "ack", "terminal_at", "signer_key_id", "signer_epoch",
] as const;
type HistoricalUnsigned = {
  terminal_version: "1";
  terminal: "HISTORICAL_NO_STATE";
  ack: AckToken;
  terminal_at: number;
  signer_key_id: string;
  signer_epoch: string;
};
export interface HistoricalTerminal extends HistoricalUnsigned { signature: string }

const object = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 256;
const safePositive = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value > 0;
const exact = (value: Record<string, unknown>, fields: readonly string[]) =>
  Object.keys(value).length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field));
const digest = (value: Uint8Array): string => createHash("sha256").update(value).digest("hex");

function ackDigest(ack: AckToken): string {
  const unsigned = Object.fromEntries(Object.entries(ack).filter(([key]) => key !== "signature")) as unknown as AckFields;
  return digest(new TextEncoder().encode(JSON.stringify([
    ...JSON.parse(new TextDecoder().decode(canonicalAckPayload(unsigned))),
    ack.signature,
  ])));
}

export function canonicalHistoricalTerminalPayload(value: HistoricalUnsigned): Uint8Array {
  if (!object(value) || value.terminal_version !== TERMINAL_VERSION || value.terminal !== "HISTORICAL_NO_STATE" ||
      !object(value.ack) || !safePositive(value.terminal_at) || !text(value.signer_key_id) || !text(value.signer_epoch))
    throw new TypeError("invalid historical terminal");
  return new TextEncoder().encode(JSON.stringify([
    value.terminal_version, value.terminal, ackDigest(value.ack), value.terminal_at,
    value.signer_key_id, value.signer_epoch,
  ]));
}

export async function createHistoricalTerminal(
  ack: AckToken,
  signer: AsyncSigner,
): Promise<HistoricalTerminal> {
  const expected = Object.fromEntries(Object.entries(ack).filter(([key]) => key !== "signature")) as unknown as AckFields;
  if (!verifyAck(ack, expected, signer.identity) || signer.identity.role !== "ingest-ack" ||
      ack.signer_key_id !== signer.identity.keyId || ack.signer_epoch !== signer.identity.epoch)
    throw new TypeError("invalid ACK or signer identity");
  const unsigned: HistoricalUnsigned = {
    terminal_version: TERMINAL_VERSION,
    terminal: "HISTORICAL_NO_STATE",
    ack,
    terminal_at: ack.committed_at,
    signer_key_id: signer.identity.keyId,
    signer_epoch: signer.identity.epoch,
  };
  const signature = await signer.sign(canonicalHistoricalTerminalPayload(unsigned));
  if (!/^[A-Za-z0-9_-]+$/.test(signature) || !verifyOrderedFields(canonicalHistoricalTerminalPayload(unsigned), signature, signer.identity))
    throw new Error("invalid signer output");
  return { ...unsigned, signature };
}

export function verifyHistoricalTerminal(
  value: unknown,
  expectedAckFields: AckFields,
  identity: PublicSigningIdentity,
): value is HistoricalTerminal {
  try {
    if (!object(value) || !exact(value, [...TERMINAL_FIELDS, "signature"]) ||
        value.terminal_version !== TERMINAL_VERSION || value.terminal !== "HISTORICAL_NO_STATE" ||
        !object(value.ack) || !safePositive(value.terminal_at) || !text(value.signer_key_id) ||
        !text(value.signer_epoch) || typeof value.signature !== "string" ||
        value.terminal_at !== value.ack.committed_at || value.signer_key_id !== identity.keyId ||
        value.signer_epoch !== identity.epoch || !verifyAck(value.ack, expectedAckFields, identity)) return false;
    return verifyOrderedFields(canonicalHistoricalTerminalPayload(value as unknown as HistoricalUnsigned), value.signature, identity);
  } catch { return false; }
}
