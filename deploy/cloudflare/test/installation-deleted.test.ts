import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import worker, { redriveOrphanedJobs, retryOrphanedSpawns, runContainmentDrain } from "../src/index";
import { ctx, env, event, makeDO, kv, providerReceipt, T0, writeDeliveredProof } from "./containment-redrive-test-helpers";

async function request(secret: string, body: unknown, delivery = "delivery") {
  const raw = JSON.stringify(body); const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(raw));
  const hex = [...new Uint8Array(signature)].map(n => n.toString(16).padStart(2, "0")).join("");
  return new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "installation", "x-github-delivery": delivery, "x-hub-signature-256": `sha256=${hex}` }, body: raw });
}
async function workflowRequest(secret: string, body: unknown, delivery = "workflow-delivery") {
  const raw = JSON.stringify(body); const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(raw));
  const hex = [...new Uint8Array(signature)].map(n => n.toString(16).padStart(2, "0")).join("");
  return new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "workflow_job", "x-github-delivery": delivery, "x-hub-signature-256": `sha256=${hex}` }, body: raw });
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
    const d = makeDO(); const runtime = env(d, kv(), { GITHUB_WEBHOOK_REPO_SECRET: "repo", GITHUB_WEBHOOK_REPO_SECRET_NEXT: "repo-next" }); const body = { action: "deleted", installation: { id: 42 } };
    expect((await worker.fetch(await request("repo", body), runtime, ctx() as never)).status).toBe(401);
    expect((await worker.fetch(await request("repo-next", body, "next"), runtime, ctx() as never)).status).toBe(401);
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

  it.each([
    [0, "zero numeric"],
    ["0", "zero string"],
    ["042", "leading zero"],
    [Number.MAX_SAFE_INTEGER + 1, "greater than MAX_SAFE_INTEGER numeric"],
    ["9007199254740992", "greater than MAX_SAFE_INTEGER string"],
    ["99999999999999999999", "greater than the safe decimal width"],
  ])("rejects non-canonical installation id (%s: %s) before writing a tombstone", async (id) => {
    const d = makeDO(); const runtime = env(d, kv(), { GITHUB_MINT_TOKEN: undefined });
    const response = await worker.fetch(await request("secret", { action: "deleted", installation: { id } }, `invalid-${String(id)}`), runtime, ctx() as never);
    expect(response.status).toBe(400);
    expect([...d.storage.map.keys()].some(key => key.includes("installation-tombstone"))).toBe(false);
  });

  it.each([42, "42"])("accepts the same canonical installation identity from numeric/string JSON (%s)", async (id) => {
    const d = makeDO(); const runtime = env(d, kv(), { GITHUB_MINT_TOKEN: undefined });
    expect((await worker.fetch(await request("secret", { action: "deleted", installation: { id } }, `canonical-${typeof id}`), runtime, ctx() as never)).status).toBe(202);
    expect(await d.instance.installationTombstoned("42")).toBe(true);
    expect(await d.instance.installationTombstoned(String(id))).toBe(true);
  });
});

describe("AU5.10 tombstone fences every recovery side effect", () => {
  async function tombstone(d: ReturnType<typeof makeDO>, installationId = "42") {
    expect(await d.instance.tombstoneInstallation(installationId, `deleted-${installationId}`, "b".repeat(64))).toBe("accepted");
  }

  it("blocks the first-party GitHub scan before listing, handoff, reservation, or drive", async () => {
    const d = makeDO(); const store = kv(); await tombstone(d);
    const list = vi.fn(async () => [{ jobId: "123", labels: ["corelink"] }]);
    const drive = vi.fn(async () => providerReceipt({ jobId: "123", repo: "acme/repo" }));
    await redriveOrphanedJobs(
      env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REPOS: "acme/repo" }),
      ctx() as never,
      undefined,
      T0,
      { listOrphanRunnerJobs: list, driveSpawn: drive },
    );
    expect(list).not.toHaveBeenCalled();
    expect(drive).not.toHaveBeenCalled();
    expect(store.put).not.toHaveBeenCalled();
    expect(store.delete).not.toHaveBeenCalled();
    expect([...d.storage.map.keys()].some(key => key.includes("reservation"))).toBe(false);
  });

  it("blocks an installation-token / GitHub scan path after registry discovery", async () => {
    const d = makeDO(); const store = kv(); await tombstone(d);
    const fetchSpy = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "https://registry.example/candidates") {
        return Response.json({ schema_version: 1, source: "runner_authorization_candidates", repositories: [{ repo_full_name: "acme/repo", installation_id: "42" }], next_cursor: null });
      }
      throw new Error(`unexpected external call: ${url}`);
    });
    vi.stubGlobal("fetch", fetchSpy);
    await redriveOrphanedJobs(
      env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0", RECONCILER_REGISTRY_URL: "https://registry.example/candidates", RECONCILER_REGISTRY_AUTH_KEY: "registry-key", GITHUB_APP_ID: "1", GITHUB_APP_PRIVATE_KEY: "private-key" }),
      ctx() as never,
      undefined,
      T0,
      { driveSpawn: vi.fn(async () => providerReceipt({ jobId: "123", repo: "acme/repo" })) },
    );
    expect(fetchSpy.mock.calls.map(([input]) => String(input))).toEqual(["https://registry.example/candidates"]);
    expect([...d.storage.map.keys()].some(key => key.includes("reservation"))).toBe(false);
    expect(store.put).not.toHaveBeenCalled();
    expect(store.delete).not.toHaveBeenCalled();
  });

  it("blocks warm orphan retry before placement verification, reservation, KV writes, or drive", async () => {
    const d = makeDO();
    const store = kv({ "orphan:123": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 - 1, placedMs: T0 - 300_000 }) });
    await tombstone(d);
    const verify = vi.fn(async () => ({ status: "queued", runner_id: 0 }));
    const drive = vi.fn(async () => providerReceipt({ jobId: "123", repo: "acme/repo" }));
    await retryOrphanedSpawns(env(d, store, { AUTOSCALER_REDRIVE_PAUSED: "0" }), ctx() as never, T0, drive, verify);
    expect(verify).not.toHaveBeenCalled();
    expect(drive).not.toHaveBeenCalled();
    expect(store.put).not.toHaveBeenCalled();
    expect(store.delete).not.toHaveBeenCalled();
    expect([...d.storage.map.keys()].some(key => key.includes("reservation"))).toBe(false);
  });

  it("terminalizes a tombstoned containment head after lease recovery and continues with another installation", async () => {
    const d = makeDO(); const store = kv();
    await d.instance.bootstrapContainedEventIndex("acme/repo", "1");
    await d.instance.bootstrapContainedEventIndex("acme/repo", "2");
    await d.instance.append(event(1, { installation_id: "42" }));
    await d.instance.append(event(2, { installation_id: "43", event_id: "evt-2", effect_id: "containment:v1:evt-2" }));
    await d.instance.acquireLease("old", T0);
    await d.instance.claimNext("old", 1, T0);
    await tombstone(d);
    vi.useFakeTimers();
    try {
      vi.setSystemTime(T0 + 120_000);
      const drive = vi.fn(async (_env: unknown, opts: { jobId: string; repo: string; effect_id?: string; containment_event_id?: string; effect_permit_id?: string }) => {
        await writeDeliveredProof(store, { jobId: opts.jobId, effect_id: opts.effect_id!, containment_event_id: opts.containment_event_id!, effect_permit_id: opts.effect_permit_id! });
        return providerReceipt(opts);
      });
      await runContainmentDrain(env(d, store), { claimSpawn: async () => true, bindContainmentSpawnClaim: async () => {}, driveSpawn: drive });
      expect(drive).toHaveBeenCalledTimes(1);
      expect(drive.mock.calls[0][1]).toMatchObject({ jobId: "2", installationId: "43" });
      expect(await d.instance.getEvent("evt-1")).toBeNull();
      expect(await d.instance.getEvent("evt-2")).toBeNull();
      expect(await d.instance.snapshot()).toMatchObject({ backlog_count: 0, drain_cursor: 2 });
    } finally {
      vi.useRealTimers();
    }
  });

  it("still admits a different installation and permits completion cleanup for the deleted one", async () => {
    const d = makeDO(); const store = kv({ "orphan:900": JSON.stringify({ repo: "acme/repo", installationId: "42", labels: ["corelink"], attempts: 1, firstRecordedMs: T0 }) });
    await tombstone(d);
    const admitted = await d.instance.normalIntakeEnqueue({ schema_version: 1, event_id: "other-install", body_sha256: "c".repeat(64), job_id: "901", repo: "acme/repo", installation_id: "43", labels: ["corelink"], received_at_ms: T0 });
    expect(admitted.status).toBe("accepted");
    expect(await d.instance.installationTombstoned("43")).toBe(false);
    const completed = await workflowRequest("secret", { action: "completed", workflow_job: { id: 900, labels: ["corelink"] }, repository: { full_name: "acme/repo" }, installation: { id: 42 } }, "completed-900");
    const response = await worker.fetch(completed, env(d, store), ctx() as never);
    expect(response.status).toBe(200);
    expect(store.map.has("orphan:900")).toBe(false);
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

  it("emits exactly one counter for a structurally-valid bad HMAC with both secrets configured", async () => {
    const d = makeDO(); const bump = vi.fn(async () => {});
    const metrics = { idFromName: vi.fn(() => "singleton"), get: vi.fn(() => ({ bump })) };
    const bad = new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "ping", "x-hub-signature-256": `sha256=${"0".repeat(64)}` }, body: "{}" });
    const c = ctx();
    expect((await worker.fetch(bad, env(d, kv(), { GITHUB_WEBHOOK_REPO_SECRET: "repo-secret", METRICS: metrics }), c as never)).status).toBe(401);
    await Promise.all(c.tasks);
    expect(bump).toHaveBeenCalledTimes(1);
    expect(bump).toHaveBeenCalledWith(["webhook_auth_failed"]);
  });

  it("keeps the bad-HMAC response at 401 when the metric store fails", async () => {
    const d = makeDO(); const bump = vi.fn(async () => { throw new Error("metrics unavailable"); });
    const metrics = { idFromName: vi.fn(() => "singleton"), get: vi.fn(() => ({ bump })) };
    const bad = new Request("https://worker/webhook", { method: "POST", headers: { "x-github-event": "ping", "x-hub-signature-256": `sha256=${"f".repeat(64)}` }, body: "{}" });
    const c = ctx();
    expect((await worker.fetch(bad, env(d, kv(), { METRICS: metrics }), c as never)).status).toBe(401);
    await Promise.all(c.tasks);
    expect(bump).toHaveBeenCalledTimes(1);
  });
});
