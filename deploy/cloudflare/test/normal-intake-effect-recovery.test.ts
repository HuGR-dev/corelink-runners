import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import worker, { ConcurrencySlotsDO, runNormalIntakeDrain } from "../src/index";
import { containmentSpawnActiveKey, containmentSpawnAttemptKey, intakeOwnerTuple } from "../src/containment_effect_route";
import { ctx, FakeStorage, digest, env, kv, makeDO, ns } from "./containment-redrive-test-helpers";

const BODY_SHA = "a".repeat(64);
const READBACK_ADMIN = "normal-intake-recovery-admin";

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

async function stageConfirmedIntakeOwner(f: ReturnType<typeof fixture>, eventId: string, jobId: string) {
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
  return { tuple, request, mirror, confirmed };
}

function failBoundTransactionOnce(f: ReturnType<typeof fixture>) {
  const storage = f.d.storage as unknown as { transaction: (fn: (tx: any) => Promise<unknown>) => Promise<unknown> };
  const original = storage.transaction.bind(f.d.storage);
  let failed = false;
  storage.transaction = fn => original(async tx => {
    const put = tx.put.bind(tx);
    tx.put = async (key: string, value: unknown) => {
      if (!failed && key.startsWith("containment:v1:spawn-attempt:")
        && (value as { state?: string } | null)?.state === "BOUND") {
        failed = true;
        throw new Error("simulated crash after binding sidecar write");
      }
      return put(key, value);
    };
    return fn(tx);
  });
  return () => { storage.transaction = original; };
}

function loseBoundResponseOnce(f: ReturnType<typeof fixture>) {
  const storage = f.d.storage as unknown as { transaction: (fn: (tx: any) => Promise<unknown>) => Promise<unknown> };
  const original = storage.transaction.bind(f.d.storage);
  let committedBound = false;
  let lost = false;
  storage.transaction = fn => original(async tx => {
    const put = tx.put.bind(tx);
    tx.put = async (key: string, value: unknown) => {
      if (key.startsWith("containment:v1:spawn-attempt:")
        && (value as { state?: string } | null)?.state === "BOUND") committedBound = true;
      return put(key, value);
    };
    return fn(tx);
  }).then(result => {
    if (committedBound && !lost) { lost = true; throw new Error("simulated lost BOUND response after durable commit"); }
    return result;
  });
  return () => { storage.transaction = original; };
}

async function readIntake(f: ReturnType<typeof fixture>, eventId: string) {
  const runtime = env(f.d, f.store, { ...f.runtime, CONTAINMENT_ADMIN_KEY: READBACK_ADMIN });
  return worker.fetch(new Request(`https://worker/internal/v1/normal-intake?event_id=${encodeURIComponent(eventId)}`, {
    headers: { "x-corelink-internal-auth": READBACK_ADMIN },
  }), runtime, ctx() as never);
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
  it.each(["absent", "unavailable"] as const)("recovers confirmed intake when the mirror KV is later %s", async failure => {
    const f = fixture(); const eventId = `recover-confirmed-mirror-${failure}`; const jobId = "8606";
    await enqueue(f, eventId, jobId);
    const { tuple, mirror } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    if (failure === "absent") f.store.map.delete(mirror.key);
    else f.store.get.mockImplementation(async key => {
      if (key === mirror.key) throw new Error("mirror KV unavailable after durable confirmation");
      return f.store.map.get(key) ?? null;
    });

    await runNormalIntakeDrain(f.runtime);

    const record = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string };
    expect(record.state).toBe("complete");
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(1);
    expect(tuple.effect_id).toBe(`containment:v1:${eventId}`);
  });

  it("persists canonical binding intent before KV and retries a pre-sidecar failure once", async () => {
    const f = fixture(); const eventId = "binding-intent-before-kv"; const jobId = "8607";
    await enqueue(f, eventId, jobId);
    const { tuple, confirmed } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const bindingBase = { schema_version: 1 as const, provider: "cloudflare-container", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const bindingDigest = await digest(JSON.stringify(bindingBase));
    const attemptKey = containmentSpawnAttemptKey(tuple);
    const originalPut = f.store.put.getMockImplementation()!;
    let intentVisibleBeforeKv = false;
    let failedBeforeKv = false;
    f.store.put.mockImplementation(async (key, value) => {
      if (key.startsWith("containment:v1:effect-binding:")) {
        const attempt = f.d.storage.map.get(attemptKey) as Record<string, unknown>;
        const durable = JSON.stringify(attempt);
        intentVisibleBeforeKv = durable.includes(bindingDigest) && durable.includes(bindingBase.provider)
          && durable.includes(bindingBase.resource_id) && durable.includes(bindingBase.idempotency_key)
          && durable.includes(JSON.stringify(tuple)) && durable.includes(confirmed.permit!.permit_id)
          && typeof attempt.effect_start_proof_id === "string";
        if (!failedBeforeKv) { failedBeforeKv = true; throw new Error("simulated KV outage before binding sidecar write"); }
      }
      return originalPut(key, value);
    });

    await runNormalIntakeDrain(f.runtime);

    expect(intentVisibleBeforeKv).toBe(true);
    expect(f.store.map.has(`containment:v1:effect-binding:acme/repo/${jobId}/intake/${encodeURIComponent(tuple.effect_id)}`)).toBe(false);
    const firstRecord = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string; next_attempt_ms: number };
    expect(firstRecord.state).toBe("pending");
    expect(f.startWithEnv).not.toHaveBeenCalled();
    const readback = await readIntake(f, eventId);
    expect(readback.status).toBe(200);
    expect(await readback.json()).toMatchObject({ state: "pending", effect: { kind: "owned", state: "PERMIT_ISSUED" } });

    f.store.put.mockImplementation(originalPut);
    await vi.advanceTimersByTimeAsync(Math.max(0, firstRecord.next_attempt_ms - Date.now()));
    await runNormalIntakeDrain(f.runtime);

    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("complete");
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
  });

  it("recovers a durable binding intent after KV write and before the final owner transaction", async () => {
    const f = fixture(); const eventId = "binding-intent-after-kv"; const jobId = "8608";
    await enqueue(f, eventId, jobId);
    const { tuple } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const restoreStorage = failBoundTransactionOnce(f);

    await runNormalIntakeDrain(f.runtime);

    const bindingKey = `containment:v1:effect-binding:acme/repo/${jobId}/intake/${encodeURIComponent(tuple.effect_id)}`;
    expect(f.store.map.has(bindingKey)).toBe(true);
    const firstRecord = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string; next_attempt_ms: number };
    expect(firstRecord.state).toBe("pending");
    expect(f.startWithEnv).not.toHaveBeenCalled();
    const readback = await readIntake(f, eventId);
    expect(readback.status).toBe(200);
    expect(await readback.json()).toMatchObject({ state: "pending", effect: { kind: "owned", state: "PERMIT_ISSUED" } });

    restoreStorage();
    await vi.advanceTimersByTimeAsync(Math.max(0, firstRecord.next_attempt_ms - Date.now()));
    await runNormalIntakeDrain(f.runtime);

    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("complete");
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
  });

  it("rematerializes a missing BOUND sidecar from the canonical DO binding before provider start", async () => {
    const f = fixture(); const eventId = "bound-sidecar-rematerialize"; const jobId = "8611";
    await enqueue(f, eventId, jobId);
    const { tuple, request, confirmed } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
    const bindingBase = { schema_version: 1 as const, provider: "cloudflare-container", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
    expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    const bindingKey = `containment:v1:effect-binding:acme/repo/${jobId}/intake/${encodeURIComponent(tuple.effect_id)}`;
    f.store.map.delete(bindingKey);
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const readback = await readIntake(f, eventId);
    const readbackBody = await readback.text();
    expect(readback.status).toBe(200);
    expect(JSON.parse(readbackBody)).toMatchObject({ effect: { kind: "owned", state: "BOUND" } });
    expect(readbackBody).not.toContain("binding_intent");
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);

    await runNormalIntakeDrain(f.runtime);

    expect(f.store.map.get(bindingKey)).toContain(JSON.stringify(binding));
    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("complete");
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
  });

  it.each(["persistent", "preflight"] as const)("stops before credential preparation when BOUND sidecar KV is unavailable (%s)", async failureMode => {
    const f = fixture(); const eventId = "bound-sidecar-kv-outage"; const jobId = "8612";
    await enqueue(f, eventId, jobId);
    const { tuple, request, confirmed } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
    const bindingBase = { schema_version: 1 as const, provider: "cloudflare-container", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
    expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    const bindingKey = `containment:v1:effect-binding:acme/repo/${jobId}/intake/${encodeURIComponent(tuple.effect_id)}`;
    let bindingReads = 0;
    f.store.get.mockImplementation(async key => {
      if (key === bindingKey) {
        bindingReads++;
        if (failureMode === "persistent" || bindingReads === 2) {
          throw new Error(`binding KV unavailable during BOUND ${failureMode}`);
        }
      }
      return f.store.map.get(key) ?? null;
    });

    await runNormalIntakeDrain(f.runtime);

    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expect(f.startWithEnv).not.toHaveBeenCalled();
    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("pending");
  });

  it("recovers after the BOUND transaction commits but its response is lost", async () => {
    const f = fixture(); const eventId = "binding-intent-lost-bound-response"; const jobId = "8610";
    await enqueue(f, eventId, jobId);
    const { tuple } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const loseResponse = loseBoundResponseOnce(f);

    await runNormalIntakeDrain(f.runtime);

    const firstRecord = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string; next_attempt_ms: number };
    expect(firstRecord.state).toBe("pending");
    const attempt = f.d.storage.map.get(containmentSpawnAttemptKey(tuple)) as { state: string };
    expect(attempt.state).toBe("BOUND");
    expect(f.startWithEnv).not.toHaveBeenCalled();
    const readback = await readIntake(f, eventId);
    expect(readback.status).toBe(200);
    expect(await readback.json()).toMatchObject({ state: "pending", effect: { kind: "owned", state: "BOUND" } });

    loseResponse();
    await vi.advanceTimersByTimeAsync(Math.max(0, firstRecord.next_attempt_ms - Date.now()));
    await runNormalIntakeDrain(f.runtime);

    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("complete");
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
  });

  it("fails closed on a binding sidecar that disagrees with its durable binding", async () => {
    const f = fixture(); const eventId = "binding-sidecar-mismatch"; const jobId = "8613";
    await enqueue(f, eventId, jobId);
    const { tuple, request, confirmed } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
    const bindingBase = { schema_version: 1 as const, provider: "cloudflare-container", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
    expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    const replacementBase = { ...bindingBase, resource_id: `job:acme/repo/${jobId}-replacement` };
    const replacement = { ...replacementBase, binding_sha256: await digest(JSON.stringify(replacementBase)) };
    const bindingKey = `containment:v1:effect-binding:acme/repo/${jobId}/intake/${encodeURIComponent(tuple.effect_id)}`;
    f.store.map.set(bindingKey, JSON.stringify({ schema_version: 1, tuple, permit_id: confirmed.permit!.permit_id, binding: replacement }));

    const readback = await readIntake(f, eventId);
    expect(readback.status).toBe(503);
    await runNormalIntakeDrain(f.runtime);
    expect(f.startWithEnv).not.toHaveBeenCalled();
    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("uncertain");
  });

  it("rejects a persisted BOUND binding that does not match the requested drive before credential preparation", async () => {
    const f = fixture(); const eventId = "binding-intent-mismatch"; const jobId = "8609";
    await enqueue(f, eventId, jobId);
    const { tuple, request, confirmed } = await stageConfirmedIntakeOwner(f, eventId, jobId);
    const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
    // This is internally consistent BOUND evidence from an earlier invocation,
    // but it does not match the binding this route is about to drive.
    const bindingBase = { schema_version: 1 as const, provider: "previous-provider", resource_id: `job:acme/repo/${jobId}`, idempotency_key: tuple.effect_id };
    const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
    expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");

    await runNormalIntakeDrain(f.runtime);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expect(f.startWithEnv).not.toHaveBeenCalled();
    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("uncertain");
  });

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
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);
  });

  it("upgrades expired-drive uncertainty when the original live drive commits", async () => {
    const f = fixture(); const eventId = "late-live-drive-delivery"; const jobId = "8605";
    await enqueue(f, eventId, jobId);
    let releaseStart!: () => void;
    let markStartEntered!: () => void;
    const startBlocked = new Promise<void>(resolve => { releaseStart = resolve; });
    const startEntered = new Promise<void>(resolve => { markStartEntered = resolve; });
    f.startWithEnv.mockImplementationOnce(async () => { markStartEntered(); await startBlocked; });

    const firstDrain = runNormalIntakeDrain(f.runtime);
    await startEntered;
    vi.setSystemTime(Date.now() + 120_001);
    await runNormalIntakeDrain(f.runtime);

    const afterExpiry = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string };
    expect(afterExpiry.state).toBe("uncertain");
    expect(f.d.storage.map.get("normal-inbox:v1:count")).toBe(1);
    expect(await f.d.instance.normalIntakePending()).toHaveLength(0);

    releaseStart();
    await firstDrain;

    const record = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string };
    expect(record.state).toBe("complete");
    expect(f.d.storage.map.get("normal-inbox:v1:count")).toBe(0);
    expect(await f.d.instance.normalIntakePending()).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(1);
    expect(f.startWithEnv).toHaveBeenCalledTimes(1);

    await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "complete");
    await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "uncertain");
    await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "retry");
    expect((f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string }).state).toBe("complete");
    expect(f.d.storage.map.get("normal-inbox:v1:count")).toBe(0);
    expect(await f.d.instance.normalIntakePending()).toHaveLength(0);
    vi.clearAllTimers();
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
