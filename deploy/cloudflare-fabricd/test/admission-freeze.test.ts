// Worker edge admission freeze. The switch must stop new lease-producing and
// ticket-minting requests before a shard/container lookup, while allowing
// existing lease lifecycle traffic to drain.

import { beforeEach, describe, expect, it, vi } from "vitest";

const getContainer = vi.fn();
vi.mock("@cloudflare/containers", () => ({
  getContainer: (...args: unknown[]) => getContainer(...args),
  Container: class {
    ctx: unknown;
    env: unknown;
    constructor(ctx?: unknown, env?: unknown) {
      this.ctx = ctx;
      this.env = env;
    }
  },
}));

import worker, {
  admissionPaused,
  isNewAdmissionRoute,
  type Env,
} from "../src/index";

function envWithPause(value?: string): Env {
  return {
    FABRICD: {} as Env["FABRICD"],
    CORELINK_INTROSPECT_URL: "https://introspect.example",
    FABRIC_SIGNING_KEY: "k",
    FABRIC_INTROSPECT_AUTH_KEY: "k",
    ...(value === undefined ? {} : { FABRIC_ADMISSION_PAUSED: value }),
  } as Env;
}

beforeEach(() => {
  getContainer.mockReset();
  getContainer.mockImplementation(() => ({
    fetch: () => Promise.resolve(new Response("forwarded", { status: 200 })),
  }));
});

describe("FABRIC_ADMISSION_PAUSED parsing and route scope", () => {
  it("fails open only when absent or exactly 0", () => {
    expect(admissionPaused(envWithPause())).toBe(false);
    expect(admissionPaused(envWithPause("0"))).toBe(false);
    expect(admissionPaused(envWithPause("1"))).toBe(true);
    expect(admissionPaused(envWithPause(" "))).toBe(true);
    expect(admissionPaused(envWithPause("true"))).toBe(true);
  });

  it("classifies only new lease producers and the explicit ticket mint", () => {
    expect(isNewAdmissionRoute("POST", "/v1/leases")).toBe(true);
    expect(isNewAdmissionRoute("POST", "/webhooks/github")).toBe(true);
    expect(isNewAdmissionRoute("POST", "/v1/test/mint-cred-ticket")).toBe(true);

    expect(isNewAdmissionRoute("GET", "/v1/leases")).toBe(false);
    expect(isNewAdmissionRoute("GET", "/v1/leases/lease-1")).toBe(false);
    expect(isNewAdmissionRoute("POST", "/v1/leases/lease-1/cas-cred")).toBe(false);
    expect(isNewAdmissionRoute("POST", "/v1/leases/lease-1/close")).toBe(false);
    expect(isNewAdmissionRoute("POST", "/v1/leases/lease-1/cancel")).toBe(false);
  });
});

describe("paused Worker", () => {
  it.each([
    ["direct acquire", "/v1/leases", "{}"],
    ["autoscaler webhook", "/webhooks/github", "{}"],
    ["test ticket mint", "/v1/test/mint-cred-ticket", "{}"],
  ])("rejects %s before a container lookup", async (_name, path, body) => {
    const response = await worker.fetch(
      new Request(`http://fabricd${path}`, { method: "POST", body }),
      envWithPause("1"),
    );

    expect(response.status).toBe(503);
    expect(response.headers.get("retry-after")).toBe("60");
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(response.headers.get("content-type")).toContain("application/json");
    expect(await response.json()).toEqual({ error: "fabric admission paused" });
    expect(getContainer).not.toHaveBeenCalled();
  });

  it.each([
    ["health", "GET", "/v1/health"],
    ["attestation key", "GET", "/v1/attestation/key"],
    ["lease list", "GET", "/v1/leases"],
    ["lease status", "GET", "/v1/leases/lease-1"],
    ["credential redemption", "POST", "/v1/leases/lease-1/cas-cred"],
    ["cancel and teardown", "POST", "/v1/leases/lease-1/cancel"],
    ["close and teardown", "POST", "/v1/leases/lease-1/close"],
  ])("forwards existing/liveness route: %s", async (_name, method, path) => {
    const response = await worker.fetch(
      new Request(`http://fabricd${path}`, { method, body: method === "POST" ? "{}" : undefined }),
      envWithPause("1"),
    );

    expect(response.status).toBe(200);
    expect(getContainer).toHaveBeenCalledTimes(1);
  });
});

describe("open Worker", () => {
  it.each([undefined, "0"])("forwards acquire when FABRIC_ADMISSION_PAUSED=%j", async (value) => {
    const response = await worker.fetch(
      new Request("http://fabricd/v1/leases", { method: "POST", body: "{}" }),
      envWithPause(value),
    );

    expect(response.status).toBe(200);
    expect(getContainer).toHaveBeenCalledTimes(1);
  });
});
