// W1b singleton-resilience hardening — two units:
//   (1) watchdogAction — the pure boot-grace / reboot-backoff decision the cron
//       watchdog uses to decide whether to destroy() a container. It must NOT
//       destroy a still-cold-booting container (never-yet-healthy within grace)
//       nor thrash a just-rebooted one, while still reclaiming a genuine hang
//       (previously-healthy, went dark).
//   (2) proxyFetch (via worker.fetch) — a wedged upstream on a NON-long-lived
//       route fails the client fast with a structured 503; a long-lived route
//       (exec/close/trigger) is EXEMPT and its error propagates unbounded.
//
// Same harness as the sibling tests: `@cloudflare/containers` is mocked so
// `getContainer` is a spy; `cloudflare:workers` is aliased to a node stub by
// vitest.config.ts.

import { describe, expect, it, vi, beforeEach } from "vitest";

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
  isLongLivedRoute,
  watchdogAction,
  type Env,
  type WatchdogEntry,
} from "../src/index";

function envWithShards(n: number): Env {
  return {
    FABRICD: {} as Env["FABRICD"],
    FABRIC_NUM_SHARDS: String(n),
    CORELINK_INTROSPECT_URL: "https://introspect.example",
    FABRIC_SIGNING_KEY: "k",
    FABRIC_INTROSPECT_AUTH_KEY: "k",
  } as Env;
}

beforeEach(() => {
  getContainer.mockReset();
});

// ─────────────────────────── watchdogAction (boot-grace) ───────────────────────
describe("watchdogAction — boot-grace + reboot-backoff decision", () => {
  const CFG = { bootGraceMs: 180_000, rebootBackoffMs: 180_000 };
  const fresh = (over: Partial<WatchdogEntry> = {}): WatchdogEntry => ({
    firstSeenAt: 1_000_000,
    firstHealthyAt: null,
    lastDestroyAt: null,
    ...over,
  });

  it("healthy probe records firstHealthyAt and clears any reboot backoff", () => {
    const entry = fresh({ firstSeenAt: 1_000_000, lastDestroyAt: 999_000 });
    const action = watchdogAction(entry, true, 1_050_000, CFG);
    expect(action).toBe("healthy");
    expect(entry.firstHealthyAt).toBe(1_050_000);
    expect(entry.lastDestroyAt).toBeNull(); // a successful (re)boot clears backoff
  });

  it("healthy probe does NOT overwrite an already-set firstHealthyAt", () => {
    const entry = fresh({ firstHealthyAt: 1_010_000 });
    watchdogAction(entry, true, 1_090_000, CFG);
    expect(entry.firstHealthyAt).toBe(1_010_000);
  });

  it("never-yet-healthy AND within boot-grace ⇒ skip-booting (cold boot, no destroy)", () => {
    const entry = fresh({ firstSeenAt: 1_000_000 });
    // 100s elapsed < 180s boot-grace.
    expect(watchdogAction(entry, false, 1_100_000, CFG)).toBe("skip-booting");
  });

  it("never-yet-healthy AND past boot-grace ⇒ destroy (broken/stuck boot)", () => {
    const entry = fresh({ firstSeenAt: 1_000_000 });
    // 200s elapsed >= 180s boot-grace, never came up.
    expect(watchdogAction(entry, false, 1_200_000, CFG)).toBe("destroy");
  });

  it("previously-healthy that went dark ⇒ destroy immediately (grace does NOT apply)", () => {
    // firstHealthyAt set ⇒ NOT booting; a fresh firstSeenAt must not shield it.
    const entry = fresh({ firstSeenAt: 1_190_000, firstHealthyAt: 1_005_000 });
    expect(watchdogAction(entry, false, 1_200_000, CFG)).toBe("destroy");
  });

  it("just-rebooted (lastDestroyAt within backoff) ⇒ skip-backoff even if unhealthy", () => {
    const entry = fresh({ lastDestroyAt: 1_100_000, firstHealthyAt: null });
    // 50s since destroy < 180s backoff — do not re-destroy.
    expect(watchdogAction(entry, false, 1_150_000, CFG)).toBe("skip-backoff");
  });

  it("backoff takes precedence over an expired boot-grace", () => {
    // Never healthy, past boot-grace, but destroyed recently → still skip-backoff.
    const entry = fresh({ firstSeenAt: 500_000, lastDestroyAt: 1_100_000, firstHealthyAt: null });
    expect(watchdogAction(entry, false, 1_200_000, CFG)).toBe("skip-backoff");
  });
});

// ─────────────────────────── proxyFetch timeout → 503 ───────────────────────
// Simulate a fired AbortSignal.timeout: the container fetch rejects with a
// Timeout/Abort-named error (what the platform produces on abort). proxyFetch
// must translate that to a structured 503 on a bounded route, and NOT swallow it
// on an exempt long-lived route.
function timeoutError(): Error {
  return Object.assign(new Error("The operation timed out."), { name: "TimeoutError" });
}

// ─────────────────────── scheduled() zero-idle-cost gate ───────────────────────
describe("scheduled() — idle gate skips the health probe so an idle fabricd sleeps", () => {
  // A container mock that records every path it is fetched and answers the
  // container-free idle-status from `lastActivityMs`, and /v1/health with 200.
  function mockContainer(lastActivityMs: number, opts: { idleThrows?: boolean } = {}) {
    const paths: string[] = [];
    getContainer.mockImplementation(() => ({
      fetch: (req: Request) => {
        const { pathname } = new URL(req.url);
        paths.push(pathname);
        if (pathname === "/__do/idle-status") {
          if (opts.idleThrows) return Promise.reject(new Error("boom"));
          return Promise.resolve(
            new Response(JSON.stringify({ lastActivityMs }), {
              headers: { "content-type": "application/json" },
            }),
          );
        }
        // /v1/health → healthy (200) so the watchdog loop exits on the first probe
        // (no inter-probe GAP_MS wait ⇒ the test stays fast).
        return Promise.resolve(new Response("ok", { status: 200 }));
      },
    }));
    return paths;
  }

  const run = () =>
    worker.scheduled(
      {} as unknown as Parameters<typeof worker.scheduled>[0],
      envWithShards(1),
    );

  it("idle (last activity > 4m ago) ⇒ reads idle-status but does NOT probe /v1/health", async () => {
    const paths = mockContainer(Date.now() - 5 * 60_000);
    await run();
    expect(paths).toContain("/__do/idle-status");
    expect(paths).not.toContain("/v1/health");
  });

  it("unset marker (lastActivityMs=0) ⇒ treated as idle ⇒ no /v1/health probe", async () => {
    const paths = mockContainer(0);
    await run();
    expect(paths).not.toContain("/v1/health");
  });

  it("recent activity ⇒ runs the watchdog (does probe /v1/health)", async () => {
    const paths = mockContainer(Date.now());
    await run();
    expect(paths).toContain("/v1/health");
  });

  it("idle-status read failure ⇒ fail-CLOSED to the legacy watchdog (probes /v1/health)", async () => {
    const paths = mockContainer(0, { idleThrows: true });
    await run();
    expect(paths).toContain("/v1/health");
  });
});

describe("proxyFetch — wedged upstream → structured 503 (bounded routes only)", () => {
  it("a bounded lease-op (GET /v1/leases/{id}) on a hung upstream → 503 JSON, no internals", async () => {
    getContainer.mockImplementation(() => ({ fetch: () => Promise.reject(timeoutError()) }));

    const resp = await worker.fetch(
      new Request("http://fabricd/v1/leases/lease-x", { method: "GET" }),
      envWithShards(1),
    );

    expect(resp.status).toBe(503);
    expect(resp.headers.get("content-type")).toBe("application/json");
    const body = (await resp.json()) as { error: string };
    // Generic reason only — no stack, path, or platform detail leaked.
    expect(body.error).toBe("fabricd upstream timeout");
    expect(JSON.stringify(body)).not.toMatch(/TimeoutError|stack|singleton|shard/i);
  });

  it("a healthy upstream on the happy path is UNTOUCHED (timeout never fires)", async () => {
    const ok = new Response("{}", { status: 200 });
    getContainer.mockImplementation(() => ({ fetch: () => Promise.resolve(ok) }));

    const resp = await worker.fetch(
      new Request("http://fabricd/v1/leases/lease-x", { method: "GET" }),
      envWithShards(1),
    );
    expect(resp).toBe(ok); // exact passthrough — no 503 wrapping on success
  });

  it("EXEMPT long-lived route (POST /v1/leases/{id}/exec) does NOT get a 503 — error propagates", async () => {
    getContainer.mockImplementation(() => ({ fetch: () => Promise.reject(timeoutError()) }));

    // exec is unbounded (runs box code); proxyFetch forwards as-is, so the
    // rejection surfaces rather than being masked as a 503.
    await expect(
      worker.fetch(
        new Request("http://fabricd/v1/leases/lease-x/exec", { method: "POST", body: "{}" }),
        envWithShards(1),
      ),
    ).rejects.toThrow(/timed out/);
  });

  it("a non-abort upstream error still propagates on a bounded route (not masked as 503)", async () => {
    getContainer.mockImplementation(() => ({
      fetch: () => Promise.reject(new Error("connection refused")),
    }));
    await expect(
      worker.fetch(new Request("http://fabricd/v1/leases/lease-x", { method: "GET" }), envWithShards(1)),
    ).rejects.toThrow(/connection refused/);
  });
});

// ─────────────────────────── isLongLivedRoute classification ───────────────────
describe("isLongLivedRoute — the timeout-exemption predicate", () => {
  it("exempts the unbounded/blocking routes", () => {
    expect(isLongLivedRoute("POST", "/v1/queue/trigger")).toBe(true);
    expect(isLongLivedRoute("POST", "/v1/leases/lease-abc/exec")).toBe(true);
    expect(isLongLivedRoute("POST", "/v1/leases/lease-abc/close")).toBe(true);
  });

  it("does NOT exempt bounded routes (acquire, list, status, metrics, health, agent-exec)", () => {
    expect(isLongLivedRoute("POST", "/v1/leases")).toBe(false); // acquire (bounded provision)
    expect(isLongLivedRoute("GET", "/v1/leases")).toBe(false); // list
    expect(isLongLivedRoute("GET", "/v1/leases/lease-abc")).toBe(false); // status
    expect(isLongLivedRoute("GET", "/v1/metrics/tenant")).toBe(false);
    expect(isLongLivedRoute("GET", "/v1/health")).toBe(false);
    // agent-exec is async (202 + poll) — bounded, so NOT exempt.
    expect(isLongLivedRoute("POST", "/v1/leases/lease-abc/agent-exec")).toBe(false);
  });

  it("is method-sensitive — a GET to a long-lived path is not exempt", () => {
    expect(isLongLivedRoute("GET", "/v1/queue/trigger")).toBe(false);
    expect(isLongLivedRoute("GET", "/v1/leases/lease-abc/exec")).toBe(false);
  });
});
