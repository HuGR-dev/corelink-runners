import { InvokeCommand, LambdaClient } from "@aws-sdk/client-lambda";
import { GetSecretValueCommand, SecretsManagerClient } from "@aws-sdk/client-secrets-manager";
import { performance } from "node:perf_hooks";
import { canonicalCheckpointBytes, canonicalWitnessBytes, checkpointRootFor, type SignedCheckpoint, type WitnessReceipt } from "./evidence_log.js";
import { verifyOrderedFields, type PublicSigningIdentity } from "./acks.js";
import type { SourceRegistration } from "./types.js";
import type { CurrentCheckpointWitness, SignedWitnessHead } from "./witness.js";

const HEX = /^[0-9a-f]{64}$/;
const NONCE = /^[0-9a-f]{64}$/;
const ZERO = "0".repeat(64);
const SECRET_FIELDS = ["version", "source", "service", "application", "key_id", "credential_epoch", "hmac_key_base64url"] as const;
const MAX_SECRET_BYTES = 4096;

export class AwsAdapterError extends Error {
  readonly code: "INVALID" | "TIMEOUT" | "UNAVAILABLE";
  constructor(code: AwsAdapterError["code"], message: string) { super(message); this.name = "AwsAdapterError"; this.code = code; }
}

function text(value: unknown, max = 256): value is string { return typeof value === "string" && value.length > 0 && value.length <= max && !/[\u0000-\u001f\u007f]/.test(value); }
function positive(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value > 0; }
function object(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
function exact(value: Record<string, unknown>, fields: readonly string[]): boolean { const keys = Object.keys(value); return keys.length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field)); }
function sanitized(code: AwsAdapterError["code"] = "UNAVAILABLE"): AwsAdapterError { return new AwsAdapterError(code, "AWS adapter operation failed"); }
function ensureDeadline(started: number, timeoutMs: number): void { if (performance.now() - started >= timeoutMs) throw sanitized("TIMEOUT"); }
function accountFromArn(arn: string): string | null { return /^arn:aws:[^:]+:[^:]*:(\d{12}):/.exec(arn)?.[1] ?? null; }
function qualifiedLambda(arn: string): { region: string; account: string; version: string } | null {
  const match = /^arn:aws:lambda:([a-z0-9-]+):(\d{12}):function:[A-Za-z0-9_-]{1,140}:([1-9]\d*)$/.exec(arn);
  return match ? { region: match[1], account: match[2], version: match[3] } : null;
}
function validateIdentity(identity: PublicSigningIdentity, role: "journal" | "witness"): void {
  if (!object(identity) || identity.role !== role || !text(identity.keyId) || !text(identity.epoch) || !text(identity.keyArn) || typeof identity.publicKeySpkiPem !== "string" || identity.publicKeySpkiPem.length < 1) throw sanitized("INVALID");
}
function validateCheckpoint(value: unknown, logId: string, identity: PublicSigningIdentity): value is SignedCheckpoint {
  if (!object(value) || !exact(value, ["version", "logId", "sequence", "previousRoot", "recordDigest", "operationId", "trustedAtMs", "signerKeyId", "signerEpoch", "signature"])) return false;
  const x = value as unknown as SignedCheckpoint;
  return x.version === "1" && x.logId === logId && positive(x.sequence) && HEX.test(x.previousRoot) && HEX.test(x.recordDigest) && text(x.operationId) && positive(x.trustedAtMs) && x.signerKeyId === identity.keyId && x.signerEpoch === identity.epoch && text(x.signature, 8192) && verifyOrderedFields(canonicalCheckpointBytes(x), x.signature, identity);
}
function validateReceipt(value: unknown, checkpoint: SignedCheckpoint, checkpointRoot: string, identity: PublicSigningIdentity): value is WitnessReceipt {
  if (!object(value) || !exact(value, ["version", "logId", "sequence", "checkpointRoot", "previousWitnessRoot", "checkpointSignerKeyId", "checkpointSignerEpoch", "witnessKeyId", "witnessEpoch", "trustedAtMs", "signature"])) return false;
  const x = value as unknown as WitnessReceipt;
  return x.version === "1" && x.logId === checkpoint.logId && x.sequence === checkpoint.sequence && x.checkpointRoot === checkpointRoot && HEX.test(x.previousWitnessRoot) && x.checkpointSignerKeyId === checkpoint.signerKeyId && x.checkpointSignerEpoch === checkpoint.signerEpoch && x.witnessKeyId === identity.keyId && x.witnessEpoch === identity.epoch && positive(x.trustedAtMs) && text(x.signature, 8192) && verifyOrderedFields(canonicalWitnessBytes(x), x.signature, identity);
}
function validateHead(value: unknown, logId: string, identity: PublicSigningIdentity, nonce: string): value is SignedWitnessHead {
  if (!object(value) || !exact(value, ["version", "logId", "nonce", "sequence", "checkpointRoot", "witnessRoot", "trustedAtMs", "signerKeyId", "signerEpoch", "signature"])) return false;
  const x = value as unknown as SignedWitnessHead;
  if (x.version !== "1" || x.logId !== logId || x.nonce !== nonce || typeof x.sequence !== "number" || !Number.isSafeInteger(x.sequence) || x.sequence < 0 || !HEX.test(x.checkpointRoot) || !HEX.test(x.witnessRoot) || !positive(x.trustedAtMs) || x.signerKeyId !== identity.keyId || x.signerEpoch !== identity.epoch || !text(x.signature, 8192)) return false;
  if (x.sequence === 0 && (x.checkpointRoot !== ZERO || x.witnessRoot !== ZERO)) return false;
  if (x.sequence > 0 && (x.checkpointRoot === ZERO || x.witnessRoot === ZERO)) return false;
  return verifyOrderedFields(new TextEncoder().encode(JSON.stringify([x.version, x.logId, x.nonce, x.sequence, x.checkpointRoot, x.witnessRoot, x.trustedAtMs, x.signerKeyId, x.signerEpoch])), x.signature, identity);
}

async function deadline<T>(timeoutMs: number, operation: (signal: AbortSignal) => Promise<T>): Promise<T> {
  const started = performance.now(); const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => { timer = setTimeout(() => { controller.abort(); reject(sanitized("TIMEOUT")); }, timeoutMs); });
  try { return await Promise.race([operation(controller.signal), timeout]); }
  catch (error) { if (error instanceof AwsAdapterError) throw error; throw sanitized(performance.now() - started >= timeoutMs ? "TIMEOUT" : "UNAVAILABLE"); }
  finally { if (timer) clearTimeout(timer); controller.abort(); }
}

export class AwsSourceSecrets {
  private readonly client: SecretsManagerClient;
  private readonly timeoutMs: number;
  constructor(options: { client: SecretsManagerClient; timeoutMs: number }) {
    if (!options.client || !Number.isSafeInteger(options.timeoutMs) || options.timeoutMs <= 0 || options.timeoutMs > 5000) throw sanitized("INVALID");
    this.client = options.client; this.timeoutMs = options.timeoutMs;
  }
  async load(registration: SourceRegistration): Promise<Uint8Array> {
    const secretArn = registration?.secretArn; const secretVersionId = registration?.secretVersionId;
    if (!object(registration) || !text(secretArn) || !text(secretVersionId) || !text(registration.source) || !text(registration.service) || !text(registration.application) || !text(registration.keyId) || !text(registration.credentialEpoch)) throw sanitized("INVALID");
    const started = performance.now();
    const response = await deadline(this.timeoutMs, (signal) => this.client.send(new GetSecretValueCommand({ SecretId: secretArn, VersionId: secretVersionId }), { abortSignal: signal }));
    ensureDeadline(started, this.timeoutMs);
    if (response.ARN !== secretArn || response.VersionId !== secretVersionId || typeof response.SecretString !== "string" || response.SecretString.length === 0 || new TextEncoder().encode(response.SecretString).byteLength > MAX_SECRET_BYTES) throw sanitized("INVALID");
    let parsed: unknown; try { parsed = JSON.parse(response.SecretString); } catch { throw sanitized("INVALID"); }
    if (!object(parsed) || !exact(parsed, SECRET_FIELDS) || parsed.version !== "1" || parsed.source !== registration.source || parsed.service !== registration.service || parsed.application !== registration.application || parsed.key_id !== registration.keyId || parsed.credential_epoch !== registration.credentialEpoch || typeof parsed.hmac_key_base64url !== "string" || !/^[A-Za-z0-9_-]+$/.test(parsed.hmac_key_base64url)) throw sanitized("INVALID");
    let decoded: Buffer; try { decoded = Buffer.from(parsed.hmac_key_base64url, "base64url"); } catch { throw sanitized("INVALID"); }
    if (decoded.length < 32 || decoded.length > 1024 || decoded.toString("base64url") !== parsed.hmac_key_base64url) throw sanitized("INVALID");
    ensureDeadline(started, this.timeoutMs);
    return new Uint8Array(decoded);
  }
}

export class LambdaWitnessClient implements CurrentCheckpointWitness {
  private readonly client: LambdaClient;
  private readonly functionArn: string;
  private readonly version: string;
  private readonly logId: string;
  private readonly journalIdentity: PublicSigningIdentity;
  private readonly witnessIdentity: PublicSigningIdentity;
  private readonly timeoutMs: number;
  private readonly maxResponseBytes: number;
  constructor(options: { client: LambdaClient; functionArn: string; logId: string; journalIdentity: PublicSigningIdentity; witnessIdentity: PublicSigningIdentity; timeoutMs: number; maxResponseBytes: number }) {
    const qualified = qualifiedLambda(options.functionArn);
    validateIdentity(options.journalIdentity, "journal"); validateIdentity(options.witnessIdentity, "witness");
    const journalAccount = accountFromArn(options.journalIdentity.keyArn); const witnessAccount = accountFromArn(options.witnessIdentity.keyArn);
    if (!options.client || !qualified || qualified.account !== witnessAccount || journalAccount === witnessAccount || options.journalIdentity.keyId === options.witnessIdentity.keyId || !text(options.logId) || !Number.isSafeInteger(options.timeoutMs) || options.timeoutMs <= 0 || options.timeoutMs > 5000 || !Number.isSafeInteger(options.maxResponseBytes) || options.maxResponseBytes <= 0 || options.maxResponseBytes > 262144) throw sanitized("INVALID");
    this.client = options.client; this.functionArn = options.functionArn; this.version = qualified.version; this.logId = options.logId; this.journalIdentity = options.journalIdentity; this.witnessIdentity = options.witnessIdentity; this.timeoutMs = options.timeoutMs; this.maxResponseBytes = options.maxResponseBytes;
  }
  private async regionMatches(): Promise<void> {
    const configured = await (this.client.config.region?.() ?? Promise.resolve(undefined));
    const qualified = qualifiedLambda(this.functionArn); if (configured !== undefined && configured !== qualified?.region) throw sanitized("INVALID");
  }
  private async invoke(payload: Record<string, unknown>): Promise<{ value: unknown; started: number }> {
    const started = performance.now();
    await this.regionMatches();
    const response = await deadline(this.timeoutMs, (signal) => this.client.send(new InvokeCommand({ FunctionName: this.functionArn, InvocationType: "RequestResponse", Payload: new TextEncoder().encode(JSON.stringify(payload)) }), { abortSignal: signal }));
    if (response.StatusCode !== 200 || response.FunctionError || response.ExecutedVersion !== this.version || !(response.Payload instanceof Uint8Array) || response.Payload.byteLength === 0 || response.Payload.byteLength > this.maxResponseBytes) throw sanitized("UNAVAILABLE");
    let parsed: unknown; try { parsed = JSON.parse(new TextDecoder().decode(response.Payload)); } catch { throw sanitized("INVALID"); }
    if (!object(parsed)) throw sanitized("INVALID");
    if (performance.now() - started >= this.timeoutMs) throw sanitized("TIMEOUT");
    return { value: parsed, started };
  }
  async accept(checkpoint: SignedCheckpoint): Promise<WitnessReceipt> {
    if (!validateCheckpoint(checkpoint, this.logId, this.journalIdentity)) throw sanitized("INVALID");
    const invocation = await this.invoke({ action: "accept", checkpoint });
    if (!validateReceipt(invocation.value, checkpoint, checkpointRootFor(checkpoint), this.witnessIdentity)) throw sanitized("INVALID");
    if (performance.now() - invocation.started >= this.timeoutMs) throw sanitized("TIMEOUT");
    return invocation.value;
  }
  async readHead(nonce: string): Promise<SignedWitnessHead> {
    if (!NONCE.test(nonce)) throw sanitized("INVALID");
    const invocation = await this.invoke({ action: "head", nonce, logId: this.logId });
    if (!validateHead(invocation.value, this.logId, this.witnessIdentity, nonce)) throw sanitized("INVALID");
    if (performance.now() - invocation.started >= this.timeoutMs) throw sanitized("TIMEOUT");
    return invocation.value;
  }
}
