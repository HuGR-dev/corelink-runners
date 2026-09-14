import { describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({ teardown: vi.fn(async () => {}) })),
}));

import worker, { type Env } from "../src/index";

function kv() {
  const values = new Map<string, string>();
  return {
    values,
    async get(key: string) { return values.get(key) ?? null; },
    async put(key: string, value: string) { values.set(key, value); },
    async delete(key: string) { values.delete(key); },
  };
}

async function sign(body: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode("secret"), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  return `sha256=${[...new Uint8Array(mac)].map(b => b.toString(16).padStart(2, "0")).join("")}`;
}

it("completion of a job longer than 7200s reopens durable authority and retains identity across failed billing", async () => {
  const jobs = kv();
  let tenant = "tenant-authority";
  const authority = {
    snapshot: vi.fn(async () => ({})),
    readJobAttribution: vi.fn(async () => JSON.stringify({ jobId: "9001", tenant })),
    deleteJobAttribution: vi.fn(async () => {}),
  };
  const env = {
    RUNNER_CONTAINER: { _ns: "runner" },
    CHECK_HOST_CONTAINER: { _ns: "check" },
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    GITHUB_WEBHOOK_SECRET: "secret",
    GITHUB_MINT_TOKEN: "ghp-test",
    BILLING_INGEST_URL: "https://billing.invalid/usage",
    BILLING_INGEST_AUTH_KEY: "billing-key",
    BILLING_REGION: "iad",
    RUNNER_JOB_PATS: jobs,
    CONTAINMENT: {
      idFromName: () => "global",
      get: () => authority,
    },
  } as unknown as Env;
  const body = JSON.stringify({ action: "completed", workflow_job: {
    id: 9001,
    labels: ["corelink-dogfood"],
    started_at: "2026-07-17T00:00:00.000Z",
    completed_at: "2026-07-18T03:00:00.000Z",
  }});
  const originalFetch = globalThis.fetch;
  vi.stubGlobal("fetch", vi.fn(async () => new Response("retry", { status: 503 })));
  vi.setSystemTime(new Date("2026-07-18T10:00:01.000Z"));
  try {
    const response = await worker.fetch(new Request("https://worker.invalid/webhook", {
      method: "POST",
      headers: { "x-github-event": "workflow_job", "x-hub-signature-256": await sign(body) },
      body,
    }), env, { waitUntil() {}, passThroughOnException() {} } as never);
    const payload = await response.json();
    expect(response.status, JSON.stringify(payload)).toBe(200);
    expect(payload).toMatchObject({ billed: false, ledgered: true });
    expect(authority.readJobAttribution).toHaveBeenCalledWith("job-attribution:9001");
    expect(JSON.parse(jobs.values.get("usage:9001") as string).tenant).toBe(tenant);
    expect(authority.deleteJobAttribution).not.toHaveBeenCalled();
    tenant = "tenant-authority";
    expect((await authority.readJobAttribution("job-attribution:9001"))).toContain(tenant);
  } finally {
    vi.useRealTimers();
    vi.stubGlobal("fetch", originalFetch);
  }
});
