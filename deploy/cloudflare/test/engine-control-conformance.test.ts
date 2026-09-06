import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

const containers: Array<Record<string, unknown>> = [];
const RUNNER_NS = { kind: "runner" };
const CHECK_NS = { kind: "check" };

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((ns: unknown, handle: string) => {
    const container = {
      ns,
      handle,
      start: vi.fn(async () => {}),
      startWithEnv: vi.fn(async () => {}),
      containerFetch: vi.fn(async () => new Response(JSON.stringify({ exit_code: 0, stdout: "", stderr: "" }), { status: 200 })),
      isAlive: vi.fn(async () => true),
      teardown: vi.fn(async () => {}),
      cutEgress: vi.fn(async () => {}),
    };
    containers.push(container);
    return container;
  }),
}));

import worker from "../src/index";
import type { Env } from "../src/index";

type Fixture = {
  schema_version: number;
  requests: Array<{
    operation: "spawn" | "exec" | "teardown";
    method: string;
    path: string;
    headers: Record<string, string>;
    body: string | null;
  }>;
};

const fixture = JSON.parse(readFileSync(new URL("../../../conformance/engine-worker-control-requests.json", import.meta.url), "utf8")) as Fixture;

function env(): Env {
  const authorities = makeWorkerAuthorities(undefined);
  return {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "fixture-spawn-secret",
    CLOUDFLARE_EXEC_AUTH_TOKEN: "fixture-exec-secret",
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "fixture-lifecycle-secret",
    EXEC_SERVER_AUTH_TOKEN: "fixture-exec-server-secret",
    PINNED_IMAGE_DIGEST: "",
    CONTAINMENT: authorities.CONTAINMENT as never,
    CONCURRENCY_SLOTS: authorities.CONCURRENCY_SLOTS as never,
  } as Env;
}

function requestFor(item: Fixture["requests"][number], authorization = item.headers.Authorization): Request {
  const headers = { ...item.headers, Authorization: authorization };
  return new Request(`https://worker.test${item.path}`, { method: item.method, headers, body: item.body ?? undefined });
}

describe("Engine → Worker control request conformance", () => {
  it("accepts the three captured Engine requests with the expected statuses", async () => {
    expect(fixture.schema_version).toBe(1);
    expect(fixture.requests.map((item) => item.operation)).toEqual(["spawn", "exec", "teardown"]);
    const expected = { spawn: 201, exec: 200, teardown: 204 } as const;
    for (const item of fixture.requests) {
      containers.length = 0;
      const response = await worker.fetch(requestFor(item), env());
      expect(response.status, item.operation).toBe(expected[item.operation]);
      expect(containers.length, item.operation).toBeGreaterThan(0);
    }
  });

  it("rejects each captured request with either other domain token before container effects", async () => {
    const tokens = fixture.requests.map((item) => item.headers.Authorization);
    for (const item of fixture.requests) {
      for (const wrong of tokens.filter((token) => token !== item.headers.Authorization)) {
        containers.length = 0;
        const response = await worker.fetch(requestFor(item, wrong), env());
        expect(response.status, `${item.operation} with wrong domain`).toBe(401);
        expect(containers, `${item.operation} with wrong domain`).toHaveLength(0);
      }
    }
  });
});
