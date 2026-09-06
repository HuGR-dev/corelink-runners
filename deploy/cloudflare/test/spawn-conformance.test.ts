// TS-side conformance golden for the spawn-Worker's /v1/spawn SpawnBody shape.
//
// The committed vector conformance/cloudflare-spawn.json is the drift tripwire
// for the fabricd↔Worker spawn seam (ADR-0008). Today it is enforced on the RUST
// side ONLY — a TS field rename on this side would ship green while breaking the
// live wire. This binds the TS `/v1/spawn` handler to the SAME committed vector
// so the CLAUDE.md law holds symmetrically: "either side's golden tests break on
// any type divergence, so a difference is never silent."
//
// Two-directional tripwire:
//   • rename/remove a required TS SpawnBody field ⇒ the handler 400s (or drops
//     the field) instead of 201-accepting the vector ⇒ this test breaks.
//   • add a key to the vector that TS doesn't model ⇒ the key-set assertion breaks.
//
// NEW FILE (W4). Does NOT touch test/index.test.ts or test/check-host.test.ts.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── @cloudflare/containers double so the handler's runner spawn is observable ──
interface FakeContainer {
  ns: unknown;
  handle: string;
  startWithEnv: ReturnType<typeof vi.fn>;
  start: ReturnType<typeof vi.fn>;
  containerFetch: ReturnType<typeof vi.fn>;
  isAlive: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
  cutEgress: ReturnType<typeof vi.fn>;
}
let containers: FakeContainer[] = [];
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
    const c: FakeContainer = {
      ns,
      handle,
      startWithEnv: vi.fn(async () => {}),
      start: vi.fn(async () => {}),
      containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
      isAlive: vi.fn(async () => true),
      teardown: vi.fn(async () => {}),
      cutEgress: vi.fn(async () => {}),
    };
    containers.push(c);
    return c;
  }),
}));

import worker, { type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";

const RUNNER_NS = { _ns: "runner" };
const AUTH = "spawn-secret";

// The committed cross-repo vector (repo root: conformance/cloudflare-spawn.json).
const VECTOR_PATH = fileURLToPath(
  new URL("../../../conformance/cloudflare-spawn.json", import.meta.url),
);
const vector = JSON.parse(readFileSync(VECTOR_PATH, "utf8")) as {
  request: Record<string, unknown>;
  response: { handle: string };
};

// The keys the TS SpawnBody type models (src/index.ts `interface SpawnBody`).
// A vector key outside this set means the vector drifted ahead of the TS type.
const KNOWN_SPAWNBODY_KEYS = new Set([
  "image_digest",
  "jitconfig",
  "env",
  "labels",
  "expiry_ms",
  "mode",
  "toolchain_digest",
]);
// The keys a valid runner-mode SpawnBody MUST carry (the vector is a runner spawn).
const REQUIRED_KEYS = ["image_digest", "jitconfig", "env", "labels", "expiry_ms"];

function envWith(over: Partial<Env> = {}): Env {
  const env = {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: { _ns: "check" } as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: AUTH,
    CLOUDFLARE_EXEC_AUTH_TOKEN: "exec-control-secret",
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle-control-secret",
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  if (!over.CONCURRENCY_SLOTS) env.CONCURRENCY_SLOTS = authorities.CONCURRENCY_SLOTS as never;
  return env;
}

beforeEach(() => {
  containers = [];
  vi.mocked(getContainer).mockClear();
});

describe("conformance: /v1/spawn SpawnBody ↔ conformance/cloudflare-spawn.json", () => {
  it("the committed vector's request keys are all modeled by the TS SpawnBody type", () => {
    for (const k of Object.keys(vector.request)) {
      expect(KNOWN_SPAWNBODY_KEYS.has(k)).toBe(true);
    }
    // ...and every required field is present in the vector.
    for (const k of REQUIRED_KEYS) {
      expect(vector.request).toHaveProperty(k);
    }
  });

  it("the TS /v1/spawn handler ACCEPTS the committed vector byte-for-byte (201 {handle})", async () => {
    const resp = await worker.fetch(
      new Request("https://w/v1/spawn", {
        method: "POST",
        headers: { authorization: `Bearer ${AUTH}`, "content-type": "application/json" },
        // The vector's request object, unmodified — a TS field rename would make
        // this fail the handler's guards (400/409) instead of 201-accepting it.
        body: JSON.stringify(vector.request),
      }),
      envWith(),
    );
    expect(resp.status).toBe(201);
    const j = (await resp.json()) as { handle: string };
    expect(typeof j.handle).toBe("string"); // shape parity with vector.response.handle

    // The runner DO was started with the vector's `env` verbatim — this consumes
    // the SpawnBody.env field, so a rename of `env` on the TS side breaks here.
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(RUNNER_NS);
    expect(containers[0].startWithEnv).toHaveBeenCalledWith(vector.request.env);
  });

  it("the vector's image_digest satisfies the handler's content-pin guard (@sha256:)", async () => {
    // The guard that a TS rename of image_digest would trip: the handler 400s a
    // request whose image_digest is not a @sha256:-pinned string. The vector must
    // pass it — so this is a live assertion on the vector↔guard contract.
    expect(String(vector.request.image_digest)).toContain("@sha256:");
    // And a request whose image_digest key is MISSING is rejected — proving the
    // field name is load-bearing (rename ⇒ absent ⇒ 400, not a silent accept).
    const { image_digest: _omit, ...withoutDigest } = vector.request;
    const resp = await worker.fetch(
      new Request("https://w/v1/spawn", {
        method: "POST",
        headers: { authorization: `Bearer ${AUTH}`, "content-type": "application/json" },
        body: JSON.stringify(withoutDigest),
      }),
      envWith(),
    );
    expect(resp.status).toBe(400);
    expect(containers).toHaveLength(0);
  });
});
