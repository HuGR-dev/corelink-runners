import { createHash, randomBytes } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

export interface TrustedTimeProof {
  timeMs: number;
  proofDigest: string;
  requestDigest: string;
  authority: string;
}

export interface TrustedClock {
  now(): Promise<TrustedTimeProof>;
}

export type TrustedTimeErrorCode = "UNKNOWN" | "UNAUTHORIZED" | "TIMEOUT" | "INVALID";

export class TrustedTimeError extends Error {
  readonly code: TrustedTimeErrorCode;

  constructor(code: TrustedTimeErrorCode, message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "TrustedTimeError";
    this.code = code;
  }
}

type FetchLike = typeof fetch;

export interface Rfc3161ClockOptions {
  endpoint: string;
  rootPem: string;
  intermediatePem: string;
  crlUrls: string[];
  timeoutMs: number;
  maxResponseBytes: number;
  opensslPath: string;
  minimumTimeMs: number;
  maxAdvanceMs: number;
  loadFloor: () => Promise<number>;
  commitFloor: (expected: number, next: number) => Promise<boolean>;
  /** Test-only transport seam. It cannot bypass query or cryptographic validation. */
  fetcher?: FetchLike;
}

interface ProcessResult {
  stdout: Buffer;
  stderr: Buffer;
}

const MAX_TIMEOUT_MS = 5_000;
const MAX_RESPONSE_BYTES = 256 * 1024;
const MAX_FLOOR_RETRIES = 3;
const MAX_PROCESS_OUTPUT_BYTES = 64 * 1024;

function sha256(value: Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

function run(program: string, args: string[], timeoutMs: number): Promise<ProcessResult> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) return Promise.reject(new TrustedTimeError("TIMEOUT", "timestamp operation exceeded its budget"));
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { shell: false, stdio: ["ignore", "pipe", "pipe"] });
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    let outSize = 0;
    let errSize = 0;
    let settled = false;
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      finish(new TrustedTimeError("TIMEOUT", "openssl operation timed out"));
    }, timeoutMs);
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) reject(error);
      else resolve({ stdout: Buffer.concat(out), stderr: Buffer.concat(err) });
    };
    child.stdout.on("data", (chunk: Buffer) => {
      outSize += chunk.length;
      if (outSize > MAX_PROCESS_OUTPUT_BYTES) {
        child.kill("SIGKILL");
        finish(new TrustedTimeError("INVALID", "openssl stdout exceeds configured bound"));
      } else out.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => {
      errSize += chunk.length;
      if (errSize > MAX_PROCESS_OUTPUT_BYTES) {
        child.kill("SIGKILL");
        finish(new TrustedTimeError("INVALID", "openssl stderr exceeds configured bound"));
      } else err.push(chunk);
    });
    child.on("error", (error) => finish(new TrustedTimeError("UNKNOWN", "openssl could not start", { cause: error })));
    child.on("close", (code) => {
      if (code !== 0) finish(new TrustedTimeError("UNAUTHORIZED", `openssl rejected timestamp: ${Buffer.concat(err).toString("utf8").trim()}`));
      else finish();
    });
  });
}

function parseGenTime(text: string): number {
  const match = /Time stamp:\s*([^\r\n]+)/.exec(text);
  if (!match) throw new TrustedTimeError("UNKNOWN", "timestamp response has no GenTime");
  const parsed = Date.parse(match[1].trim());
  if (!Number.isSafeInteger(parsed)) throw new TrustedTimeError("UNKNOWN", "timestamp GenTime is not a safe integer");
  return parsed;
}

function parseCrlDates(text: string): { thisUpdate: number; nextUpdate: number } {
  const thisMatch = /Last Update:\s*([^\r\n]+)/.exec(text);
  const nextMatch = /Next Update:\s*([^\r\n]+)/.exec(text);
  if (!thisMatch || !nextMatch) throw new TrustedTimeError("UNAUTHORIZED", "CRL has no bounded validity interval");
  const thisUpdate = Date.parse(thisMatch[1].trim());
  const nextUpdate = Date.parse(nextMatch[1].trim());
  if (!Number.isFinite(thisUpdate) || !Number.isFinite(nextUpdate) || thisUpdate >= nextUpdate) {
    throw new TrustedTimeError("UNAUTHORIZED", "CRL validity interval is invalid");
  }
  return { thisUpdate, nextUpdate };
}

async function readLimited(response: Response, maxBytes: number): Promise<Buffer> {
  if (!response.body) {
    throw new TrustedTimeError("INVALID", "timestamp response has no bounded body stream");
  }
  const reader = response.body.getReader();
  const chunks: Buffer[] = [];
  let total = 0;
  try {
    for (;;) {
      const item = await reader.read();
      if (item.done) break;
      total += item.value.byteLength;
      if (total > maxBytes) throw new TrustedTimeError("INVALID", "timestamp response exceeds configured bound");
      chunks.push(Buffer.from(item.value));
    }
  } finally {
    reader.releaseLock();
  }
  return Buffer.concat(chunks);
}

export class Rfc3161Clock implements TrustedClock {
  private readonly options: Rfc3161ClockOptions;

  constructor(options: Rfc3161ClockOptions) {
    let endpoint: URL;
    try { endpoint = new URL(options.endpoint); } catch { throw new TrustedTimeError("INVALID", "RFC 3161 endpoint is not a URL"); }
    if (endpoint.protocol !== "http:" && endpoint.protocol !== "https:") throw new TrustedTimeError("INVALID", "RFC 3161 endpoint must use HTTP(S)");
    if (!endpoint.hostname || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) throw new TrustedTimeError("INVALID", "RFC 3161 endpoint must not contain credentials, query, or fragment");
    if (!options.rootPem.trim() || !options.intermediatePem.trim()) throw new TrustedTimeError("INVALID", "pinned certificate roots are required");
    if (!Number.isInteger(options.timeoutMs) || options.timeoutMs <= 0 || options.timeoutMs > MAX_TIMEOUT_MS) {
      throw new TrustedTimeError("INVALID", "timeoutMs must be an integer between 1 and 5000");
    }
    if (!Number.isInteger(options.maxResponseBytes) || options.maxResponseBytes <= 0 || options.maxResponseBytes > MAX_RESPONSE_BYTES) {
      throw new TrustedTimeError("INVALID", "maxResponseBytes exceeds 256 KiB bound");
    }
    if (options.crlUrls.length === 0) throw new TrustedTimeError("INVALID", "at least one CRL URL is required");
    if (!Number.isSafeInteger(options.minimumTimeMs) || !Number.isSafeInteger(options.maxAdvanceMs) || options.maxAdvanceMs < 0) {
      throw new TrustedTimeError("INVALID", "time bounds must be safe integers");
    }
    this.options = { ...options, endpoint: endpoint.toString() };
  }

  async now(): Promise<TrustedTimeProof> {
    const started = process.hrtime.bigint();
    const remaining = () => {
      const elapsed = Number(process.hrtime.bigint() - started) / 1_000_000;
      const left = this.options.timeoutMs - Math.floor(elapsed);
      if (left <= 0) throw new TrustedTimeError("TIMEOUT", "timestamp operation exceeded its budget");
      return left;
    };
    const budget = async <T>(operation: Promise<T>): Promise<T> => {
      const ms = remaining();
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeout = new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new TrustedTimeError("TIMEOUT", "timestamp operation exceeded its budget")), ms);
      });
      try { return await Promise.race([operation, timeout]); }
      finally { if (timer) clearTimeout(timer); }
    };
    const dir = await mkdtemp(join(tmpdir(), "corelink-tsa-"));
    const requestPath = join(dir, "request.tsq");
      const responsePath = join(dir, "response.tsr");
    const rootPath = join(dir, "root.pem");
    const intermediatePath = join(dir, "intermediate.pem");
    try {
      const challenge = randomBytes(32);
      const challengePath = join(dir, "challenge.bin");
      await writeFile(challengePath, challenge, { mode: 0o600 });
      await writeFile(rootPath, this.options.rootPem, { mode: 0o600 });
      await writeFile(intermediatePath, this.options.intermediatePem, { mode: 0o600 });
      await run(this.options.opensslPath, ["ts", "-query", "-data", challengePath, "-sha256", "-cert", "-out", requestPath], remaining());
      const request = await readFile(requestPath);
      const fetcher = this.options.fetcher ?? fetch;
      const response = await budget(fetcher(this.options.endpoint, {
        method: "POST",
        redirect: "error",
        headers: { "content-type": "application/timestamp-query", accept: "application/timestamp-reply" },
        body: request,
        signal: AbortSignal.timeout(remaining()),
      }));
      if (!response.ok) throw new TrustedTimeError("UNKNOWN", `timestamp authority returned HTTP ${response.status}`);
      const reply = await readLimited(response, this.options.maxResponseBytes);
      await writeFile(responsePath, reply, { mode: 0o600 });
      const textResult = await run(this.options.opensslPath, ["ts", "-reply", "-in", responsePath, "-text"], remaining());
      const genTime = parseGenTime(textResult.stdout.toString("utf8"));
      const crlPaths: string[] = [];
      for (let i = 0; i < this.options.crlUrls.length; i += 1) {
        const crlResponse = await budget(fetcher(this.options.crlUrls[i], { method: "GET", redirect: "error", signal: AbortSignal.timeout(remaining()) }));
        if (!crlResponse.ok) throw new TrustedTimeError("UNAUTHORIZED", `CRL authority returned HTTP ${crlResponse.status}`);
        const crl = await readLimited(crlResponse, MAX_RESPONSE_BYTES);
        const rawCrlPath = join(dir, `crl-${i}.der`);
        const crlPath = join(dir, `crl-${i}.pem`);
        await writeFile(rawCrlPath, crl, { mode: 0o600 });
        try {
          await run(this.options.opensslPath, ["crl", "-inform", "DER", "-in", rawCrlPath, "-outform", "PEM", "-out", crlPath], remaining());
        } catch {
          await run(this.options.opensslPath, ["crl", "-inform", "PEM", "-in", rawCrlPath, "-outform", "PEM", "-out", crlPath], remaining());
        }
        const crlText = await run(this.options.opensslPath, ["crl", "-in", crlPath, "-text", "-noout"], remaining());
        const dates = parseCrlDates(crlText.stdout.toString("utf8"));
        if (genTime < dates.thisUpdate || genTime >= dates.nextUpdate) throw new TrustedTimeError("UNAUTHORIZED", "GenTime is outside CRL validity");
        try {
          await run(this.options.opensslPath, ["crl", "-in", crlPath, "-CAfile", intermediatePath, "-verify", "-noout"], remaining());
        } catch {
          await run(this.options.opensslPath, ["crl", "-in", crlPath, "-CAfile", rootPath, "-verify", "-noout"], remaining());
        }
        crlPaths.push(crlPath);
      }
      await run(this.options.opensslPath, ["ts", "-verify", "-in", responsePath, "-queryfile", requestPath, "-CAfile", rootPath, "-untrusted", intermediatePath, "-attime", String(Math.floor(genTime / 1000))], remaining());
      const tokenPath = join(dir, "token.der");
      const certificatesPath = join(dir, "certificates.pem");
      await run(this.options.opensslPath, ["ts", "-reply", "-in", responsePath, "-token_out", "-out", tokenPath], remaining());
      await run(this.options.opensslPath, ["pkcs7", "-inform", "DER", "-in", tokenPath, "-print_certs", "-out", certificatesPath], remaining());
      const certificates = (await readFile(certificatesPath, "utf8")).match(/-----BEGIN CERTIFICATE-----[\s\S]*?-----END CERTIFICATE-----/g) ?? [];
      let signerPath: string | undefined;
      for (let i = 0; i < certificates.length; i += 1) {
        const certificatePath = join(dir, `certificate-${i}.pem`);
        await writeFile(certificatePath, certificates[i], { mode: 0o600 });
        const purpose = await run(this.options.opensslPath, ["x509", "-in", certificatePath, "-purpose", "-noout"], remaining());
        if (/^Time Stamp signing\s*:\s*Yes$/m.test(purpose.stdout.toString("utf8"))) signerPath = certificatePath;
      }
      if (!signerPath) throw new TrustedTimeError("UNAUTHORIZED", "timestamp signer lacks the Time Stamping EKU");
      const allCrlPath = join(dir, "all-crls.pem");
      const allCrls = Buffer.concat(await Promise.all(crlPaths.map((path) => readFile(path))));
      await writeFile(allCrlPath, allCrls, { mode: 0o600 });
      const leafVerify = ["verify", "-CAfile", rootPath, "-untrusted", intermediatePath, "-attime", String(Math.floor(genTime / 1000)), "-crl_check_all", "-CRLfile", allCrlPath];
      leafVerify.push(signerPath);
      await run(this.options.opensslPath, leafVerify, remaining());
      if (genTime < this.options.minimumTimeMs) throw new TrustedTimeError("UNKNOWN", "timestamp is below configured minimum");
      await this.advanceFloor(genTime, remaining, budget);
      return { timeMs: genTime, proofDigest: sha256(reply), requestDigest: sha256(request), authority: this.options.endpoint };
    } catch (error) {
      if (error instanceof TrustedTimeError) throw error;
      throw new TrustedTimeError("UNKNOWN", "RFC 3161 timestamp failed", { cause: error });
    } finally {
      try { await rm(dir, { recursive: true, force: true }); } catch { /* preserve the primary result or error */ }
    }
  }

  private async advanceFloor(timeMs: number, remaining: () => number, budget: <T>(operation: Promise<T>) => Promise<T>): Promise<number> {
    for (let attempt = 0; attempt < MAX_FLOOR_RETRIES; attempt += 1) {
      const floor = await budget(this.options.loadFloor());
      if (!Number.isSafeInteger(floor)) throw new TrustedTimeError("UNKNOWN", "persisted time floor is invalid");
      if (timeMs < floor || this.options.maxAdvanceMs > Number.MAX_SAFE_INTEGER - floor || timeMs > floor + this.options.maxAdvanceMs) throw new TrustedTimeError("UNKNOWN", "timestamp violates persisted time floor");
      if (await budget(this.options.commitFloor(floor, timeMs))) return timeMs;
    }
    throw new TrustedTimeError("UNKNOWN", "time floor changed during validation");
  }
}
