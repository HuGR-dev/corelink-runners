import { DynamoDBClient } from "@aws-sdk/client-dynamodb";
import { KMSClient } from "@aws-sdk/client-kms";
import { S3Client } from "@aws-sdk/client-s3";
import { AwsKmsSigner, type PublicSigningIdentity } from "./acks.js";
import { DurableCheckpointWitness, type CurrentCheckpointWitness } from "./witness.js";
import { DurableTimeFloor } from "./durable_time_floor.js";
import { DynamoDbStateStore } from "./state.js";
import { S3ImmutableJournal } from "./journal.js";
import { Rfc3161Clock, type TrustedClock } from "./trusted_time.js";
import type { SignedCheckpoint } from "./evidence_log.js";

const ARN = /^arn:aws:kms:([^:]+):(\d{12}):key\/.+$/;
const LAMBDA = /^arn:aws:lambda:([^:]+):(\d{12}):function:[^:]+:([1-9][0-9]*)$/;
const POSITIVE = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
const TEXT = (v: unknown, max = 512): v is string => typeof v === "string" && v.length > 0 && v.length <= max && v.trim() === v;
const ACCOUNT = (v: unknown): v is string => typeof v === "string" && /^\d{12}$/.test(v);
const keysExact = (value: Record<string, unknown>, keys: readonly string[]) => Object.keys(value).length === keys.length && keys.every((key) => Object.prototype.hasOwnProperty.call(value, key));

export interface TrustedTimeConfig { endpoint: string; rootPem: string; intermediatePem: string; crlUrls: string[]; minimumTimeMs: number; maxAdvanceMs: number; timeoutMs: number; maxResponseBytes: number; opensslPath: string }
export interface WitnessConfig {
  version: "1"; region: string; monitorAccountId: string; verifierAccountId: string; stateTable: string; stateNamespace: string;
  journalBucket: string; journalPrefix: string; journalRetentionMs: number; allowedLogIds: string[];
  journalIdentity: PublicSigningIdentity; witnessIdentity: PublicSigningIdentity; trustedTime: TrustedTimeConfig;
}
export interface WitnessRuntimeDependencies { witnesses: ReadonlyMap<string, CurrentCheckpointWitness> }
export class LambdaFunctionError extends Error { override readonly name = "LambdaFunctionError"; constructor() { super("witness request failed"); } }

function identity(value: unknown, role: "journal" | "witness"): value is PublicSigningIdentity {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const x = value as Record<string, unknown>;
  return keysExact(x, ["keyId", "epoch", "keyArn", "publicKeySpkiPem", "role"]) && TEXT(x.keyId, 256) && TEXT(x.epoch, 256) && TEXT(x.keyArn, 512) && TEXT(x.publicKeySpkiPem, 8192) && x.role === role;
}
function validateArn(identityValue: PublicSigningIdentity, region: string, account: string): boolean {
  const match = ARN.exec(identityValue.keyArn); return !!match && match[1] === region && match[2] === account;
}
function validateTrustedTime(value: unknown): value is TrustedTimeConfig {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const x = value as Record<string, unknown>;
  return keysExact(x, ["endpoint", "rootPem", "intermediatePem", "crlUrls", "minimumTimeMs", "maxAdvanceMs", "timeoutMs", "maxResponseBytes", "opensslPath"]) &&
    x.endpoint === "https://timestamp.digicert.com" && TEXT(x.rootPem, 16384) && TEXT(x.intermediatePem, 16384) &&
    Array.isArray(x.crlUrls) && x.crlUrls.length === 2 && x.crlUrls.every((url) => typeof url === "string" && /^https:\/\//.test(url) && url.length <= 2048) &&
    POSITIVE(x.minimumTimeMs) && POSITIVE(x.maxAdvanceMs) && POSITIVE(x.timeoutMs) && x.timeoutMs <= 5000 && POSITIVE(x.maxResponseBytes) && x.maxResponseBytes <= 262144 && TEXT(x.opensslPath, 512);
}
export function validateWitnessConfig(input: unknown): WitnessConfig {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new TypeError("invalid witness config");
  const x = input as Record<string, unknown>;
  const fields = ["version", "region", "monitorAccountId", "verifierAccountId", "stateTable", "stateNamespace", "journalBucket", "journalPrefix", "journalRetentionMs", "allowedLogIds", "journalIdentity", "witnessIdentity", "trustedTime"] as const;
  if (!keysExact(x, fields) || x.version !== "1" || !TEXT(x.region, 64) || !ACCOUNT(x.monitorAccountId) || !ACCOUNT(x.verifierAccountId) || x.monitorAccountId === x.verifierAccountId ||
    !TEXT(x.stateTable) || !TEXT(x.stateNamespace) || !TEXT(x.journalBucket) || !TEXT(x.journalPrefix) || !POSITIVE(x.journalRetentionMs) || x.journalRetentionMs < 8 * 24 * 60 * 60 * 1000 ||
    !Array.isArray(x.allowedLogIds) || x.allowedLogIds.length === 0 || x.allowedLogIds.length > 100 || !x.allowedLogIds.every((id) => TEXT(id, 256)) || new Set(x.allowedLogIds).size !== x.allowedLogIds.length ||
    !identity(x.journalIdentity, "journal") || !identity(x.witnessIdentity, "witness") || !validateTrustedTime(x.trustedTime)) throw new TypeError("invalid witness config");
  const journalIdentity = x.journalIdentity as PublicSigningIdentity; const witnessIdentity = x.witnessIdentity as PublicSigningIdentity;
  if (!validateArn(journalIdentity, x.region as string, x.monitorAccountId as string) || !validateArn(witnessIdentity, x.region as string, x.verifierAccountId as string) || journalIdentity.keyArn === witnessIdentity.keyArn) throw new TypeError("invalid witness identity domain");
  return structuredClone(x) as unknown as WitnessConfig;
}

function productionWitnesses(config: WitnessConfig): ReadonlyMap<string, CurrentCheckpointWitness> {
  const ddb = new DynamoDBClient({ region: config.region }); const s3 = new S3Client({ region: config.region }); const kms = new KMSClient({ region: config.region });
  const witnesses = new Map<string, CurrentCheckpointWitness>();
  for (const logId of config.allowedLogIds) {
    const store = new DynamoDbStateStore({ client: ddb, tableName: config.stateTable, namespace: `${config.stateNamespace}:witness:${logId}` });
    const floor = new DurableTimeFloor({ store, key: `${config.stateNamespace}:trusted-time-floor`, minimumTimeMs: config.trustedTime.minimumTimeMs });
    const clock: TrustedClock = new Rfc3161Clock({ ...config.trustedTime, loadFloor: () => floor.load(), commitFloor: (expected, next) => floor.commit(expected, next) });
    const journal = new S3ImmutableJournal({ client: s3, bucket: config.journalBucket, prefix: `${config.journalPrefix}/${logId}`, retentionMs: config.journalRetentionMs });
    const signer = new AwsKmsSigner({ client: kms, identity: config.witnessIdentity });
    witnesses.set(logId, new DurableCheckpointWitness({ store, journal, clock, signer, logId, namespace: `${config.stateNamespace}:witness:${logId}`, journalIdentity: config.journalIdentity }));
  }
  return witnesses;
}

function requestKeys(event: Record<string, unknown>, expected: readonly string[]): boolean { return keysExact(event, expected); }
function contextQualified(context: unknown, config: WitnessConfig): boolean {
  if (!context || typeof context !== "object") return false; const arn = (context as { invokedFunctionArn?: unknown }).invokedFunctionArn;
  const match = typeof arn === "string" ? LAMBDA.exec(arn) : null; return !!match && match[1] === config.region && match[2] === config.monitorAccountId;
}
function allowed(config: WitnessConfig, logId: string): boolean { return config.allowedLogIds.includes(logId); }

export function createWitnessHandler(config: WitnessConfig, dependencies?: WitnessRuntimeDependencies): (event: unknown, context: unknown) => Promise<unknown> {
  const witnessesPromise = Promise.resolve(dependencies?.witnesses ?? productionWitnesses(config));
  return async (event, context) => {
    try {
      if (!contextQualified(context, config) || !event || typeof event !== "object" || Array.isArray(event)) throw new LambdaFunctionError();
      const body = event as Record<string, unknown>; const witnesses = await witnessesPromise;
      if (body.action === "accept" && requestKeys(body, ["action", "checkpoint"])) {
        const checkpoint = body.checkpoint as SignedCheckpoint; if (!checkpoint || typeof checkpoint.logId !== "string" || !allowed(config, checkpoint.logId)) throw new LambdaFunctionError();
        const witness = witnesses.get(checkpoint.logId); if (!witness) throw new LambdaFunctionError(); return await witness.accept(checkpoint);
      }
      if (body.action === "head" && requestKeys(body, ["action", "nonce", "logId"]) && typeof body.nonce === "string" && body.nonce.length > 0 && typeof body.logId === "string" && allowed(config, body.logId)) {
        const witness = witnesses.get(body.logId); if (!witness) throw new LambdaFunctionError(); return await witness.readHead(body.nonce);
      }
      throw new LambdaFunctionError();
    } catch { throw new LambdaFunctionError(); }
  };
}

export async function handler(event: unknown, context: unknown): Promise<unknown> {
  try {
    const raw = process.env.WITNESS_CONFIG_JSON; if (typeof raw !== "string" || raw.length === 0) throw new LambdaFunctionError();
    return await createWitnessHandler(validateWitnessConfig(JSON.parse(raw)))(event, context);
  } catch { throw new LambdaFunctionError(); }
}
