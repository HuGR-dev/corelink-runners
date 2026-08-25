// ─────────────────────────────────────────────────────────────────────────────
// ORPHAN-RECONCILE INPUTS — hardening of the two data sources the orphan sweep
// feeds a (future) destroy decision from. Both bugs are FAIL-UNSAFE (they turn a
// live, accounted box into a false orphan), so they are fixed BEFORE teardown is
// ever armed, and proven here independently of `reconcileOrphanBoxes` (which
// injects fakes for both and so never exercised the real helpers).
//
//  1. listRunningInstances MUST scope to the runner app only. The CF account also
//     hosts corelink-prod-* (customer-serving), githugr-*, fabricd, and this
//     worker's own checkhost app — none write `sbox:`, so an unscoped sweep flags
//     every long-running instance of them as a false orphan.
//  2. listSpawnedBoxHandles MUST paginate the `sbox:` KV list and fail closed. A
//     single kv.list caps at 1000 keys; a CI storm truncates the known-handle set
//     and turns accounted live boxes into false orphans.
// ─────────────────────────────────────────────────────────────────────────────

import { describe, it, expect, vi, afterEach } from "vitest";

// `../src/index` imports @cloudflare/containers, whose real Container base class is
// undefined outside the Workers runtime. These helpers touch neither — stub it so
// the module loads (mirrors orphan-box-reconcile.test.ts).
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({ destroy: vi.fn(), teardown: vi.fn() })),
}));

import { listRunningInstances, listSpawnedBoxHandles } from "../src/index";

const RUNNER_APP_ID = "a03d11a2-7e03-48a4-96bb-4d2c43892cd4";

// ── listRunningInstances: a fetch mock routing by URL ────────────────────────
// `apps` is the applications-list payload; `instancesByApp[appId]` is that app's
// instances payload ({ instances, next_page_token? }).
function mockCfFetch(
  apps: Array<{ id: string; name: string }>,
  instancesByApp: Record<string, { instances: unknown[]; next_page_token?: string }>,
) {
  return vi.fn(async (url: string) => {
    const u = String(url);
    if (u.endsWith("/containers/applications")) {
      return { ok: true, json: async () => ({ success: true, result: apps }) } as never;
    }
    const m = u.match(/applications\/([^/]+)\/instances/);
    if (m) {
      const appId = m[1];
      const payload = instancesByApp[appId] ?? { instances: [] };
      return {
        ok: true,
        json: async () => ({
          success: true,
          result: { instances: payload.instances },
          result_info: payload.next_page_token
            ? { next_page_token: payload.next_page_token }
            : {},
        }),
      } as never;
    }
    throw new Error(`unexpected fetch: ${u}`);
  });
}

const runInst = (name: string, ageH = 10) => ({
  id: `inst-${name}`,
  name,
  status: { state: "running" },
  started_at: new Date(Date.now() - ageH * 3600 * 1000).toISOString(),
  image: "registry/x@sha256:abc",
});

const enumEnv = () =>
  ({ CLOUDFLARE_ACCOUNT_ID: "acct-123", CLOUDFLARE_CONTAINERS_API_TOKEN: "tok" }) as never;

describe("listRunningInstances — app scoping", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("returns ONLY runner-app instances; prod/githugr names never surface", async () => {
    const apps = [
      { id: RUNNER_APP_ID, name: "corelink-spawn-worker-runnercontainer" },
      { id: "prod-app-id", name: "corelink-prod-corelinkserver-prod" },
      { id: "githugr-app-id", name: "githugr-githugrcontainer" },
    ];
    vi.stubGlobal(
      "fetch",
      mockCfFetch(apps, {
        [RUNNER_APP_ID]: { instances: [runInst("runner-box-1"), runInst("runner-box-2")] },
        "prod-app-id": { instances: [runInst("prod-server-live")] },
        "githugr-app-id": { instances: [runInst("githugr-live")] },
      }),
    );
    const out = await listRunningInstances(enumEnv());
    const names = out.map((i) => i.name).sort();
    expect(names).toEqual(["runner-box-1", "runner-box-2"]);
    expect(names).not.toContain("prod-server-live");
    expect(names).not.toContain("githugr-live");
  });

  it("runner app absent ⇒ returns [] and logs orphan_scan_runner_app_missing (fail-quiet, loud)", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal(
      "fetch",
      mockCfFetch([{ id: "prod-app-id", name: "corelink-prod-corelinkserver-prod" }], {
        "prod-app-id": { instances: [runInst("prod-server-live")] },
      }),
    );
    const out = await listRunningInstances(enumEnv());
    expect(out).toEqual([]);
    const logged = errors.mock.calls.map((c) => String(c[0]));
    expect(logged.some((l) => l.includes("orphan_scan_runner_app_missing"))).toBe(true);
    errors.mockRestore();
  });

  it("a truncated instances page (next_page_token present) ⇒ THROWS (fail-closed)", async () => {
    vi.stubGlobal(
      "fetch",
      mockCfFetch([{ id: RUNNER_APP_ID, name: "corelink-spawn-worker-runnercontainer" }], {
        [RUNNER_APP_ID]: { instances: [runInst("runner-box-1")], next_page_token: "more" },
      }),
    );
    await expect(listRunningInstances(enumEnv())).rejects.toThrow(/TRUNCATED PAGE/);
  });

  it("no creds ⇒ returns [] without any fetch (belt-and-braces gate)", async () => {
    const f = vi.fn();
    vi.stubGlobal("fetch", f);
    const out = await listRunningInstances({} as never);
    expect(out).toEqual([]);
    expect(f).not.toHaveBeenCalled();
  });
});

// ── listSpawnedBoxHandles: a paginating KV stub ──────────────────────────────
// Pages the keys in chunks; emits list_complete only on the final chunk, with a
// cursor otherwise — mirroring the Workers KV list contract.
function pagingKv(handles: string[], pageSize: number, opts?: { dropFinalCursor?: boolean }) {
  const keys = handles.map((h, i) => `sbox:cf-runner-${i}`);
  const store = new Map(keys.map((k, i) => [k, JSON.stringify({ h: handles[i] })]));
  return {
    list: vi.fn(async ({ cursor }: { prefix: string; cursor?: string }) => {
      const start = cursor ? Number(cursor) : 0;
      const slice = keys.slice(start, start + pageSize);
      const next = start + pageSize;
      const complete = next >= keys.length;
      return {
        keys: slice.map((name) => ({ name })),
        list_complete: complete,
        // The pathological case: not complete yet no cursor to continue.
        cursor: complete ? undefined : opts?.dropFinalCursor ? undefined : String(next),
      };
    }),
    get: vi.fn(async (k: string) => store.get(k) ?? null),
  };
}

describe("listSpawnedBoxHandles — pagination + fail-closed", () => {
  it("collects ALL handles across multiple pages (no 1000-key truncation)", async () => {
    const handles = Array.from({ length: 2500 }, (_, i) => `handle-${i}`);
    const kv = pagingKv(handles, 1000);
    const known = await listSpawnedBoxHandles({ RUNNER_JOB_PATS: kv } as never);
    expect(known.size).toBe(2500);
    expect(known.has("handle-0")).toBe(true);
    expect(known.has("handle-2499")).toBe(true);
    // Proves it did NOT stop at the first 1000-key page.
    expect(kv.list.mock.calls.length).toBeGreaterThanOrEqual(3);
  });

  it("a single complete page works", async () => {
    const kv = pagingKv(["a", "b", "c"], 1000);
    const known = await listSpawnedBoxHandles({ RUNNER_JOB_PATS: kv } as never);
    expect([...known].sort()).toEqual(["a", "b", "c"]);
  });

  it("incomplete list with NO cursor ⇒ THROWS (fail-closed, never a truncated set)", async () => {
    const handles = Array.from({ length: 2500 }, (_, i) => `handle-${i}`);
    const kv = pagingKv(handles, 1000, { dropFinalCursor: true });
    await expect(
      listSpawnedBoxHandles({ RUNNER_JOB_PATS: kv } as never),
    ).rejects.toThrow(/incomplete but returned no cursor/);
  });

  it("no KV bound ⇒ empty set (gate handles the real inertness elsewhere)", async () => {
    const known = await listSpawnedBoxHandles({} as never);
    expect(known.size).toBe(0);
  });
});
