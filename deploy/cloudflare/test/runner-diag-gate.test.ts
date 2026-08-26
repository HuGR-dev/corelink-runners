// POST /v1/leases/{lease_id}/runner-diag — the box→Worker diagnostic sink.
//
// This route answers BEFORE the bearer gate, and it must: a runner container has
// only CF-internal egress and no `wrangler containers logs`, so a `run.sh
// --jitconfig` registration failure is otherwise invisible — this sink is how the
// 2026-07-21 root cause was found. It cannot demand a credential either, because a
// COLD box holds no CLW_CRED_TICKET at all, and requiring one would blind exactly
// the boxes that fail most.
//
// ⚠️ WHAT THAT COST US, until 2026-08-25: the route was reachable by ANYONE.
// Verified against production — an anonymous POST returned `200 {"ok":true}` while
// the sibling `/v1/` route returned `401` — so any client could write 3000
// attacker-chosen characters into our logs as an error-level `runner_diag` under
// any jobId: unbounded log cost, and FORGED registration diagnostics poisoning the
// exact channel an operator reads after a failed spawn.
//
// The gate is existence, not identity: the named job must hold a live spawn-claim.
// That is deliberately weaker than authentication and the cells below say so —
// job ids are public on a public repo, so a targeted flood against a REAL in-flight
// job is still possible, which is why the rate limiter is part of the contract and
// gets its own cell rather than being treated as a nicety.
import { describe, it, expect, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import worker, { type Env } from "../src/index";

const ctx = {
  waitUntil: () => {},
  passThroughOnException: () => {},
} as unknown as ExecutionContext;

function fakeKv(seed: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(seed));
  return {
    store,
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async () => {}),
    delete: vi.fn(async () => {}),
    list: vi.fn(async () => ({ keys: [], list_complete: true })),
  };
}

function post(jobId: string, body = "run.sh exit=1\n---output tail---\nboom") {
  return new Request(`https://spawn.example/v1/leases/${jobId}/runner-diag`, {
    method: "POST",
    headers: { "content-type": "text/plain" },
    body,
  });
}

/** Capture what actually reached the log, which is the thing under attack.
 *
 * BOTH streams: `logEvent` routes `error` to console.error and everything else
 * to console.log (`src/lib.ts:12-20`), and `runner_diag` is an ERROR-level event.
 * A capture that watched only console.log would make the two "payload must not
 * appear" cells below pass vacuously — they assert an ABSENCE, and absence is
 * free when nothing is being recorded at all. */
function captureLogs() {
  const lines: string[] = [];
  const sink = (...a: unknown[]) => {
    lines.push(a.map(String).join(" "));
  };
  const spies = [
    vi.spyOn(console, "log").mockImplementation(sink),
    vi.spyOn(console, "error").mockImplementation(sink),
  ];
  return { lines, restore: () => spies.forEach((s) => s.mockRestore()) };
}

describe("runner-diag sink is gated on a live spawn-claim", () => {
  it("a CLAIMED job's diagnostic still reaches the log (the channel keeps working)", async () => {
    const kv = fakeKv({ "spawn:job-live": String(Date.now()) });
    const { lines, restore } = captureLogs();
    const r = await worker.fetch(
      post("job-live"),
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      ctx,
    );
    restore();
    expect(r.status).toBe(200);
    expect(lines.join("\n")).toContain("runner_diag");
    expect(lines.join("\n")).toContain("boom"); // the body was recorded
  });

  it("an UNCLAIMED jobId never gets its body into the log (the forgery hole)", async () => {
    const kv = fakeKv(); // no claim for this id
    const { lines, restore } = captureLogs();
    const r = await worker.fetch(
      post("job-forged", "ATTACKER-CONTROLLED-PAYLOAD"),
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      ctx,
    );
    restore();
    // Uniform response: it must not become an oracle for which ids exist.
    expect(r.status).toBe(200);
    expect(await r.json()).toEqual({ ok: true });
    expect(lines.join("\n")).not.toContain("ATTACKER-CONTROLLED-PAYLOAD");
  });

  it("the rate limiter bounds a flood against a job that IS claimed", async () => {
    const kv = fakeKv({ "spawn:job-live": String(Date.now()) });
    const limit = vi.fn(async () => ({ success: false })); // over budget
    const { lines, restore } = captureLogs();
    const r = await worker.fetch(
      post("job-live", "FLOOD-PAYLOAD"),
      { RUNNER_JOB_PATS: kv, WEBHOOK_LIMITER: { limit } } as unknown as Env,
      ctx,
    );
    restore();
    expect(r.status).toBe(200);
    expect(limit).toHaveBeenCalledWith({ key: "diag:job-live" });
    expect(lines.join("\n")).not.toContain("FLOOD-PAYLOAD");
  });
});
