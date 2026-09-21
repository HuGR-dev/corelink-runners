import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import worker, { runNormalIntakeDrain } from "../src/index";
import { intakeOwnerTuple } from "../src/containment_effect_route";
import { ctx, digest, env, kv, makeDO, ns } from "./containment-redrive-test-helpers";

const ADMIN = "normal-intake-readback-admin";
const BODY_SHA = "b".repeat(64);

function intake(event_id: string) {
  return {
    schema_version: 1 as const, event_id, body_sha256: BODY_SHA, job_id: "8201", repo: "acme/repo",
    installation_id: "42", labels: ["private-label"], received_at_ms: 1_750_000_000_000,
  };
}

function fixture() {
  const d = makeDO();
  const store = kv();
  const runtime = env(d, store, {
    CONTAINMENT_ADMIN_KEY: ADMIN,
    CORELINK_RUNNER_MINT_AUTH_KEY: "mint-auth",
    CORELINK_MINT_URL: "https://mint.example",
    SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
    CONCURRENCY_SLOTS: ns({ acquire: vi.fn(async () => ({ admitted: true })), release: vi.fn(async () => {}) }),
  });
  // The normal-intake drain takes its canonical owner path while all external
  // services remain deterministic local stubs.
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/runner/authorize")) return Response.json({ tenant: "tenant-a", max_concurrency: 2 });
    if (url.endsWith("/runner/mint")) {
      const requestBody = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: string };
      const operationId = requestBody.operation_id ?? "operation";
      return Response.json({ operation_id: operationId, tenant: "tenant-a", lifecycle_generation: "1", max_concurrency: 2, pat_id: `pat-${operationId}`, token_plaintext: `secret-${operationId}` });
    }
    if (url.endsWith("/runner/adopt")) return new Response(null, { status: 204 });
    if (url.includes("generate-jitconfig")) return Response.json({ encoded_jit_config: "jit", runner: { id: 5 } });
    if (url.endsWith("/runner/revoke")) return new Response(null, { status: 204 });
    throw new Error(`unexpected external URL: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.mocked(getContainer).mockReturnValue({
    startWithEnv: vi.fn(async () => {}),
    teardown: vi.fn(async () => {}),
  } as never);
  return { d, store, runtime, fetchMock };
}

async function seedDrivingEffect(f: ReturnType<typeof fixture>, eventId: string) {
  const effectId = `containment:v1:${eventId}`;
  const tuple = await intakeOwnerTuple("acme/repo", "8201", effectId, eventId);
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
  expect(started.proof).not.toBeNull();
  const bindingBase = {
    schema_version: 1 as const, provider: "cloudflare-container", resource_id: "job:acme/repo/8201", idempotency_key: effectId,
  };
  const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
  const bound = await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding);
  expect(bound.kind).toBe("bound");
  expect((await f.d.instance.ownerMarkDriving(request, confirmed.permit!.permit_id, started.proof!.proof_id)).kind).toBe("driving");
}

function readRequest(query = "", auth?: string): Request {
  return new Request(`https://worker/internal/v1/normal-intake${query}`, {
    headers: auth === undefined ? {} : { "x-corelink-internal-auth": auth },
  });
}

async function read(f: ReturnType<typeof fixture>, query: string, auth = ADMIN) {
  return worker.fetch(readRequest(query, auth), f.runtime, ctx() as never);
}

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

describe("GET /internal/v1/normal-intake", () => {
  it("is invisible when CONTAINMENT_ADMIN_KEY is unset and rejects a wrong key", async () => {
    const f = fixture();
    expect((await worker.fetch(readRequest("?event_id=delivery-1", ADMIN), env(f.d, f.store), ctx() as never)).status).toBe(404);
    expect((await read(f, "?event_id=delivery-1", "wrong")).status).toBe(401);
  });

  it("returns 401 when the key is configured but the header is missing", async () => {
    const f = fixture();
    expect((await worker.fetch(readRequest("?event_id=delivery-1"), f.runtime, ctx() as never)).status).toBe(401);
  });

  it.each([
    ["missing event_id", ""],
    ["empty event_id", "?event_id="],
    ["duplicate event_id", "?event_id=one&event_id=two"],
    ["an extra parameter", "?event_id=one&debug=1"],
    ["a control character", "?event_id=%00"],
  ])("rejects %s with 400", async (_label, query) => {
    const f = fixture();
    await f.d.instance.normalIntakeEnqueue(intake("unchanged-delivery"));
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeMirror = structuredClone([...f.store.map.entries()]);
    const response = await read(f, query);

    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeMirror);
    expect(response.status).toBe(400);
  });

  it("returns 404 for a well-formed lookup with no durable delivery", async () => {
    const f = fixture();
    expect((await read(f, "?event_id=absent-delivery")).status).toBe(404);
  });

  it("fails closed when the stored delivery record is malformed", async () => {
    const f = fixture();
    f.d.storage.map.set("normal-inbox:v1:event:corrupt-delivery", { schema_version: 1, event_id: "corrupt-delivery", body_sha256: "not-a-digest" });
    f.store.map.set("spawn:readback-snapshot", "unchanged");
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeMirror = structuredClone([...f.store.map.entries()]);

    const response = await read(f, "?event_id=corrupt-delivery");

    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeMirror);
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable" });
  });

  it("fails closed when canonical attempt permit evidence disagrees with its projection", async () => {
    const f = fixture();
    const eventId = "permit-evidence-mismatch";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    await seedDrivingEffect(f, eventId);
    await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "uncertain");

    const attemptEntry = [...f.d.storage.map.entries()].find(([key]) =>
      key.startsWith("containment:v1:spawn-attempt:acme/repo/8201/intake/"));
    expect(attemptEntry).toBeDefined();
    const [attemptKey, rawAttempt] = attemptEntry!;
    const attempt = structuredClone(rawAttempt) as {
      permit_id: string; permit: { permit_id: string }; caller_nonce: string; tuple: { token: string };
    };
    expect(attempt.permit.permit_id).toBe(attempt.permit_id);
    const mismatchedPermitId = "nested-permit-id-mismatch-secret";
    attempt.permit.permit_id = mismatchedPermitId;
    f.d.storage.map.set(attemptKey, attempt);

    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeMirror = structuredClone([...f.store.map.entries()]);
    const response = await read(f, `?event_id=${eventId}`);

    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeMirror);
    expect(response.status).toBe(503);
    const body = await response.text();
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable" });
    expect(body).not.toContain(attempt.permit_id);
    expect(body).not.toContain(mismatchedPermitId);
    expect(body).not.toContain(attempt.caller_nonce);
    expect(body).not.toContain(attempt.tuple.token);
  });

  it.each([
    ["pending", "pending"],
    ["uncertain", "uncertain"],
    ["complete", "complete"],
  ] as const)("returns a sanitized read-only %s record and canonical effect state", async (label, expectedState) => {
    const f = fixture();
    const eventId = `readback-${label}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    if (label === "uncertain") {
      await seedDrivingEffect(f, eventId);
      await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "uncertain");
    }
    if (label === "complete") await runNormalIntakeDrain(f.runtime);

    const stored = f.d.storage.map.get(`normal-inbox:v1:event:${eventId}`) as { state: string; received_at_ms: number; next_attempt_ms: number };
    expect(stored.state).toBe(expectedState);
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeMirror = structuredClone([...f.store.map.entries()]);

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(200);
    const body = await response.json() as Record<string, unknown> & { effect: Record<string, unknown> };
    expect(Object.keys(body).sort()).toEqual([
      "effect", "event_id", "installation_id", "job_id", "next_attempt_ms", "received_at_ms", "repo", "schema_version", "state",
    ]);
    expect(body).toMatchObject({
      schema_version: 1, event_id: eventId, job_id: "8201", repo: "acme/repo", installation_id: "42", state: expectedState,
      received_at_ms: stored.received_at_ms, next_attempt_ms: stored.next_attempt_ms,
    });
    expect(Object.keys(body.effect).sort()).toEqual(["kind", "state"]);
    const expectedEffect = label === "pending"
      ? { kind: "missing", state: null }
      : label === "uncertain"
        ? { kind: "owned", state: "DRIVING" }
        : { kind: "committed", state: "COMMITTED" };
    expect(body.effect).toEqual(expectedEffect);
    expect(JSON.stringify(body)).not.toContain(BODY_SHA);
    expect(JSON.stringify(body)).not.toContain("private-label");
    expect(JSON.stringify(body)).not.toContain("secret-");
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeMirror);
  });
});
