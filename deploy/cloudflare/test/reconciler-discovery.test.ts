import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({ destroy: vi.fn(), teardown: vi.fn() })),
}));
import {
  claimReconcileHandoff,
  discoverAuthorizationCandidates,
  reconcileHandoffKey,
  releaseReconcileHandoff,
} from "../src/reconciler";

function response(body: unknown, status = 200): Response {
  const encoded = new TextEncoder().encode(JSON.stringify(body));
  return new Response(encoded, {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("authoritative reconciler registry", () => {
  it("follows stable cursors and returns only verified eligible installations", async () => {
    const fetcher = vi.fn()
      .mockResolvedValueOnce(response({
        schema_version: 1,
        source: "runner_authorization_candidates",
        repositories: [
          { repo_full_name: "Acme/Customer", installation_id: 42 },
        ],
        next_cursor: "c1",
      }))
      .mockResolvedValueOnce(response({
        schema_version: 1,
        source: "runner_authorization_candidates",
        repositories: [{ repo_full_name: "acme/other", installation_id: "44" }],
        next_cursor: null,
      }));
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test/repos", RECONCILER_REGISTRY_AUTH_KEY: "secret" },
      fetcher,
    )).resolves.toEqual([
      { repo: "Acme/Customer", installationId: "42" },
      { repo: "acme/other", installationId: "44" },
    ]);
    expect(fetcher.mock.calls[1][0]).toContain("cursor=c1");
    expect(fetcher.mock.calls[0][1]).toMatchObject({
      method: "GET",
      headers: { "x-corelink-internal-auth": "secret" },
      redirect: "error",
    });
  });

  it("preserves repository spelling and deduplicates only exact repository/install pairs", async () => {
    const fetcher = vi.fn().mockResolvedValue(response({
      schema_version: 1,
      source: "runner_authorization_candidates",
      repositories: [
        { repo_full_name: "Acme/Repo", installation_id: "42" },
        { repo_full_name: "Acme/Repo", installation_id: 42 },
        { repo_full_name: "Acme/Repo", installation_id: "43" },
      ],
      next_cursor: null,
    }));
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, fetcher,
    )).resolves.toEqual([
      { repo: "Acme/Repo", installationId: "42" },
      { repo: "Acme/Repo", installationId: "43" },
    ]);
  });

  it("refuses an incomplete 100-page stream", async () => {
    let page = 0;
    const fetcher = vi.fn(async () => response({
      schema_version: 1,
      source: "runner_authorization_candidates",
      repositories: [],
      next_cursor: `next-${page++}`,
    }));
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, fetcher,
    )).resolves.toBeNull();
    expect(fetcher).toHaveBeenCalledTimes(100);
  });

  it("fails closed on malformed schema or a repeated cursor", async () => {
    const malformed = vi.fn().mockResolvedValueOnce(response({ schema_version: 1, source: "wrong", repositories: [], next_cursor: null }));
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, malformed,
    )).resolves.toBeNull();
    const loop = vi.fn().mockResolvedValue(response({ schema_version: 1, source: "runner_authorization_candidates", repositories: [], next_cursor: "same" }));
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, loop,
    )).resolves.toBeNull();
  });

  it("does not arm an inventory without registry credentials", async () => {
    const fetcher = vi.fn();
    await expect(discoverAuthorizationCandidates({}, fetcher)).resolves.toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("fails closed and cancels an oversized streaming response", async () => {
    let cancelled = false;
    const fetcher = vi.fn().mockResolvedValue({
      ok: true,
      body: new ReadableStream({
        start(controller) {
          controller.enqueue(new Uint8Array(256 * 1024));
          controller.enqueue(new Uint8Array(1));
        },
        cancel() {
          cancelled = true;
        },
      }),
    } as Response);
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, fetcher,
    )).resolves.toBeNull();
    expect(cancelled).toBe(true);
  });

  it("fails closed on an invalid registry URL before fetching", async () => {
    const fetcher = vi.fn();
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "::invalid-url::", RECONCILER_REGISTRY_AUTH_KEY: "k" }, fetcher,
    )).resolves.toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("refuses a non-HTTPS registry before fetching", async () => {
    const fetcher = vi.fn();
    await expect(discoverAuthorizationCandidates(
      { RECONCILER_REGISTRY_URL: "http://registry.test/repos", RECONCILER_REGISTRY_AUTH_KEY: "k" }, fetcher,
    )).resolves.toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });
});

describe("durable reconciliation handoff", () => {
  function kv() {
    const values = new Map<string, string>();
    return {
      values,
      get: vi.fn(async (key: string) => values.get(key) ?? null),
      put: vi.fn(async (key: string, value: string) => { values.set(key, value); }),
      delete: vi.fn(async (key: string) => { values.delete(key); }),
    };
  }

  it("deduplicates the same repo/job and releases after handoff", async () => {
    const store = kv();
    const handoff = {
      schema_version: 1 as const,
      repo: "acme/customer",
      job_id: "99",
      installation_id: "42",
      labels: ["corelink"],
      enqueued_at_ms: 1,
    };
    await expect(claimReconcileHandoff(store, handoff, 1_000)).resolves.toBe(true);
    await expect(claimReconcileHandoff(store, handoff, 1_000)).resolves.toBe(false);
    expect(store.values.has(reconcileHandoffKey("acme/customer", "99"))).toBe(true);
    await releaseReconcileHandoff(store, "acme/customer", "99");
    await expect(claimReconcileHandoff(store, handoff, 1_000)).resolves.toBe(true);
  });
});
