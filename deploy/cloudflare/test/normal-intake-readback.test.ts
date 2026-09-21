import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import worker, { runNormalIntakeDrain } from "../src/index";
import { ContainmentEffectLedger } from "../src/containment_effect_ledger";
import { containmentSpawnActiveKey, containmentSpawnAttemptKey, containmentSpawnMirrorKey, intakeOwnerTuple } from "../src/containment_effect_route";
import { ctx, digest, env, kv, makeDO, ns } from "./containment-redrive-test-helpers";

const ADMIN = "normal-intake-readback-admin";
const BODY_SHA = "b".repeat(64);
const symbolExtraEffectDto = { kind: "missing", state: null, [Symbol("secret")]: "SENTINEL_secret_token" };
const customPrototypeEffectDto = Object.assign(Object.create({ token: "SENTINEL_secret_token" }), { kind: "missing", state: null });

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

async function seedConfirmedIntakeOwner(f: ReturnType<typeof fixture>, eventId: string) {
  const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
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

function readRequest(query = "", auth?: string): Request {
  return new Request(`https://worker/internal/v1/normal-intake${query}`, {
    headers: auth === undefined ? {} : { "x-corelink-internal-auth": auth },
  });
}

async function read(f: ReturnType<typeof fixture>, query: string, auth = ADMIN) {
  return worker.fetch(readRequest(query, auth), f.runtime, ctx() as never);
}

function countReadbackDoWrites(f: ReturnType<typeof fixture>) {
  let writes = 0;
  const transaction = f.d.storage.transaction.bind(f.d.storage);
  f.d.storage.transaction = ((fn: (storage: unknown) => Promise<unknown>) => transaction(storage => fn(new Proxy(storage, {
    get(target, property, receiver) {
      const value = Reflect.get(target, property, receiver);
      if (property === "put" || property === "delete") return async (...args: unknown[]) => {
        writes++;
        return value.apply(target, args);
      };
      return typeof value === "function" ? value.bind(target) : value;
    },
  })))) as typeof f.d.storage.transaction;
  return () => writes;
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

  it.each(["start proof", "binding", "mirror"] as const)("fails closed when an intake %s sidecar survives without owner records", async sidecar => {
    const f = fixture(); const eventId = `orphan-${sidecar.replaceAll(" ", "-")}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const activeKey = `containment:v1:spawn-active:acme/repo/8201/intake/${encodeURIComponent(tuple.effect_id)}`;
    const suffix = activeKey.slice("containment:v1:spawn-active:".length);
    if (sidecar === "start proof") f.d.storage.map.set(`containment:v1:effect-start:${suffix}`, { stale: true });
    else if (sidecar === "binding") f.store.map.set(`containment:v1:effect-binding:${suffix}`, "stale");
    else f.store.map.set(`containment:v1:spawn-mirror:${suffix}`, "stale");

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "orphan_sidecar" });
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
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "delivery_readback_unavailable" });
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
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable", reason: "permit" });
    expect(body).not.toContain(attempt.permit_id);
    expect(body).not.toContain(mismatchedPermitId);
    expect(body).not.toContain(attempt.caller_nonce);
    expect(body).not.toContain(attempt.tuple.token);
  });

  it.each(["absent", "unavailable"] as const)("uses the durable confirmation digest when a previously confirmed mirror is %s", async failure => {
    const f = fixture();
    const eventId = `confirmed-mirror-${failure}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const { tuple, mirror } = await seedConfirmedIntakeOwner(f, eventId);
    const attempt = f.d.storage.map.get(containmentSpawnAttemptKey(tuple)) as Record<string, unknown>;
    expect(attempt.mirror_digest).toBe(mirror.payload_digest);
    const mirrorKey = containmentSpawnMirrorKey(tuple);
    expect(f.store.map.get(mirrorKey)).toBe(mirror.payload);
    if (failure === "absent") f.store.map.delete(mirrorKey);
    else f.store.get.mockImplementation(async key => {
      if (key === mirrorKey) throw new Error("mirror KV unavailable after durable confirmation");
      return f.store.map.get(key) ?? null;
    });

    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);
    expect(response.status).toBe(200);
    const body = await response.json() as { effect: { kind: string; state: string } };
    expect(body.effect).toEqual({ kind: "owned", state: "PERMIT_ISSUED" });
  });

  it("rejects a changed canonical mirror when its durable confirmation digest is present", async () => {
    const f = fixture(); const eventId = "confirmed-mirror-digest-mismatch";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const { tuple, mirror } = await seedConfirmedIntakeOwner(f, eventId);
    const attempt = f.d.storage.map.get(containmentSpawnAttemptKey(tuple)) as Record<string, unknown>;
    expect(attempt.mirror_digest).toBe(mirror.payload_digest);
    const changed = JSON.parse(mirror.payload!) as { written_at_ms: number };
    changed.written_at_ms += 1;
    f.store.map.set(mirror.key, JSON.stringify(changed));

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "mirror_invalid" });
  });

  it("keeps legacy confirmed owners dependent on their live canonical mirror when no digest was recorded", async () => {
    const f = fixture(); const eventId = "legacy-confirmed-mirror-no-digest";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const { tuple, mirror } = await seedConfirmedIntakeOwner(f, eventId);
    const attemptKey = containmentSpawnAttemptKey(tuple);
    const activeKey = containmentSpawnActiveKey(tuple);
    const attempt = f.d.storage.map.get(attemptKey) as Record<string, unknown>;
    const pointer = f.d.storage.map.get(activeKey) as Record<string, unknown>;
    delete attempt.sidecar_version; delete pointer.sidecar_version;
    delete attempt.mirror_digest;
    delete pointer.mirror_digest;
    f.d.storage.map.set(attemptKey, attempt);
    f.d.storage.map.set(activeKey, pointer);
    const legacyMirrorKey = `containment:v1:spawn-mirror:acme/repo/8201/intake/${encodeURIComponent(tuple.effect_id)}`;
    f.store.map.delete(mirror.key);
    f.store.map.set(legacyMirrorKey, mirror.payload!);

    expect((await read(f, `?event_id=${encodeURIComponent(eventId)}`)).status).toBe(200);
    f.store.map.delete(legacyMirrorKey);
    const missingMirror = await read(f, `?event_id=${encodeURIComponent(eventId)}`);
    expect(missingMirror.status).toBe(503);
    expect(await missingMirror.json()).toEqual({ error: "normal intake readback unavailable", reason: "mirror_invalid" });
  });

  it("reads a pre-v2 owner from its legacy mirror key without inventing a durable digest", async () => {
    const f = fixture(); const eventId = "legacy-v1-mirror-fallback";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const { tuple, mirror } = await seedConfirmedIntakeOwner(f, eventId);
    const attemptKey = containmentSpawnAttemptKey(tuple);
    const activeKey = containmentSpawnActiveKey(tuple);
    const attempt = f.d.storage.map.get(attemptKey) as Record<string, unknown>;
    const pointer = f.d.storage.map.get(activeKey) as Record<string, unknown>;
    delete attempt.sidecar_version; delete attempt.mirror_digest;
    delete pointer.sidecar_version; delete pointer.mirror_digest;
    f.d.storage.map.set(attemptKey, attempt); f.d.storage.map.set(activeKey, pointer);
    f.store.map.delete(mirror.key);
    const legacyMirrorKey = `containment:v1:spawn-mirror:acme/repo/8201/intake/${encodeURIComponent(tuple.effect_id)}`;
    f.store.map.set(legacyMirrorKey, mirror.payload!);

    expect((await read(f, `?event_id=${encodeURIComponent(eventId)}`)).status).toBe(200);
    expect(f.d.storage.map.get(attemptKey)).not.toHaveProperty("mirror_digest");
    f.store.map.delete(legacyMirrorKey);
    expect((await read(f, `?event_id=${encodeURIComponent(eventId)}`)).status).toBe(503);
  });

  it("reads and regenerates an acquired owner mirror lost before durable confirmation", async () => {
    const f = fixture();
    const eventId = "unconfirmed-mirror-absent";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    expect((await f.d.instance.ownerPrepare(request)).kind).toBe("prepared");
    expect((await f.d.instance.ownerAcquire(request)).kind).toBe("acquired");
    const mirror = await f.d.instance.ownerMirror(request, "acquired");
    expect(mirror.kind).toBe("exact");
    f.store.map.delete(mirror.key);

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ effect: { kind: "owned", state: "CLAIM_ACQUIRED" } });
    const regenerated = await f.d.instance.ownerMirror(request, "acquired");
    expect(regenerated.kind).toBe("exact");
    expect(f.store.map.get(mirror.key)).toBe(regenerated.payload);
  });

  it("keeps legacy CLAIM_ACQUIRED owners fail-closed when their mirror is absent", async () => {
    const f = fixture();
    const eventId = "legacy-claim-acquired-mirror-absent";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    expect((await f.d.instance.ownerPrepare(request)).kind).toBe("prepared");
    expect((await f.d.instance.ownerAcquire(request)).kind).toBe("acquired");
    const attemptKey = containmentSpawnAttemptKey(tuple);
    const activeKey = containmentSpawnActiveKey(tuple);
    const attempt = f.d.storage.map.get(attemptKey) as Record<string, unknown>;
    const pointer = f.d.storage.map.get(activeKey) as Record<string, unknown>;
    delete attempt.sidecar_version;
    delete pointer.sidecar_version;
    f.d.storage.map.set(attemptKey, attempt);
    f.d.storage.map.set(activeKey, pointer);

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "mirror_invalid" });
  });

  it("reads V2 CLAIM_ACQUIRED without a mirror and lets the canonical mirror/confirm path resume", async () => {
    const f = fixture();
    const eventId = "v2-claim-acquired-before-mirror";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    expect((await f.d.instance.ownerPrepare(request)).kind).toBe("prepared");
    expect((await f.d.instance.ownerAcquire(request)).kind).toBe("acquired");
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const doWrites = countReadbackDoWrites(f);
    const kvPutCalls = f.store.put.mock.calls.length;
    const kvDeleteCalls = f.store.delete.mock.calls.length;

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ effect: { kind: "owned", state: "CLAIM_ACQUIRED" } });
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);
    expect(doWrites()).toBe(0);
    expect(f.store.put).toHaveBeenCalledTimes(kvPutCalls);
    expect(f.store.delete).toHaveBeenCalledTimes(kvDeleteCalls);

    const mirror = await f.d.instance.ownerMirror(request, "acquired");
    expect(mirror.kind).toBe("exact");
    const confirmed = await f.d.instance.ownerConfirm(
      { ...request, observation_kind: mirror.kind, observation_digest: mirror.payload_digest },
      mirror.payload_digest!, mirror.payload_digest!,
    );
    expect(confirmed.kind).toBe("permit_issued");
  });

  it.each(["malformed", "noncanonical"] as const)("rejects a present %s V2 mirror before permit issuance", async corruption => {
    const f = fixture();
    const eventId = `v2-claim-acquired-${corruption}-mirror`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    expect((await f.d.instance.ownerPrepare(request)).kind).toBe("prepared");
    expect((await f.d.instance.ownerAcquire(request)).kind).toBe("acquired");
    const mirror = await f.d.instance.ownerMirror(request, "acquired");
    expect(mirror.kind).toBe("exact");
    if (corruption === "malformed") f.store.map.set(mirror.key, "not-json");
    else {
      const payload = JSON.parse(mirror.payload!) as { written_at_ms: number };
      payload.written_at_ms += 1;
      f.store.map.set(mirror.key, JSON.stringify(payload));
    }

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "mirror_invalid" });
  });

  it.each(["BOUND", "DRIVING", "COMMITTED"] as const)("reads a V2 %s owner without its previously validated binding projection", async state => {
    const f = fixture();
    const eventId = `v2-${state.toLowerCase()}-binding-projection-missing`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
    if (state === "BOUND") {
      const { confirmed } = await seedConfirmedIntakeOwner(f, eventId);
      const started = await f.d.instance.ownerBegin(request, confirmed.permit!.permit_id);
      expect(started.proof).not.toBeNull();
      const bindingBase = {
        schema_version: 1 as const, provider: "cloudflare-container", resource_id: "job:acme/repo/8201", idempotency_key: tuple.effect_id,
      };
      const binding = { ...bindingBase, binding_sha256: await digest(JSON.stringify(bindingBase)) };
      expect((await f.d.instance.ownerBind(request, confirmed.permit!.permit_id, started.proof!.proof_id, binding)).kind).toBe("bound");
    } else if (state === "DRIVING") {
      await seedDrivingEffect(f, eventId);
    } else {
      await runNormalIntakeDrain(f.runtime);
    }
    const bindingSidecar = `containment:v1:effect-binding:acme/repo/8201/intake/${encodeURIComponent(tuple.effect_id)}`;
    expect(f.store.map.has(bindingSidecar)).toBe(true);
    f.store.map.delete(bindingSidecar);
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const doWrites = countReadbackDoWrites(f);
    const kvPutCalls = f.store.put.mock.calls.length;
    const kvDeleteCalls = f.store.delete.mock.calls.length;

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ effect: state === "COMMITTED"
      ? { kind: "committed", state }
      : { kind: "owned", state } });
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);
    expect(doWrites()).toBe(0);
    expect(f.store.put).toHaveBeenCalledTimes(kvPutCalls);
    expect(f.store.delete).toHaveBeenCalledTimes(kvDeleteCalls);
  });

  it.each(["mismatch", "unavailable"] as const)("keeps V2 DRIVING binding projection %s fail-closed", async failure => {
    const f = fixture();
    const eventId = `v2-driving-binding-${failure}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    await seedDrivingEffect(f, eventId);
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const bindingSidecar = `containment:v1:effect-binding:acme/repo/8201/intake/${encodeURIComponent(tuple.effect_id)}`;
    if (failure === "mismatch") f.store.map.set(bindingSidecar, "not-the-canonical-binding");
    else f.store.get.mockImplementation(async key => {
      if (key === bindingSidecar) throw new Error("binding KV unavailable");
      return f.store.map.get(key) ?? null;
    });

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: failure === "mismatch" ? "binding_divergent" : "binding_unavailable" });
  });

  it.each(["malformed", "tuple", "permit"] as const)("fails closed when an existing intake owner mirror is %s", async corruption => {
    const f = fixture(); const eventId = `mirror-${corruption}-mismatch`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    await seedDrivingEffect(f, eventId);
    await f.d.instance.normalIntakeSettle(eventId, BODY_SHA, "uncertain");
    const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
    const mirrorKey = containmentSpawnMirrorKey(tuple);
    const mirrorRaw = f.store.map.get(mirrorKey);
    expect(mirrorRaw).toBeDefined();
    if (corruption === "malformed") f.store.map.set(mirrorKey, "not-json");
    else {
      const mirror = JSON.parse(mirrorRaw!) as { tuple: { event_id: string }; permit_id: string | null };
      if (corruption === "tuple") mirror.tuple.event_id = "different-delivery";
      else mirror.permit_id = "divergent-permit-id";
      f.store.map.set(mirrorKey, JSON.stringify(mirror));
    }

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);

    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "normal intake readback unavailable", reason: "mirror_invalid" });
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

  it.each([
    ["owner_storage_unavailable", "sidecar read failure"],
    ["orphan_sidecar", "orphan sidecar secret"],
    ["owner_evidence", "missing owner pointer"],
    ["permit", "permit projection mismatch"],
    ["proof", "start proof mismatch"],
    ["binding_invalid", "binding payload invalid"],
    ["binding_unavailable", "binding KV unavailable"],
    ["binding_divergent", "binding projection divergent"],
    ["mirror_unavailable", "mirror KV unavailable"],
    ["mirror_invalid", "mirror payload invalid"],
    ["receipt", "receipt signature invalid"],
    ["delivery_readback_unavailable", "delivery storage unavailable"],
    ["tuple_unavailable", "owner tuple construction failure"],
    ["ledger_unexpected", "unexpected ledger exception"],
    ["effect_rpc_unavailable", "effect inspection RPC failure"],
  ] as const)("returns only the closed %s failure reason for %s", async (reason, failure) => {
    const f = fixture();
    const eventId = `reason-${reason}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));

    if (failure === "sidecar read failure") {
      f.store.get.mockRejectedValue(new Error("SENTINEL_secret_token"));
    } else if (failure === "orphan sidecar secret") {
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const suffix = containmentSpawnActiveKey(tuple).slice("containment:v1:spawn-active:".length);
      f.store.map.set(`containment:v1:spawn-mirror:${suffix}/${encodeURIComponent(tuple.caller_nonce)}`, "SENTINEL_secret_token");
    } else if (failure === "missing owner pointer") {
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
      await f.d.instance.ownerPrepare(request);
      f.d.storage.map.delete(containmentSpawnActiveKey(tuple));
    } else if (failure === "permit projection mismatch") {
      const { tuple } = await seedConfirmedIntakeOwner(f, eventId);
      const attemptKey = containmentSpawnAttemptKey(tuple);
      const attempt = structuredClone(f.d.storage.map.get(attemptKey)) as { permit: { permit_id: string } };
      attempt.permit.permit_id = "SENTINEL_secret_token";
      f.d.storage.map.set(attemptKey, attempt);
    } else if (failure === "start proof mismatch") {
      await seedDrivingEffect(f, eventId);
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const suffix = containmentSpawnActiveKey(tuple).slice("containment:v1:spawn-active:".length);
      f.d.storage.map.set(`containment:v1:effect-start:${suffix}`, { proof_id: "SENTINEL_secret_token" });
    } else if (failure === "binding payload invalid") {
      await seedDrivingEffect(f, eventId);
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const attemptKey = containmentSpawnAttemptKey(tuple);
      const attempt = structuredClone(f.d.storage.map.get(attemptKey)) as { binding: unknown };
      attempt.binding = null;
      f.d.storage.map.set(attemptKey, attempt);
    } else if (failure === "binding KV unavailable" || failure === "binding projection divergent") {
      await seedDrivingEffect(f, eventId);
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const suffix = containmentSpawnActiveKey(tuple).slice("containment:v1:spawn-active:".length);
      const key = `containment:v1:effect-binding:${suffix}`;
      if (failure === "binding KV unavailable") f.store.get.mockImplementation(async requested => {
        if (requested === key) throw new Error("SENTINEL_secret_token");
        return f.store.map.get(requested) ?? null;
      });
      else f.store.map.set(key, "SENTINEL_secret_token");
    } else if (failure === "mirror KV unavailable" || failure === "mirror payload invalid") {
      if (failure === "mirror KV unavailable") {
        const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
        const request = { schema_version: 1 as const, tuple, caller_nonce: tuple.caller_nonce };
        await f.d.instance.ownerPrepare(request);
        await f.d.instance.ownerAcquire(request);
        const mirrorKey = containmentSpawnMirrorKey(tuple);
        f.store.get.mockImplementation(async key => {
          if (key === mirrorKey) throw new Error("SENTINEL_secret_token");
          return f.store.map.get(key) ?? null;
        });
      } else {
        const { mirror } = await seedConfirmedIntakeOwner(f, eventId);
        f.store.map.set(mirror.key, "SENTINEL_secret_token");
      }
    } else if (failure === "receipt signature invalid") {
      await runNormalIntakeDrain(f.runtime);
      const tuple = await intakeOwnerTuple("acme/repo", "8201", `containment:v1:${eventId}`, eventId);
      const attemptKey = containmentSpawnAttemptKey(tuple);
      const attempt = structuredClone(f.d.storage.map.get(attemptKey)) as { effect_observation: { provider_signature: string } };
      attempt.effect_observation.provider_signature = "";
      f.d.storage.map.set(attemptKey, attempt);
    } else if (failure === "delivery storage unavailable") {
      vi.spyOn(f.d.instance, "normalIntakeInspect").mockRejectedValue(new Error("SENTINEL_secret_token"));
    } else if (failure === "owner tuple construction failure") {
      vi.spyOn(crypto.subtle, "digest").mockRejectedValueOnce(new Error("SENTINEL_secret_token"));
    } else if (failure === "unexpected ledger exception") {
      vi.spyOn(ContainmentEffectLedger.prototype, "inspectIntakeOwner").mockRejectedValue(new Error("SENTINEL_secret_token"));
    } else {
      vi.spyOn(f.d.instance, "normalIntakeEffectInspect").mockRejectedValue(new Error("SENTINEL_secret_token"));
    }

    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const doWrites = countReadbackDoWrites(f);
    const kvPutCalls = f.store.put.mock.calls.length;
    const kvDeleteCalls = f.store.delete.mock.calls.length;
    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);
    const body = await response.text();

    expect(response.status).toBe(503);
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable", reason });
    expect(body).not.toContain("SENTINEL_secret_token");
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);
    expect(doWrites()).toBe(0);
    expect(f.store.put).toHaveBeenCalledTimes(kvPutCalls);
    expect(f.store.delete).toHaveBeenCalledTimes(kvDeleteCalls);
  });

  it("does not expose a reason for missing authentication and attributes canonical DO unexpected", async () => {
    const f = fixture();
    const eventId = "reason-auth-and-fallback";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const missingAuth = await worker.fetch(readRequest(`?event_id=${eventId}`), f.runtime, ctx() as never);
    expect(missingAuth.status).toBe(401);
    expect(await missingAuth.json()).toEqual({ error: "unauthorized" });

    vi.spyOn(f.d.instance, "normalIntakeEffectInspect").mockResolvedValue({
      kind: "unavailable", reason: "unexpected",
    } as never);
    const response = await read(f, `?event_id=${eventId}`);
    expect(response.status).toBe(503);
    const body = await response.text();
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable", reason: "effect_do_unexpected" });
    expect(body).not.toContain("SENTINEL_secret_token");
  });

  it("maps an exception after effect RPC in the response boundary without exposing it", async () => {
    const f = fixture();
    const eventId = "readback-response-boundary-failure";
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    const record = await f.d.instance.normalIntakeInspect(eventId);
    expect(record).not.toBeNull();
    Object.defineProperty(record!, "state", { get() { throw new Error("SENTINEL_secret_token"); } });

    vi.spyOn(f.d.instance, "normalIntakeInspect").mockResolvedValue(record);
    const response = await read(f, `?event_id=${eventId}`);

    expect(response.status).toBe(503);
    const body = await response.text();
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable", reason: "readback_response_unavailable" });
    expect(body).not.toContain("SENTINEL_secret_token");
  });

  it.each([
    ["unknown kind", { kind: "SENTINEL_secret_token", state: null }],
    ["malformed owned state", { kind: "owned", state: "SENTINEL_secret_token" }],
    ["owned committed state", { kind: "owned", state: "COMMITTED" }],
    ["extra success field", { kind: "missing", state: null, token: "SENTINEL_secret_token" }],
    ["symbol extra field", symbolExtraEffectDto],
    ["custom prototype", customPrototypeEffectDto],
    ["extra unavailable field", { kind: "unavailable", reason: "owner_evidence", token: "SENTINEL_secret_token" }],
    ["unknown unavailable reason", { kind: "unavailable", reason: "SENTINEL_secret_token" }],
  ] as const)("rejects malformed readback DTO: %s", async (_caseName, dto) => {
    const f = fixture();
    const eventId = `malformed-dto-${_caseName.replaceAll(" ", "-")}`;
    await f.d.instance.normalIntakeEnqueue(intake(eventId));
    vi.spyOn(f.d.instance, "normalIntakeEffectInspect").mockResolvedValue(dto as never);
    const beforeStorage = structuredClone([...f.d.storage.map.entries()]);
    const beforeKv = structuredClone([...f.store.map.entries()]);
    const doWrites = countReadbackDoWrites(f);
    const kvPutCalls = f.store.put.mock.calls.length;
    const kvDeleteCalls = f.store.delete.mock.calls.length;

    const response = await read(f, `?event_id=${encodeURIComponent(eventId)}`);
    const body = await response.text();

    expect(response.status).toBe(503);
    expect(JSON.parse(body)).toEqual({ error: "normal intake readback unavailable", reason: "effect_dto_invalid" });
    expect(body).not.toContain("SENTINEL_secret_token");
    expect([...f.d.storage.map.entries()]).toEqual(beforeStorage);
    expect([...f.store.map.entries()]).toEqual(beforeKv);
    expect(doWrites()).toBe(0);
    expect(f.store.put).toHaveBeenCalledTimes(kvPutCalls);
    expect(f.store.delete).toHaveBeenCalledTimes(kvDeleteCalls);
  });
});
