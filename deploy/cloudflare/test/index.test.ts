// Unit tests for the spawn-Worker's security-critical pure logic: constant-time
// auth compare, GitHub HMAC verification, and the FAIL-OPEN warm-mint env build.
// Plain vitest (node) — these functions don't need the Workers runtime
// (crypto.subtle + crypto.randomUUID are on Node 20+).
import { describe, it, expect, vi, afterEach } from "vitest";
import {
  safeEqual,
  verifyGithubHmac,
  buildContainerEnv,
  revokeCasPatById,
  buildUsageEvent,
  pushUsageEvent,
  usageIdemKey,
  billingPeriod,
} from "../src/lib";

describe("safeEqual (constant-time bearer compare)", () => {
  it("true for equal strings", () => expect(safeEqual("abc", "abc")).toBe(true));
  it("false for different strings of equal length", () =>
    expect(safeEqual("abc", "abd")).toBe(false));
  it("false for different lengths", () => expect(safeEqual("abc", "abcd")).toBe(false));
  it("false for empty vs non-empty", () => expect(safeEqual("", "x")).toBe(false));
});

// Sign a body exactly as GitHub does (HMAC-SHA256 → `sha256=<hex>`).
async function ghSign(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `sha256=${hex}`;
}

describe("verifyGithubHmac", () => {
  const secret = "whsec-test";
  const body = '{"action":"queued"}';

  it("accepts a correctly-signed body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(true);
  });
  it("rejects a tampered body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body + " ")).toBe(false);
  });
  it("rejects a wrong secret", async () => {
    const sig = await ghSign("other", body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(false);
  });
  it("rejects a signature without the sha256= prefix", async () => {
    expect(await verifyGithubHmac(secret, "deadbeef", body)).toBe(false);
  });
  it("rejects an empty signature", async () => {
    expect(await verifyGithubHmac(secret, "", body)).toBe(false);
  });
});

describe("buildContainerEnv (warm-mint, FAIL-OPEN to cold)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const JIT = "encoded-jit";
  const JOB = "987654321"; // GH workflow_job.id

  it("COLD when no mint key configured (only the JIT, no CLW_*, no patId)", async () => {
    const env = { CLW_TENANT: "t" } as never; // key absent
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CORELINK_RUNNER_JITCONFIG).toBe(JIT);
    expect(containerEnv.CLW_TOKEN).toBeUndefined();
    expect(containerEnv.CLW_ENDPOINT).toBeUndefined();
    expect(patId).toBeUndefined();
  });

  it("COLD when no tenant configured", async () => {
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never; // tenant absent
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CLW_TOKEN).toBeUndefined();
    expect(patId).toBeUndefined();
  });

  it("WARM when key+tenant present and the D-9 mint succeeds (returns patId)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(JSON.stringify({ token_plaintext: "per-job-pat", pat_id: "pat-123" }), {
          status: 200,
        }),
      ),
    );
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "ee30f7ba",
      CLW_ENDPOINT: "https://corelink-api.humangr.com",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    } as never;
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CORELINK_RUNNER_JITCONFIG).toBe(JIT);
    expect(containerEnv.CLW_TOKEN).toBe("per-job-pat");
    expect(containerEnv.CLW_TENANT).toBe("ee30f7ba");
    expect(containerEnv.CLW_ENDPOINT).toBe("https://corelink-api.humangr.com");
    expect(containerEnv.CLW_REF_DOMAIN).toBe("runner");
    expect(patId).toBe("pat-123"); // carried out for KV → revoke-by-pat_id
  });

  it("mints under the GH workflow_job.id (correlation id)", async () => {
    const fetchMock = vi.fn(
      async () =>
        new Response(JSON.stringify({ token_plaintext: "per-job-pat", pat_id: "pat-123" }), {
          status: 200,
        }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    await buildContainerEnv(env, JIT, JOB);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/mint");
    expect(JSON.parse((init as RequestInit).body as string).job_id).toBe(JOB);
  });

  it("FAIL-OPEN to COLD when the D-9 mint returns non-2xx", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CORELINK_RUNNER_JITCONFIG).toBe(JIT); // job still runs
    expect(containerEnv.CLW_TOKEN).toBeUndefined(); // but cold — no CLW_*
    expect(patId).toBeUndefined();
  });

  it("FAIL-OPEN to COLD when the mint 200 lacks token_plaintext", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CLW_TOKEN).toBeUndefined();
    expect(patId).toBeUndefined();
  });

  it("FAIL-OPEN to COLD when the mint 200 has a token but no pat_id", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify({ token_plaintext: "x" }), { status: 200 })),
    );
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    const { containerEnv, patId } = await buildContainerEnv(env, JIT, JOB);
    expect(containerEnv.CLW_TOKEN).toBeUndefined(); // can't track for revoke ⇒ cold
    expect(patId).toBeUndefined();
  });
});

describe("revokeCasPatById (revoke-by-pat_id, the live /revoke contract)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("POSTs {pat_id, owner_tenant} to /revoke with the auth header on 2xx", async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "ee30f7ba",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    } as never;
    await revokeCasPatById(env, "pat-123");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/revoke");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body.pat_id).toBe("pat-123");
    expect(body.owner_tenant).toBe("ee30f7ba");
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "k" });
  });

  it("throws on non-2xx (caller swallows — fail-open)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("bad", { status: 400 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    await expect(revokeCasPatById(env, "pat-123")).rejects.toThrow(/D-9 revoke 400/);
  });
});

describe("billing usage-push (ASK-2 — runner_slot_seconds)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("billingPeriod is UTC YYYY-MM", () => {
    expect(billingPeriod(Date.parse("2026-06-23T11:08:00Z"))).toBe("2026-06");
    expect(billingPeriod(Date.parse("2026-01-01T00:00:00Z"))).toBe("2026-01");
  });

  it("usageIdemKey is deterministic 64-hex, scoped to (job, period)", async () => {
    const a = await usageIdemKey("job-1", "2026-06");
    expect(a).toMatch(/^[0-9a-f]{64}$/);
    expect(await usageIdemKey("job-1", "2026-06")).toBe(a); // deterministic
    expect(await usageIdemKey("job-2", "2026-06")).not.toBe(a); // job-scoped
    expect(await usageIdemKey("job-1", "2026-07")).not.toBe(a); // period-scoped
  });

  it("buildUsageEvent computes slot·seconds, the canonical kind, and the full wire", async () => {
    const started = "2026-06-23T11:00:00Z";
    const completed = "2026-06-23T11:00:03Z"; // +3s
    const ev = await buildUsageEvent({
      tenantId: "3560e213-1e23-4fd0-8871-7033c6052ebd",
      jobId: "82597479935",
      startedMs: Date.parse(started),
      completedMs: Date.parse(completed),
      region: "iad",
    });
    expect(ev.tenant_id).toBe("3560e213-1e23-4fd0-8871-7033c6052ebd");
    expect(ev.event_kind).toBe("runner_slot_seconds");
    expect(ev.qty).toBe(3); // (11:00:03 − 11:00:00)/1000
    expect(ev.region).toBe("iad");
    expect(ev.source).toBe("corelink-runners/spawn-worker");
    expect(ev.billing_period).toBe("2026-06");
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);
    expect(ev.time_ms).toBe(Date.parse(completed));
  });

  it("buildUsageEvent clamps a negative duration (clock skew) to qty 0", async () => {
    const ev = await buildUsageEvent({
      tenantId: "t",
      jobId: "j",
      startedMs: 5000,
      completedMs: 1000, // completed before started
      region: "iad",
    });
    expect(ev.qty).toBe(0); // never bills negative
  });

  it("pushUsageEvent POSTs a one-event batch with the dedicated key on 2xx", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ accepted: 1 }), { status: 202 }));
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      BILLING_INGEST_URL: "https://corelink-api.humangr.com/internal/v1/billing/usage",
      BILLING_INGEST_AUTH_KEY: "billing-key",
      CLW_TENANT: "t",
    };
    const ev = await buildUsageEvent({
      tenantId: "t",
      jobId: "j",
      startedMs: 0,
      completedMs: 2000,
      region: "iad",
    });
    await pushUsageEvent(env, ev);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/billing/usage");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(Array.isArray(body)).toBe(true); // a batch
    expect(body[0].event_kind).toBe("runner_slot_seconds");
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "billing-key" });
  });

  it("pushUsageEvent throws on non-2xx (caller swallows — fail-open)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("bad", { status: 400 })));
    const env = { BILLING_INGEST_URL: "https://x/usage", BILLING_INGEST_AUTH_KEY: "k", CLW_TENANT: "t" };
    const ev = await buildUsageEvent({ tenantId: "t", jobId: "j", startedMs: 0, completedMs: 1000, region: "iad" });
    await expect(pushUsageEvent(env, ev)).rejects.toThrow(/billing usage-push 400/);
  });
});
