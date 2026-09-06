import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { CredStashDO } from "../src/index";
import { buildContainerEnv, type CredStashLike, type MintEnv } from "../src/lib";
import { runnerCredentialLeaseId } from "../src/lib/runner_credential_lease";
import { revokeIssuedCredential, retryFailedRevocations } from "../src/lib/revocation_outbox";

function makeDO() {
  const map = new Map<string, unknown>();
  const storage = {
    async get<T>(key: string) { return map.get(key) as T | undefined; },
    async put(key: string, value: unknown) { map.set(key, value); },
    async delete(key: string) { map.delete(key); },
    async deleteAll() { map.clear(); },
    async deleteAlarm() {},
    async setAlarm(_at: number) {},
  };
  let gate = Promise.resolve();
  const ctx = { storage, blockConcurrencyWhile: (fn: () => Promise<unknown>) => {
    const result = gate.then(fn);
    gate = result.then(() => undefined, () => undefined);
    return result;
  } } as never;
  return new CredStashDO(ctx, {} as never);
}

describe("exact minted credential stash identity", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("maps concurrent same-job PATs to distinct real CredStashDO leases", async () => {
    const leases = new Map<string, CredStashDO>();
    const stash: CredStashLike = {
      stash: async (leaseId, ticket, cred, ttlMs) => {
        const doInst = leases.get(leaseId) ?? makeDO();
        leases.set(leaseId, doInst);
        return doInst.stash(ticket, cred, ttlMs);
      },
    };
    let mint = 0;
    vi.stubGlobal("fetch", vi.fn(async () => {
      mint++;
      const suffix = mint === 1 ? "a" : "b";
      return new Response(JSON.stringify({ token_plaintext: `token-${suffix}`, pat_id: `pat-${suffix}`, tenant: "tenant-a", max_concurrency: 2 }), { status: 200 });
    }));
    const env: MintEnv = { CORELINK_RUNNER_MINT_AUTH_KEY: "key", CORELINK_MINT_URL: "https://mint.invalid" };
    const params = (repoFullName: string) => ({ jobId: "same-job", repoFullName, installationId: 7 });
    const [a, b] = await Promise.all([
      buildContainerEnv(env, params("repo-a"), { stash, fabricEndpoint: "https://fabric.invalid" }),
      buildContainerEnv(env, params("repo-b"), { stash, fabricEndpoint: "https://fabric.invalid" }),
    ]);
    expect(a.authz).toBe("ok");
    expect(b.authz).toBe("ok");
    expect(a.containerEnv.CLW_LEASE_ID).toBe(runnerCredentialLeaseId("same-job", "tenant-a", "pat-a"));
    expect(b.containerEnv.CLW_LEASE_ID).toBe(runnerCredentialLeaseId("same-job", "tenant-a", "pat-b"));
    expect(a.containerEnv.CLW_LEASE_ID).not.toBe(b.containerEnv.CLW_LEASE_ID);
    expect((await leases.get(a.containerEnv.CLW_LEASE_ID)!.redeem(a.containerEnv.CLW_CRED_TICKET)).status).toBe(200);
    expect((await leases.get(b.containerEnv.CLW_LEASE_ID)!.redeem(b.containerEnv.CLW_CRED_TICKET)).status).toBe(200);
  });

  it("wipes only the exact revoked PAT lease and leaves the winning PAT live", async () => {
    const a = { jobId: "same-job", tenant: "tenant-a", patId: "pat-a" };
    const b = { ...a, patId: "pat-b" };
    const leases = new Map([[runnerCredentialLeaseId(a.jobId, a.tenant, a.patId), makeDO()], [runnerCredentialLeaseId(b.jobId, b.tenant, b.patId), makeDO()]]);
    await leases.get(runnerCredentialLeaseId(a.jobId, a.tenant, a.patId))!.stash("ticket-a", { token: "token-a", endpoint: "https://fabric.invalid", tenant: a.tenant }, 60_000);
    await leases.get(runnerCredentialLeaseId(b.jobId, b.tenant, b.patId))!.stash("ticket-b", { token: "token-b", endpoint: "https://fabric.invalid", tenant: b.tenant }, 60_000);
    const authority = { requestCredentialRevocation: vi.fn(async () => {}), confirmCredentialRevoked: vi.fn(async () => {}) } as never;
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 204 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "key", CORELINK_MINT_URL: "https://mint.invalid", CRED_STASH: {
      idFromName: (name: string) => name,
      get: (name: unknown) => leases.get(name as string)!,
    } };
    await expect(revokeIssuedCredential(env, authority, a)).resolves.toBe(true);
    expect((await leases.get(runnerCredentialLeaseId(a.jobId, a.tenant, a.patId))!.redeem("ticket-a")).status).toBe(404);
    expect((await leases.get(runnerCredentialLeaseId(b.jobId, b.tenant, b.patId))!.redeem("ticket-b")).status).toBe(200);
  });

  it("keeps the obligation requested when wipe fails, then confirms on cron retry", async () => {
    const identity = { jobId: "wipe-retry", tenant: "tenant-a", patId: "pat-a" };
    const authority = new CredStashAuthority();
    await authority.registerCredential(identity);
    const stashDO = makeDO();
    await stashDO.stash("ticket-a", { token: "token-a", endpoint: "https://fabric.invalid", tenant: identity.tenant }, 60_000);
    let wipeAttempts = 0;
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "key", CORELINK_MINT_URL: "https://mint.invalid",
      CRED_STASH: { idFromName: (name: string) => name, get: () => ({ wipe: async () => {
        wipeAttempts++;
        if (wipeAttempts === 1) throw new Error("stash unavailable");
        await stashDO.wipe();
      } }) },
    };
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 204 })));
    await expect(revokeIssuedCredential(env, authority, identity)).resolves.toBe(false);
    expect((await authority.revocationRequestedCredentials()).records).toEqual([identity]);
    await expect(retryFailedRevocations(env, authority)).resolves.toBe(1);
    expect((await authority.revocationRequestedCredentials()).records).toEqual([]);
    expect(wipeAttempts).toBe(2);
    expect((await stashDO.redeem("ticket-a")).status).toBe(404);
  });
});

class CredStashAuthority {
  private records = new Map<string, { jobId: string; tenant: string; patId: string; status: string }>();
  async registerCredential(identity: { jobId: string; tenant: string; patId: string }) { this.records.set(identity.patId, { ...identity, status: "registered" }); }
  async requestCredentialRevocation(identity: { patId: string }) { const record = this.records.get(identity.patId); if (!record) throw new Error("missing"); record.status = "revoke_requested"; }
  async confirmCredentialRevoked(identity: { patId: string }) { const record = this.records.get(identity.patId); if (!record) throw new Error("missing"); record.status = "revoked"; }
  async closeJobCredentials() { return { known: true }; }
  async revocationRequestedCredentials() { return { records: [...this.records.values()].filter(record => record.status === "revoke_requested").map(({ jobId, tenant, patId }) => ({ jobId, tenant, patId })), complete: true }; }
  async pendingCredentials() { return { records: [], complete: true }; }
}
