import { execFile } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, expect, it, vi } from "vitest";
import { Rfc3161Clock, TrustedTimeError } from "../src/trusted_time.js";

const exec = promisify(execFile);
const fixture = join(dirname(fileURLToPath(import.meta.url)), "fixtures", "trusted-time", "pki");
const TSA_URL = "https://fixture.test/tsa";
const LEAF_CRL_URL = "https://fixture.test/leaf.crl";
const INTERMEDIATE_CRL_URL = "https://fixture.test/intermediate.crl";

async function fixtureText(path: string): Promise<string> { return readFile(join(fixture, path), "utf8"); }
async function fixtureBytes(path: string): Promise<Buffer> { return readFile(join(fixture, path)); }
async function openssl(args: string[]): Promise<void> { await exec("openssl", args, { maxBuffer: 128 * 1024 }); }

async function timestampReply(query: Buffer, signer = "tsa-good"): Promise<Buffer> {
  const dir = await mkdtemp(join(tmpdir(), "corelink-test-tsa-"));
  try {
    const queryPath = join(dir, "query.tsq"); const replyPath = join(dir, "reply.tsr"); const configPath = join(dir, "tsa.cnf");
    await writeFile(queryPath, query, { mode: 0o600 }); await writeFile(join(dir, "tsaserial"), "01\n", { mode: 0o600 });
    await writeFile(configPath, `[ tsa ]
default_tsa = tsa_config1
[ tsa_config1 ]
dir = ${dir}
serial = $dir/tsaserial
crypto_device = builtin
signer_cert = ${join(fixture, `${signer}.pem`)}
certs = ${join(fixture, "intermediate", "intermediate.pem")}
signer_key = ${join(fixture, `${signer}.key`)}
signer_digest = sha256
default_policy = 1.2.3.4.1
other_policies = 1.2.3.4.2
digests = sha256
accuracy = secs:1
ordering = yes
tsa_name = yes
ess_cert_id_chain = yes
ess_cert_id_alg = sha256
`, { mode: 0o600 });
    await openssl(["ts", "-reply", "-config", configPath, "-section", "tsa_config1", "-queryfile", queryPath, "-out", replyPath]);
    return readFile(replyPath);
  } finally { await rm(dir, { recursive: true, force: true }); }
}
async function unrelatedReply(): Promise<Buffer> {
  const dir = await mkdtemp(join(tmpdir(), "corelink-test-tsa-wrong-"));
  try { const data = join(dir, "other.bin"); const query = join(dir, "other.tsq"); await writeFile(data, "different RFC3161 imprint", { mode: 0o600 }); await openssl(["ts", "-query", "-data", data, "-sha256", "-cert", "-out", query]); return timestampReply(await readFile(query)); }
  finally { await rm(dir, { recursive: true, force: true }); }
}
type CrlSet = { leaf: string; intermediate: string };
function fetcherFor(crls: CrlSet, reply = timestampReply): typeof fetch {
  return async (input, init) => {
    const url = String(input);
    if (url === TSA_URL) { const body = init?.body; if (!(body instanceof Uint8Array)) throw new Error("RFC3161 request was not raw bytes"); return new Response(await reply(Buffer.from(body)), { status: 200 }); }
    if (url === LEAF_CRL_URL) return new Response(await fixtureBytes(crls.leaf), { status: 200 });
    if (url === INTERMEDIATE_CRL_URL) return new Response(await fixtureBytes(crls.intermediate), { status: 200 });
    throw new Error(`unexpected offline fixture URL: ${url}`);
  };
}
async function options(overrides: Partial<ConstructorParameters<typeof Rfc3161Clock>[0]> = {}) {
  return { endpoint: TSA_URL, rootPem: await fixtureText("root/root.pem"), intermediatePem: await fixtureText("intermediate/intermediate.pem"), crlUrls: [LEAF_CRL_URL, INTERMEDIATE_CRL_URL], timeoutMs: 5_000, maxResponseBytes: 64 * 1024, opensslPath: "openssl", minimumTimeMs: 0, maxAdvanceMs: Number.MAX_SAFE_INTEGER, loadFloor: async () => 0, commitFloor: async () => true, fetcher: fetcherFor({ leaf: "intermediate/leaf-valid.crl.pem", intermediate: "root/intermediate-valid.crl.pem" }), ...overrides };
}
async function corelinkTsaDirs(): Promise<string[]> { return (await readdir(tmpdir())).filter((name) => name.startsWith("corelink-tsa-")); }

describe("Rfc3161Clock offline RFC3161 PKI", () => {
  it("verifies a real query-bound TSA reply and returns signed proof fields", async () => {
    const proof = await new Rfc3161Clock(await options()).now();
    expect(Number.isSafeInteger(proof.timeMs)).toBe(true); expect(proof.timeMs).toBeGreaterThan(0); expect(proof.proofDigest).toMatch(/^[0-9a-f]{64}$/); expect(proof.requestDigest).toMatch(/^[0-9a-f]{64}$/); expect(proof.proofDigest).not.toBe(proof.requestDigest); expect(proof.authority).toBe(TSA_URL);
  }, 20_000);
  it("rejects a real TSA reply whose nonce and imprint bind a different query", async () => {
    const clock = new Rfc3161Clock(await options({ fetcher: fetcherFor({ leaf: "intermediate/leaf-valid.crl.pem", intermediate: "root/intermediate-valid.crl.pem" }, async () => unrelatedReply()) }));
    await expect(clock.now()).rejects.toMatchObject({ code: "UNAUTHORIZED" });
  }, 20_000);
  it.each([
    ["revoked TSA leaf", { leaf: "intermediate/leaf-revoked.crl.pem", intermediate: "root/intermediate-valid.crl.pem" }],
    ["revoked intermediate", { leaf: "intermediate/leaf-valid.crl.pem", intermediate: "root/intermediate-revoked.crl.pem" }],
    ["stale leaf CRL", { leaf: "intermediate/leaf-stale.crl.pem", intermediate: "root/intermediate-valid.crl.pem" }],
  ] as const)("rejects a %s using real signed CRLs", async (_label, crls) => {
    await expect(new Rfc3161Clock(await options({ fetcher: fetcherFor(crls) })).now()).rejects.toMatchObject({ code: "UNAUTHORIZED" });
  }, 20_000);
  it("rejects a chain whose pinned intermediate does not authenticate the TSA", async () => {
    await expect(new Rfc3161Clock(await options({ intermediatePem: await fixtureText("tsa-bad-eku.pem") })).now()).rejects.toMatchObject({ code: "UNAUTHORIZED" });
  }, 20_000);
  it("fails closed when OpenSSL refuses a wrong-EKU TSA signer", async () => {
    const clock = new Rfc3161Clock(await options({ fetcher: fetcherFor(
      { leaf: "intermediate/leaf-valid.crl.pem", intermediate: "root/intermediate-valid.crl.pem" },
      async (query) => timestampReply(query, "tsa-bad-eku"),
    ) }));
    await expect(clock.now()).rejects.toMatchObject({ code: "UNKNOWN" });
  }, 20_000);
  it("uses signed time for rollback, bounded advance, and CAS retry/failure", async () => {
    await expect(new Rfc3161Clock(await options({ loadFloor: async () => Number.MAX_SAFE_INTEGER })).now()).rejects.toMatchObject({ code: "UNKNOWN" });
    await expect(new Rfc3161Clock(await options({ loadFloor: async () => Number.MAX_SAFE_INTEGER - 1, maxAdvanceMs: 2 })).now()).rejects.toMatchObject({ code: "UNKNOWN" });
    const commitRetry = vi.fn(async () => commitRetry.mock.calls.length > 1);
    await expect(new Rfc3161Clock(await options({ commitFloor: commitRetry })).now()).resolves.toMatchObject({ authority: TSA_URL }); expect(commitRetry).toHaveBeenCalledTimes(2);
    const commitFail = vi.fn(async () => false); await expect(new Rfc3161Clock(await options({ commitFloor: commitFail })).now()).rejects.toMatchObject({ code: "UNKNOWN" }); expect(commitFail).toHaveBeenCalledTimes(3);
  }, 45_000);
  it("bounds transport, child, and reply-body failures and removes operation directories", async () => {
    const before = await corelinkTsaDirs();
    await expect(new Rfc3161Clock(await options({ maxResponseBytes: 32, fetcher: async () => new Response(new Uint8Array(33), { status: 200 }) })).now()).rejects.toMatchObject({ code: "INVALID" });
    await expect(new Rfc3161Clock(await options({ timeoutMs: 25, fetcher: async () => new Promise<Response>(() => {}) })).now()).rejects.toMatchObject({ code: "TIMEOUT" });
    await expect(new Rfc3161Clock(await options({ fetcher: async () => new Response(null, { status: 302, headers: { location: "https://other.invalid/tsa" } }) })).now()).rejects.toMatchObject({ code: "UNKNOWN" });
    const sleeperDir = await mkdtemp(join(tmpdir(), "corelink-tsa-sleeper-")); const sleeper = join(sleeperDir, "openssl-sleep"); await writeFile(sleeper, "#!/bin/sh\nsleep 1\n", { mode: 0o700 });
    await expect(new Rfc3161Clock(await options({ opensslPath: sleeper, timeoutMs: 25 })).now()).rejects.toMatchObject({ code: "TIMEOUT" }); await rm(sleeperDir, { recursive: true, force: true }); expect(await corelinkTsaDirs()).toEqual(before);
  }, 20_000);
});
describe("Rfc3161Clock option bounds", () => {
  it("rejects unsafe transport bounds and a missing CRL policy", async () => {
    const base = await options(); expect(() => new Rfc3161Clock({ ...base, timeoutMs: 5_001 })).toThrow(TrustedTimeError); expect(() => new Rfc3161Clock({ ...base, crlUrls: [] })).toThrow(TrustedTimeError); expect(() => new Rfc3161Clock({ ...base, maxResponseBytes: 256 * 1024 + 1 })).toThrow(TrustedTimeError); expect(() => new Rfc3161Clock({ ...base, endpoint: "https://user:pass@tsa.invalid/path?x=1" })).toThrow(TrustedTimeError);
  });
});
