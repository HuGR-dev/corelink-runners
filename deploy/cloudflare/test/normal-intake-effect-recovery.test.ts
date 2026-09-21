import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import { ConcurrencySlotsDO, runNormalIntakeDrain } from "../src/index";
import { containmentSpawnActiveKey, containmentSpawnAttemptKey, intakeOwnerTuple } from "../src/containment_effect_route";
import { FakeStorage, digest, env, kv, makeDO, ns } from "./containment-redrive-test-helpers";

const BODY_SHA = "a".repeat(64);

function fixture() {
  const d = makeDO();
  const store = kv();
  const slotsStorage = new FakeStorage();
  const slots = new ConcurrencySlotsDO({ storage: slotsStorage } as never, {} as never);
  const runtime = env(d, store, {
    CORELINK_RUNNER_MINT_AUTH_KEY: "mint-auth",
    CORELINK_MINT_URL: "https://mint.example",
    SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
    CONCURRENCY_SLOTS: ns(slots),
  });
  const issuedOperations = new Map<string, string>();
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/runner/authorize")) return Response.json({ tenant: "tenant-a", max_concurrency: 2 });
    if (url.endsWith("/runner/mint")) {
      const body = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: string };
      const operationId = body.operation_id ?? "operation";
      const patId = `pat-${operationId}`;
      issuedOperations.set(operationId, patId);
      return Response.json({ operation_id: operationId, tenant: "tenant-a", lifecycle_generation: "1", max_concurrency: 2, pat_id: patId, token_plaintext: `secret-${operationId}` });
    }
    if (url.endsWith("/runner/adopt")) return new Response(null, { status: 204 });
    if (url.includes("generate-jitconfig")) return Response.json({ encoded_jit_config: "jit", runner: { id: 5 } });
    if (url.includes("/actions/runners/") && init?.method === "DELETE") return new Response(null, { status: 204 });
    if (url.endsWith("/runner/revoke")) return new Response(null, { status: 204 });
    throw new Error(`unexpected external URL: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const startWithEnv = vi.fn(async () => {});
  const teardown = vi.fn(async () => {});
  vi.mocked(getContainer).mockReturnValue({ startWithEnv, teardown } as never);
  return { d, store, slotsStorage, runtime, fetchMock, startWithEnv, teardown };
}

async function enqueue(f: ReturnType<typeof fixture>, eventId: string, jobId: string) {
  const result = await f.d.instance.normalIntakeEnqueue({
    schema_version: 1,
    event_id: eventId,
    body_sha256: BODY_SHA,
    job_id: jobId,
    repo: "acme/repo",
    installation_id: "42",
    labels: ["corelink"],
    received_at_ms: Date.now(),
  });
  expect(result.status).toBe("accepted");
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_750_000_000_000); vi.clearAllMocks(); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

describe("normal intake effect recovery", () => {
  it("keeps a fully abandoned post-DRIVING start failure recoverable", async () => {
    const f = fixture();
    await enqueue(f, "failed-start-delivery", "8601");
    f.startWithEnv.mockRejectedValue(new Error("container start rejected"));

    const drain = runNormalIntakeDrain(f.runtime);
    await vi.waitFor(() => expect(f.startWithEnv).toHaveBeenCalledTimes(1));
    await vi.runAllTimersAsync();
    await drain;

    const record = f.d.storage.map.get("normal-inbox:v1:event:failed-start-delivery") as { state: string; next_attempt_ms: number };
    const recoverableRetry = record.state === "pending" && record.next_attempt_ms > Date.now();
    const recoverableDeadLetter = f.store.map.has("orphan:8601");
    expect(recoverableRetry || recoverableDeadLetter).toBe(true);

    const jitAttempts = f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"));
    const deletedRegistrations = f.fetchMock.mock.calls.filter(([url, init]) => String(url).includes("/actions/runners/") && init?.method === "DELETE");
    expect(jitAttempts.length).toBeGreaterThan(0);
    expect(f.startWithEnv).toHaveBeenCalledTimes(jitAttempts.length);
    expect(deletedRegistrations).toHaveLength(jitAttempts.length);
    expect(f.teardown).toHaveBeenCalledTimes(f.startWithEnv.mock.calls.length);
  });

  it("does not let a concurrent DRIVING observer poison the eventual committed inbox state", async () => {
    const f = fixture();
    await enqueue(f, "concurrent-delivery", "8602");
    let releaseStart!: () => void;
    let markStartEntered!: () => void;
    const startBlocked = new Promise<void>(resolve => { releaseStart = resolve; });
    const startEntered = new Promise<void>(resolve => { markStartEntered = resolve; });
    f.startWithEnv.mockImplementationOnce(async () => { markStartEntered(); await startBlocked; });

    const firstDrain = runNormalIntakeDrain(f.runtime);
    await startEntered;
    await runNormalIntakeDrain(f.runtime);
    releaseStart();
    await firstDrain;
    vi.clearAllTimers();

    const record = f.d.storage.map.get("normal-inbox:v1:event:concurrent-delivery") as { state: string };
    const activeCount = f.d.storage.map.get("normal-inbox:v1:count");
    const committedEffect = [...f.d.storage.map.values()].some(value => {
      const state = value as { path?: string; state?: string };
      return state.path === "intake" && state.state === "COMMITTED";
    });
    expect(committedEffect).toBe(true);
    expect(record.state).toBe("complete");
    expect(activeCount).toBe(0);
  });

  it("settles an expired sole DRIVING owner as uncertain without preparing or reclaiming", async () => {
    const f = fixture(); const eventId = "expired-driving-delivery"; const jobId = "8604";
    await enqueue(f, eventId, jobId);
    const tuple = await intakeOwnerTuple("acme/repo", jobId, `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    expect((await f.d.instance.ownerPrepare(request)).kind).toBe("prepared");
    expect((await f.d.instance.ownerAcquire(request)).kind).toBe("acquired");
    const mirror = await f.d.instance.ownerMirror(request, "acquired");
    expect(mirror.kind).toBe("exact");
    const confirmed = await f.d.instance.ownerConfirm(
      { ...request, observation_kind: mirror.kind, observation_digest: mirror.payload_digest },
      mirror.payload_digest!, mirror.payload_digest!,
    );
    expect(confirmed.kind).toBe("permit_issued");
    const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
    const bindingBase = { schema_version: 1 as const, provider: "cloudflare-container", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
    expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    expect((await f.d.instance.ownerMarkDriving(request, confirmed.permit!.permit_id, started.proof!.proof_id)).kind).toBe("driving");
    const attemptKey = containmentSpawnAttemptKey(tuple);
    const attempt = f.d.storage.map.get(attemptKey) as { expires_ms: number };
    attempt.expires_ms = Date.now() - 1;
    f.d.storage.map.set(attemptKey, attempt);

    await runNormalIntakeDrain(f.runtime);

    const inbox = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string };
    const pendingIndex = [...f.d.storage.map.keys()].filter(key => key.startsWith("normal-inbox:v1:pending:"));
    const authorizations = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"));
    const mints = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"));
    expect(inbox.state).toBe("uncertain");
    expect(pendingIndex).toHaveLength(0);
    expect(await f.d.instance.normalIntakePending()).toHaveLength(0);
    expect(authorizations).toHaveLength(0); expect(mints).toHaveLength(0);
    expect(f.store.map.has(`spawn:${jobId}`)).toBe(false);
    expect(f.startWithEnv).not.toHaveBeenCalled();
  });

  it("quarantines corrupt persisted owner evidence without repeating preparation", async () => {
    const f = fixture();
    await enqueue(f, "corrupt-owner-delivery", "8603");
    const tuple = await intakeOwnerTuple("acme/repo", "8603", "containment:v1:corrupt-owner-delivery", "corrupt-owner-delivery");
    f.d.storage.map.set(containmentSpawnActiveKey(tuple), { schema_version: 1, state: "DRIVING" });

    await runNormalIntakeDrain(f.runtime);
    const afterFirstDrain = f.d.storage.map.get("normal-inbox:v1:event:corrupt-owner-delivery") as { state: string; next_attempt_ms: number };
    const firstAuthorizationCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize")).length;
    const firstMintCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint")).length;

    expect.soft(afterFirstDrain.state).toBe("uncertain");
    expect.soft(firstAuthorizationCalls).toBe(0);
    expect.soft(firstMintCalls).toBe(0);

    await vi.advanceTimersByTimeAsync(Math.max(0, afterFirstDrain.next_attempt_ms - Date.now()));
    await runNormalIntakeDrain(f.runtime);

    const afterSecondDrain = f.d.storage.map.get("normal-inbox:v1:event:corrupt-owner-delivery") as { state: string };
    const authorizationCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize")).length;
    const mintCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint")).length;
    expect.soft(afterSecondDrain.state).toBe("uncertain");
    expect.soft(authorizationCalls).toBe(firstAuthorizationCalls);
    expect.soft(mintCalls).toBe(firstMintCalls);
  });
});
