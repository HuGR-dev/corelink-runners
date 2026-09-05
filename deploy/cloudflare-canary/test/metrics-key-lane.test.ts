import { afterEach, describe, expect, it, vi } from "vitest";
import { runCycle, type Env } from "../src/index";

/**
 * These are local/untrusted capability fixtures only. They do not sign a
 * human page acknowledgement, touch a deployed route, or advance production
 * state. The fake service binding models the worker's actual
 * X-Corelink-Internal-Auth contract (not an invented Authorization shape).
 */
const CURRENT_KEY = "fixture-current-metrics-key";
const PRIOR_KEY = "fixture-prior-metrics-key";
const RESEND_KEY = "fixture-resend-key";
const METRICS = { counters: { webhook_job_completed: 7, spawn_failed: 0 } };
const NOW = Date.UTC(2026, 8, 1, 12, 0, 0);

function localKv(): KVNamespace {
  const values = new Map<string, string>();
  return {
    get: vi.fn(async (key: string, type?: "text" | "json") => {
      const raw = values.get(key);
      if (raw === undefined) return null;
      return type === "json" ? JSON.parse(raw) : raw;
    }),
    put: vi.fn(async (key: string, value: string) => {
      values.set(key, value);
    }),
  } as unknown as KVNamespace;
}

function fabricMustStayAsleep(): Fetcher {
  return {
    fetch: vi.fn(async () => {
      throw new Error("fabric probe was attempted while FABRIC_PROBES_ENABLED=0");
    }),
  } as unknown as Fetcher;
}

function namedSpawnService() {
  const calls: Array<{ key: string; url: string }> = [];
  const service = {
    fetch: vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      const key = headers.get("X-Corelink-Internal-Auth") ?? "";
      calls.push({ key, url: String(input) });
      // This is the actual worker route's behavior: current key → 200 body;
      // stale/prior key → 401, with no production endpoint involved.
      if (key === CURRENT_KEY) return new Response(JSON.stringify(METRICS), { status: 200 });
      return new Response(JSON.stringify({ error: "unauthorized" }), { status: 401 });
    }),
  } as unknown as Fetcher;
  return { service, calls };
}

function env(spawn: Fetcher, key: string, fabric: Fetcher): Env {
  return {
    CANARY_KV: localKv(),
    FABRICD_SVC: fabric,
    SPAWN_SVC: spawn,
    FABRIC_PROBES_ENABLED: "0",
    FABRIC_OBSERVABILITY_KEY: "fixture-fabric-key",
    METRICS_OBSERVABILITY_KEY: key,
    SPAWN_METRICS_URL: "https://fixture.local/internal/v1/metrics",
    RESEND_API_KEY: RESEND_KEY,
    ALERT_EMAIL_TO: "owner@fixture.invalid",
    ALERT_EMAIL_FROM: "canary@fixture.invalid",
    ALERT_COOLDOWN_MINUTES: "30",
  };
}

describe("T6-W13 metrics-key lane (offline local capability fixtures)", () => {
  afterEach(() => vi.restoreAllMocks());

  it("uses the current key and keeps fabric status/health at zero across 12 cycles", async () => {
    const { service, calls } = namedSpawnService();
    const fabric = fabricMustStayAsleep();
    const resend = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", resend);
    const canaryEnv = env(service, CURRENT_KEY, fabric);

    for (let cycle = 0; cycle < 12; cycle += 1) {
      const result = await runCycle(canaryEnv, NOW + cycle * 60_000);
      expect(result).toContain("config=valid");
    }

    expect(calls).toHaveLength(12);
    expect(calls.every(({ key }) => key === CURRENT_KEY)).toBe(true);
    expect(calls.every(({ url }) => url.endsWith("/internal/v1/metrics"))).toBe(true);
    expect(fabric.fetch).not.toHaveBeenCalled();
    expect(resend).not.toHaveBeenCalled();
  });

  it("warns and attempts one stale-key delivery across 12 cycles, preserving cooldown and key secrecy", async () => {
    const { service, calls } = namedSpawnService();
    const fabric = fabricMustStayAsleep();
    const resendRequests: RequestInit[] = [];
    const logs: string[] = [];
    const resend = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      resendRequests.push(init ?? {});
      return new Response("{}", { status: 200 });
    });
    const log = vi.spyOn(console, "log").mockImplementation((...args) => logs.push(args.join(" ")));
    vi.stubGlobal("fetch", resend);
    const canaryEnv = env(service, PRIOR_KEY, fabric);

    for (let cycle = 0; cycle < 12; cycle += 1) {
      const result = await runCycle(canaryEnv, NOW + cycle * 60_000);
      expect(result).toContain("config=valid");
    }

    expect(calls).toHaveLength(12);
    expect(calls.every(({ key }) => key === PRIOR_KEY)).toBe(true);
    expect(resend).toHaveBeenCalledTimes(1); // stale-key WARN is cooldown-deduplicated
    expect(resendRequests[0]?.headers).toEqual({
      Authorization: `Bearer ${RESEND_KEY}`,
      "content-type": "application/json",
    });
    const delivered = JSON.parse(String(resendRequests[0]?.body));
    expect(delivered.subject).toContain("WARN");
    expect(delivered.text).toContain("auth:spawn");
    expect(logs.join("\n")).not.toContain(CURRENT_KEY);
    expect(logs.join("\n")).not.toContain(PRIOR_KEY);
    expect(logs.join("\n")).not.toContain(RESEND_KEY);
    expect(fabric.fetch).not.toHaveBeenCalled();
    expect(log).toHaveBeenCalled();
  });
});
