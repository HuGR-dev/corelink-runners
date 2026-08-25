// ─────────────────────────────────────────────────────────────────────────────
// ORPHAN-BOX RECONCILIATION (B-002) — reap keyed on PLATFORM TRUTH, observe-only.
// ─────────────────────────────────────────────────────────────────────────────
//
// `reapStaleBoxes` starts from OUR `sbox:` bookkeeping and is structurally blind
// to a running box that has NO `sbox:` record — the exact 10.2 h leak of the
// 2026-08-23 incident. `reconcileOrphanBoxes` starts from the Cloudflare
// Containers API instead: a RUNNING instance whose `name` is in no `sbox:` record
// AND whose platform age exceeds 2×JOB_PAT_TTL_S (4 h) is an ORPHAN candidate.
//
// ⚠️ THE LOAD-BEARING CONTRACT is that this sweep is DRY-RUN: it LOGS + counts and
// NEVER tears anything down at this stage. Cell 1 and cell 8 assert `destroy` is
// never called. If a future change makes it destroy, those cells must go red until
// the teardown flag + its own test land.

import { describe, it, expect, vi, beforeEach } from "vitest";

// If the sweep ever reaches for a container, this records it — every cell then
// asserts it stays empty (no teardown at this stage).
const destroyed: string[] = [];
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string) => ({
    destroy: vi.fn(async () => {
      destroyed.push(handle);
    }),
    teardown: vi.fn(async () => {
      destroyed.push(handle);
    }),
  })),
}));

import { reconcileOrphanBoxes } from "../src/index";

const NOW = 1_800_000_000_000;
const FOUR_H_MS = 2 * 7200 * 1000; // 2×JOB_PAT_TTL_S — the orphan age floor
const iso = (ms: number) => new Date(ms).toISOString();

// A running instance whose name is NOT a known sbox handle and is 10 h old.
type Inst = {
  app: string;
  id: string;
  name: string;
  started_at: string | null;
  image: string | null;
};
const orphan10h: Inst = {
  app: "runner",
  id: "inst-orphan",
  name: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", // bare UUID = the DO handle join key
  started_at: iso(NOW - 10 * 3600 * 1000),
  image: "registry/runner@sha256:abc",
};

// An env with the enable flag ON and CF creds bound, so the gate passes and the
// injected `listInstances`/`listSbox` are exercised. METRICS absent ⇒ bumpMetrics
// is a documented no-op (no DO stub needed).
function enabledEnv(): never {
  return {
    RECONCILE_ORPHAN_BOXES: "1",
    CLOUDFLARE_ACCOUNT_ID: "acct-123",
    CLOUDFLARE_CONTAINERS_API_TOKEN: "tok",
    RUNNER_CONTAINER: {},
  } as never;
}

const instances = (...xs: Inst[]) => async () => xs;
const sbox = (...handles: string[]) => async () => new Set(handles);

beforeEach(() => {
  destroyed.length = 0;
});

describe("reconcileOrphanBoxes", () => {
  it("cell 1 — a running box with NO sbox record and age>4h is counted + LOGGED, and destroy is NEVER called (dry-run)", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(orphan10h), sbox("some-other-handle"));
    expect(n).toBe(1);
    // THE load-bearing assertion: observe-only, nothing torn down.
    expect(destroyed).toEqual([]);
    // It emitted the structured orphan line.
    const logged = errors.mock.calls.map((c) => String(c[0]));
    expect(logged.some((l) => l.includes("orphan_box_detected") && l.includes(orphan10h.id))).toBe(true);
    errors.mockRestore();
  });

  it("cell 2 — a running box WITH a matching sbox record is not flagged", async () => {
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(orphan10h), sbox(orphan10h.name));
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  it("cell 3 — a no-sbox instance YOUNGER than 4h (mid-spawn) is not flagged (false-orphan guard)", async () => {
    const young: Inst = { ...orphan10h, id: "inst-young", started_at: iso(NOW - (FOUR_H_MS - 60_000)) };
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(young), sbox());
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  it("cell 4 — an inactive tombstone is never flagged (listInstances only ever yields RUNNING)", async () => {
    // `listRunningInstances` filters status.state==="running" before this function
    // sees anything, so a tombstone simply never appears in the injected list.
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(), sbox());
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  it("cell 5 — enable flag absent ⇒ returns 0 WITHOUT calling listInstances (inert)", async () => {
    const listInstances = vi.fn(instances(orphan10h));
    const env = { ...enabledEnv(), RECONCILE_ORPHAN_BOXES: undefined } as never;
    const n = await reconcileOrphanBoxes(env, NOW, listInstances as never, sbox());
    expect(n).toBe(0);
    expect(listInstances).not.toHaveBeenCalled();
  });

  it("cell 6 — CF creds absent ⇒ returns 0 WITHOUT calling listInstances", async () => {
    const listInstances = vi.fn(instances(orphan10h));
    const env = { RECONCILE_ORPHAN_BOXES: "1", RUNNER_CONTAINER: {} } as never; // no account id / token
    const n = await reconcileOrphanBoxes(env, NOW, listInstances as never, sbox());
    expect(n).toBe(0);
    expect(listInstances).not.toHaveBeenCalled();
  });

  it("cell 7 — a listInstances throw (API error / truncated page / self-check miss) ⇒ returns 0, nothing flagged (fail-closed)", async () => {
    const n = await reconcileOrphanBoxes(
      enabledEnv(),
      NOW,
      async () => {
        throw new Error("TRUNCATED PAGE / HTTP 400 code:9106");
      },
      sbox(),
    );
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  it("cell 7b — a listSbox throw (KV unreadable) ⇒ returns 0 (cannot assert sbox absence)", async () => {
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(orphan10h), async () => {
      throw new Error("kv list failed");
    });
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  it("cell 8 — with RECONCILE_ORPHAN_TEARDOWN still off, even an unambiguous 10h orphan is only logged, destroy uncalled", async () => {
    const env = { ...enabledEnv(), RECONCILE_ORPHAN_TEARDOWN: "0" } as never;
    const n = await reconcileOrphanBoxes(env, NOW, instances(orphan10h), sbox());
    expect(n).toBe(1);
    expect(destroyed).toEqual([]);
  });

  it("cell 9 — an instance with no/unparseable started_at cannot be aged ⇒ not flagged", async () => {
    const noAge: Inst = { ...orphan10h, id: "inst-noage", started_at: null };
    const n = await reconcileOrphanBoxes(enabledEnv(), NOW, instances(noAge), sbox());
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });
});
