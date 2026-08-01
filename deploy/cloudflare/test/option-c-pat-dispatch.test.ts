// Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21).
//
// Proves the mint wire the runners TL + server TL froze in
// docs/handoff/2026-07-21-*optionC*: when the workflow_job repo is mapped to a
// tenant-PAT secret, `mintCasPat` (via buildContainerEnv) resolves the tenant by
// INTROSPECTING the acquiring PAT instead of deriving it from installation_id —
//   • KEEP  x-corelink-internal-auth (dispatcher trust boundary, unchanged)
//   • ADD   Authorization: Bearer <pat>
//   • OMIT  installation_id ENTIRELY (a null/"" would be a 400)
//   • PIN   scope cas:rw
// and the default installation-derived path stays byte-identical.
import { describe, it, expect, vi, afterEach } from "vitest";
import { buildContainerEnv, tenantPatSecretForRepo, type MintEnv } from "../src/lib";

const COLD_PAT = "corelink_pat_FRRBJ4DG0HGFJG5P.aaa.bbb";
const REPO = "HuGR-Labs/corelink-cold-organic-e2e";

// Capture the single /internal/v1/runner/mint request and return a fake 200.
function mockMint(capture: { req?: { headers: Headers; body: unknown } }) {
  return vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const u = String(url);
    if (u.endsWith("/internal/v1/runner/mint")) {
      capture.req = {
        headers: new Headers(init?.headers as HeadersInit),
        body: JSON.parse(String(init?.body ?? "{}")),
      };
      return new Response(
        JSON.stringify({
          token_plaintext: "corelink_pat_MINTED.xxx.yyy",
          pat_id: "pat_123",
          token_id: "tok_123",
          tenant: "3c7d77b1-0a50-4f87-893f-36ac785670df",
          max_concurrency: 20,
          expires_ms: 9_999_999_999_999,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    return new Response(null, { status: 404 });
  });
}

afterEach(() => vi.restoreAllMocks());

describe("tenantPatSecretForRepo — repo → secret-name map (pure)", () => {
  const map = JSON.stringify({ [REPO]: "COLD_ORGANIC_TENANT_PAT" });
  it("returns the secret NAME for a mapped repo", () => {
    expect(tenantPatSecretForRepo(map, REPO)).toBe("COLD_ORGANIC_TENANT_PAT");
  });
  it("returns '' for an unmapped repo", () => {
    expect(tenantPatSecretForRepo(map, "acme/other")).toBe("");
  });
  it("returns '' on absent/blank/malformed map (⇒ default path, never throws)", () => {
    expect(tenantPatSecretForRepo(undefined, REPO)).toBe("");
    expect(tenantPatSecretForRepo("", REPO)).toBe("");
    expect(tenantPatSecretForRepo("{not json", REPO)).toBe("");
    expect(tenantPatSecretForRepo(JSON.stringify({ [REPO]: 42 }), REPO)).toBe("");
  });
});

describe("mintCasPat Option-C wire (via buildContainerEnv)", () => {
  const baseEnv: MintEnv = {
    CORELINK_RUNNER_MINT_AUTH_KEY: "dispatcher-key",
    CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    CLW_ENDPOINT: "https://corelink-api.humangr.com",
    ALLOW_LEGACY_PAT_ENV: "1", // non-prod: let the warm mint complete without env-0 deps
  };

  it("acquiringPat set ⇒ Bearer + internal-auth + NO installation_id + scope cas:rw", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    const r = await buildContainerEnv(baseEnv, {
      jobId: "42",
      repoFullName: REPO,
      installationId: "150584374", // present (for the JIT), but MUST NOT reach the mint body
      acquiringPat: COLD_PAT,
    });
    expect(cap.req).toBeDefined();
    // internal-auth stays (trust boundary), Bearer added (names the tenant).
    expect(cap.req!.headers.get("x-corelink-internal-auth")).toBe("dispatcher-key");
    expect(cap.req!.headers.get("authorization")).toBe(`Bearer ${COLD_PAT}`);
    // installation_id OMITTED entirely (not null, not "").
    expect(cap.req!.body).not.toHaveProperty("installation_id");
    expect(cap.req!.body.repo_full_name).toBe(REPO);
    expect(cap.req!.body.scope).toBe("cas:rw");
    // Server-derived tenant threads back through.
    expect(r.authz).toBe("ok");
    expect(r.tenant).toBe("3c7d77b1-0a50-4f87-893f-36ac785670df");
  });

  it("acquiringPat absent ⇒ default path byte-identical (installation_id present, no Bearer, read-write)", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    await buildContainerEnv(baseEnv, {
      jobId: "43",
      repoFullName: "HuGR-Labs/corelink-runners",
      installationId: "150584374",
    });
    expect(cap.req!.headers.get("authorization")).toBeNull();
    expect(cap.req!.headers.get("x-corelink-internal-auth")).toBe("dispatcher-key");
    expect(cap.req!.body.installation_id).toBe("150584374");
    expect(cap.req!.body.scope).toBe("read-write");
  });

  it("gate: acquiringPat authorizes even with NO installation_id (Option-C needs no install)", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    const r = await buildContainerEnv(baseEnv, {
      jobId: "44",
      repoFullName: REPO,
      installationId: "", // no install at all
      acquiringPat: COLD_PAT,
    });
    // A mint WAS attempted (not the cold fail-open) because acquiringPat is present.
    expect(cap.req).toBeDefined();
    expect(r.tenant).toBe("3c7d77b1-0a50-4f87-893f-36ac785670df");
  });

  it("gate: NO installation_id AND NO acquiringPat ⇒ COLD fail-open (no mint attempted)", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    const r = await buildContainerEnv(baseEnv, {
      jobId: "45",
      repoFullName: REPO,
      installationId: "",
    });
    expect(cap.req).toBeUndefined(); // never called the mint
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({});
  });
});
