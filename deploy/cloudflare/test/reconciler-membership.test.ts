import { beforeEach, describe, expect, it, vi } from "vitest";

const { tokenMock } = vi.hoisted(() => ({ tokenMock: vi.fn() }));
vi.mock("../src/github_app.js", () => ({ installationToken: tokenMock }));
import { confirmInstallationRepositories } from "../src/reconciler_membership.js";

const env = {} as never;
const A = "installation-a";
const B = "installation-b";
const response = (repositories: unknown[], status = 200) => new Response(JSON.stringify({ repositories }), { status });

beforeEach(() => { tokenMock.mockReset(); tokenMock.mockImplementation(async (_env: unknown, id: string) => ({ token: `token-${id}`, expires_at: "2099-01-01T00:00:00Z" })); });

describe("confirmInstallationRepositories", () => {
  it("mints once per installation, paginates, compares case-insensitively, and dedups exact pairs", async () => {
    const calls: Request[] = [];
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = new Request(input, init); calls.push(request);
      const page = new URL(request.url).searchParams.get("page");
      return page === "1" ? response(Array.from({ length: 100 }, (_, i) => ({ full_name: `other/${i}` }))) : response([{ full_name: "Acme/Repo" }]);
    });
    const result = await confirmInstallationRepositories(env, [
      { repo: "acme/repo", installationId: A }, { repo: "ACME/REPO", installationId: A },
      { repo: "acme/repo", installationId: B }, { repo: "missing/nope", installationId: A },
    ], 1, fetcher);
    expect(result).toEqual([{ repo: "acme/repo", installationId: A }, { repo: "ACME/REPO", installationId: A }, { repo: "acme/repo", installationId: B }]);
    expect(tokenMock).toHaveBeenCalledTimes(2);
    expect(fetcher).toHaveBeenCalledTimes(4);
    expect(calls.slice(0, 2).every((request) => request.headers.get("authorization") === "Bearer token-installation-a")).toBe(true);
    expect(calls.slice(2).every((request) => request.headers.get("authorization") === "Bearer token-installation-b")).toBe(true);
    expect(calls.every((request) => request.headers.get("x-github-api-version") === "2022-11-28")).toBe(true);
  });

  it("returns only positive membership and preserves candidate spelling", async () => {
    const fetcher = vi.fn(async () => response([{ full_name: "owner/known" }]));
    await expect(confirmInstallationRepositories(env, [{ repo: "OWNER/KNOWN", installationId: A }, { repo: "owner/absent", installationId: A }], 1, fetcher)).resolves.toEqual([{ repo: "OWNER/KNOWN", installationId: A }]);
  });

  it("discards all results on a late installation or page failure", async () => {
    tokenMock.mockImplementation(async (_env: unknown, id: string) => { if (id === B) throw new Error("mint failed"); return { token: "token", expires_at: "2099-01-01T00:00:00Z" }; });
    const fetcher = vi.fn(async () => response([{ full_name: "owner/known" }]));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/known", installationId: A }, { repo: "owner/known", installationId: B }], 1, fetcher)).resolves.toBeNull();
    fetcher.mockImplementationOnce(async () => response(Array.from({ length: 100 }, () => ({ full_name: "owner/known" })))).mockImplementationOnce(async () => response([], 503));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/known", installationId: A }], 1, fetcher)).resolves.toBeNull();
  });

  it("rejects malformed pages, redirects/status errors, and candidate bounds", async () => {
    const fetcher = vi.fn(async () => response([{ name: "missing-full-name" }]));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/repo", installationId: A }], 1, fetcher)).resolves.toBeNull();
    fetcher.mockResolvedValueOnce(response([], 302));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/repo", installationId: A }], 1, fetcher)).resolves.toBeNull();
    await expect(confirmInstallationRepositories(env, [{ repo: " ", installationId: A }], 1, fetcher)).resolves.toBeNull();
  });

  it("allows a known empty page and avoids mint/network for no candidates", async () => {
    const fetcher = vi.fn(async () => response([]));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/repo", installationId: A }], 1, fetcher)).resolves.toEqual([]);
    tokenMock.mockClear(); fetcher.mockClear();
    await expect(confirmInstallationRepositories(env, [], 1, fetcher)).resolves.toEqual([]);
    expect(tokenMock).not.toHaveBeenCalled(); expect(fetcher).not.toHaveBeenCalled();
  });

  it("fails closed on an oversized response and an unbounded full-page stream", async () => {
    const oversized = vi.fn(async () => new Response("x".repeat(256 * 1024 + 1), { status: 200 }));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/repo", installationId: A }], 1, oversized)).resolves.toBeNull();
    const full = vi.fn(async () => response(Array.from({ length: 100 }, (_, i) => ({ full_name: `owner/${i}` }))));
    await expect(confirmInstallationRepositories(env, [{ repo: "owner/none", installationId: A }], 1, full)).resolves.toBeNull();
    expect(full).toHaveBeenCalledTimes(100);
  });
});
