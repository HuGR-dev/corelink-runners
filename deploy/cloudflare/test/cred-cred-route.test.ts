// Route-level (HTTP) tests for the cred-ticket redemption route:
//   POST /v1/leases/{lease_id}/cas-cred   (src/index.ts, ~L1104)
//
// This is the seam clw (inside the untrusted container) hits at boot to redeem
// its single-use CLW_CRED_TICKET for the per-job CAS PAT. It is TICKET-authed
// (the ticket IS the credential) and mounted BEFORE the bearer gate. The
// handler maps the CRED_STASH DO's {status, cred} to an HTTP status AND renames
// the DO's StashedCred keys (token/endpoint/tenant) to the wire keys clw reads
// (cas_pat/clw_endpoint/clw_tenant). That rename is EXACTLY the class of bug that
// cold-boots every runner silently (cf. the 2026-07-09 token/token_plaintext
// incident) — so it must be pinned at the route level, not just in decideRedeem.
//
// NEW FILE (W4). Does NOT touch test/index.test.ts or test/check-host.test.ts.
import { describe, it, expect, vi, beforeEach } from "vitest";

// @cloudflare/containers is imported transitively by src/index.ts; mock it so the
// worker module loads under node (mirrors test/check-host.test.ts).
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import worker, { type Env } from "../src/index";
import type { StashedCred } from "../src/lib";

// ── A CRED_STASH DO double whose `redeem` returns a scripted {status, cred}. The
// test asserts the handler's DO-status→HTTP-status mapping + the body key rename.
function fakeCredStash(result: { status: number; cred?: StashedCred }) {
  const stub = { redeem: vi.fn(async (_ticket: string) => result) };
  const idFromName = vi.fn((n: string) => `id:${n}`);
  return {
    get: vi.fn(() => stub),
    idFromName,
    newUniqueId: vi.fn(),
    _stub: stub,
    _idFromName: idFromName,
  };
}

function envWith(stash: ReturnType<typeof fakeCredStash>): Env {
  return {
    // The route is reached BEFORE the bearer gate, so no spawn token is needed —
    // but the shape must satisfy Env. Cast keeps the double minimal.
    CRED_STASH: stash as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "unused-here",
  } as Env;
}

function redeemReq(leaseId: string, body: unknown, rawBody?: string): Request {
  return new Request(`https://w/v1/leases/${leaseId}/cas-cred`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: rawBody !== undefined ? rawBody : JSON.stringify(body),
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("POST /v1/leases/{id}/cas-cred — DO-status → HTTP-status mapping", () => {
  it("200 + cred ⇒ 200 with the key-renamed body (cas_pat/clw_endpoint/clw_tenant + clw_ref_domain)", async () => {
    const cred: StashedCred = {
      token: "cas-pat-plaintext",
      endpoint: "https://cache.corelink.dev",
      tenant: "acme-tenant",
    };
    const stash = fakeCredStash({ status: 200, cred });
    const resp = await worker.fetch(redeemReq("job-123", { ticket: "tkt-abc" }), envWith(stash));
    expect(resp.status).toBe(200);
    // THE bug-prone seam: StashedCred.{token,endpoint,tenant} → the wire keys clw
    // reads. A rename on EITHER side silently cold-boots the runner. Assert the
    // exact rename + the fixed clw_ref_domain, and that NO raw StashedCred key leaks.
    const body = (await resp.json()) as Record<string, unknown>;
    expect(body).toEqual({
      cas_pat: "cas-pat-plaintext",
      clw_endpoint: "https://cache.corelink.dev",
      clw_tenant: "acme-tenant",
      clw_ref_domain: "runner",
    });
    expect(body.token).toBeUndefined();
    expect(body.endpoint).toBeUndefined();
    expect(body.tenant).toBeUndefined();
    // The DO instance was addressed by the lease id from the PATH, and the ticket
    // from the body was forwarded to redeem().
    expect(stash._idFromName).toHaveBeenCalledWith("job-123");
    expect(stash._stub.redeem).toHaveBeenCalledWith("tkt-abc");
  });

  it("401 (bad ticket) ⇒ 401 {error:'invalid ticket'}", async () => {
    const stash = fakeCredStash({ status: 401 });
    const resp = await worker.fetch(redeemReq("job-1", { ticket: "wrong" }), envWith(stash));
    expect(resp.status).toBe(401);
    expect(await resp.json()).toEqual({ error: "invalid ticket" });
  });

  it("410 (already-redeemed/expired) ⇒ 410 {error:'ticket already redeemed'}", async () => {
    const stash = fakeCredStash({ status: 410 });
    const resp = await worker.fetch(redeemReq("job-1", { ticket: "t" }), envWith(stash));
    expect(resp.status).toBe(410);
    expect(await resp.json()).toEqual({ error: "ticket already redeemed" });
  });

  it("404 (no stash for this lease) ⇒ 404 {error:'no such lease'}", async () => {
    const stash = fakeCredStash({ status: 404 });
    const resp = await worker.fetch(redeemReq("nope", { ticket: "t" }), envWith(stash));
    expect(resp.status).toBe(404);
    expect(await resp.json()).toEqual({ error: "no such lease" });
  });

  it("a 200 status WITHOUT a cred does NOT deliver a PAT (falls through to 404, never a null token)", async () => {
    // Defensive: decideRedeem never returns 200 w/o cred, but the handler guards
    // `r.status === 200 && r.cred`. A 200-without-cred must NOT emit {cas_pat:undefined}.
    const stash = fakeCredStash({ status: 200 });
    const resp = await worker.fetch(redeemReq("job-1", { ticket: "t" }), envWith(stash));
    expect(resp.status).toBe(404);
    const body = (await resp.json()) as Record<string, unknown>;
    expect(body.cas_pat).toBeUndefined();
  });
});

describe("POST /v1/leases/{id}/cas-cred — input guards (never reach the DO)", () => {
  it("missing ticket ⇒ 400 {error:'ticket required'} and redeem is NOT called", async () => {
    const stash = fakeCredStash({ status: 200 });
    const resp = await worker.fetch(redeemReq("job-1", {}), envWith(stash));
    expect(resp.status).toBe(400);
    expect(await resp.json()).toEqual({ error: "ticket required" });
    expect(stash._stub.redeem).not.toHaveBeenCalled();
  });

  it("a malformed JSON body ⇒ 400 (clean, not an opaque 500) and redeem is NOT called", async () => {
    const stash = fakeCredStash({ status: 200 });
    const resp = await worker.fetch(redeemReq("job-1", undefined, "not-json{"), envWith(stash));
    expect(resp.status).toBe(400);
    expect(((await resp.json()) as { error: string }).error).toMatch(/invalid JSON body/);
    expect(stash._stub.redeem).not.toHaveBeenCalled();
  });

  it("a URL-encoded lease id in the path is decoded before addressing the DO", async () => {
    const stash = fakeCredStash({ status: 404 });
    // "acme%2Fjob" ⇒ decodeURIComponent ⇒ "acme/job".
    await worker.fetch(redeemReq("acme%2Fjob", { ticket: "t" }), envWith(stash));
    expect(stash._idFromName).toHaveBeenCalledWith("acme/job");
  });
});
