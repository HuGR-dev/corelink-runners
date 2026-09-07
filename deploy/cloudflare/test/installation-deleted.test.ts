import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import worker from "../src/index";
import { ctx, env, makeDO, kv } from "./containment-redrive-test-helpers";

async function request(secret: string, body: unknown, delivery = "delivery") {
  const raw = JSON.stringify(body); const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(raw));
  const hex = [...new Uint8Array(signature)].map(n => n.toString(16).padStart(2, "0")).join("");
  return new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "installation", "x-github-delivery": delivery, "x-hub-signature-256": `sha256=${hex}` }, body: raw });
}
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("AU5.10 installation.deleted", () => {
  it("accepts the App secret without a mint token and fences future intake", async () => {
    const d = makeDO(); const runtime = env(d, kv(), { GITHUB_MINT_TOKEN: undefined });
    expect((await worker.fetch(await request("secret", { action: "deleted", installation: { id: 42 } }), runtime, ctx() as never)).status).toBe(202);
    expect(await d.instance.installationTombstoned("42")).toBe(true);
    const queued = await d.instance.normalIntakeEnqueue({ schema_version: 1, event_id: "after", body_sha256: "a".repeat(64), job_id: "1", repo: "acme/repo", installation_id: "42", labels: ["corelink"], received_at_ms: Date.now() });
    expect(queued.status).toBe("tombstoned");
  });
  it("rejects repository-secret deletion and preserves idempotency/conflict semantics", async () => {
    const d = makeDO(); const runtime = env(d, kv(), { GITHUB_WEBHOOK_REPO_SECRET: "repo" }); const body = { action: "deleted", installation: { id: 42 } };
    expect((await worker.fetch(await request("repo", body), runtime, ctx() as never)).status).toBe(401);
    expect((await worker.fetch(await request("secret", body, "same"), runtime, ctx() as never)).status).toBe(202);
    expect((await worker.fetch(await request("secret", body, "same"), runtime, ctx() as never)).status).toBe(202);
    expect((await worker.fetch(await request("secret", { action: "deleted", installation: { id: 43 } }, "same"), runtime, ctx() as never)).status).toBe(409);
  });
  it("rejects malformed installation ids and returns 503 on tombstone storage failure", async () => {
    const d = makeDO(); const runtime = env(d, kv());
    expect((await worker.fetch(await request("secret", { action: "deleted", installation: { id: "bad" } }), runtime, ctx() as never)).status).toBe(400);
    vi.spyOn(d.instance, "tombstoneInstallation").mockRejectedValueOnce(new Error("down"));
    expect((await worker.fetch(await request("secret", { action: "deleted", installation: { id: 42 } }), runtime, ctx() as never)).status).toBe(503);
  });
});

describe("AU6.16 webhook authentication metric", () => {
  it("counts a configured structurally-valid bad HMAC once, and config absence stays silent", async () => {
    const d = makeDO(); const bump = vi.fn(async () => {});
    const metrics = { idFromName: vi.fn(() => "singleton"), get: vi.fn(() => ({ bump })) };
    const runtime = env(d, kv(), { METRICS: metrics });
    const body = { ignored: true };
    const bad = new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "ping", "x-hub-signature-256": `sha256=${"0".repeat(64)}` }, body: JSON.stringify(body) });
    const c = ctx(); expect((await worker.fetch(bad, runtime, c as never)).status).toBe(401); await Promise.all(c.tasks);
    expect(bump).toHaveBeenCalledTimes(1); expect(bump).toHaveBeenCalledWith(["webhook_auth_failed"]);
    const absent = env(makeDO(), kv(), { GITHUB_WEBHOOK_SECRET: undefined, GITHUB_WEBHOOK_REPO_SECRET: undefined, METRICS: metrics });
    expect((await worker.fetch(bad, absent, ctx() as never)).status).toBe(503); expect(bump).toHaveBeenCalledTimes(1);
  });
});
