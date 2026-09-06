// Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21).
//
// ADR-0013 retains both owner identities for server-side conflict validation.
// When a workflow_job repo is mapped to a tenant-PAT secret:
//   • KEEP  x-corelink-internal-auth (dispatcher trust boundary, unchanged)
//   • ADD   Authorization: Bearer <pat>
//   • KEEP  installation_id when supplied; omit it only for native PAT callers
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
  it("matches canonical lowercase webhook repos to mixed-case map keys", () => {
    const mixedCaseMap = JSON.stringify({ "HuGR-Labs/Corelink-Cold-Organic-E2E": "COLD_ORGANIC_TENANT_PAT" });
    expect(tenantPatSecretForRepo(mixedCaseMap, REPO.toLowerCase())).toBe("COLD_ORGANIC_TENANT_PAT");
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
  it("fails closed for an invalid query or duplicate canonical map keys", () => {
    expect(tenantPatSecretForRepo(map, "not-a-repo")).toBe("");
    const duplicate = JSON.stringify({ "Acme/Repo": "PAT_A", " acme/repo ": "PAT_B" });
    expect(tenantPatSecretForRepo(duplicate, "acme/repo")).toBe("");
  });
});

describe("mintCasPat Option-C wire (via buildContainerEnv)", () => {
  const baseEnv: MintEnv = {
    CORELINK_RUNNER_MINT_AUTH_KEY: "dispatcher-key",
    CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    CLW_ENDPOINT: "https://corelink-api.humangr.com",
  };
  const env0 = {
    stash: { stash: async (_lease: string, _ticket: string) => "ticket-1" },
    fabricEndpoint: "https://runner.example",
  };

  it("keeps installation identity alongside acquiring PAT for server conflict validation", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    const r = await buildContainerEnv(baseEnv, {
      jobId: "42",
      repoFullName: REPO,
      installationId: "150584374",
      acquiringPat: COLD_PAT,
    }, env0);
    expect(cap.req).toBeDefined();
    // Both credentials reach the server; the PAT cannot hide the installation.
    expect(cap.req!.headers.get("x-corelink-internal-auth")).toBe("dispatcher-key");
    expect(cap.req!.headers.get("authorization")).toBe(`Bearer ${COLD_PAT}`);
    expect(cap.req!.body.installation_id).toBe("150584374");
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
    }, env0);
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
    }, env0);
    // A mint WAS attempted (not the cold fail-open) because acquiringPat is present.
    expect(cap.req).toBeDefined();
    expect(cap.req!.body).not.toHaveProperty("installation_id");
    expect(r.tenant).toBe("3c7d77b1-0a50-4f87-893f-36ac785670df");
  });

  it("refuses without installation or acquiring PAT before mint", async () => {
    const cap: { req?: { headers: Headers; body: any } } = {};
    vi.stubGlobal("fetch", mockMint(cap));
    const r = await buildContainerEnv(baseEnv, {
      jobId: "45",
      repoFullName: REPO,
      installationId: "",
    });
    expect(cap.req).toBeUndefined(); // never called the mint
    expect(r.authz).toBe("forbidden");
    expect(r.containerEnv).toEqual({});
  });
});
