// Pure, runtime-agnostic helpers for the spawn-Worker. NO `cloudflare:workers` /
// `@cloudflare/containers` imports here — so this module is unit-testable in
// plain vitest (node): only `crypto` + `fetch` (Node 20+ globals) are used.

/** The subset of Env the warm-mint path reads. */
export interface MintEnv {
  CORELINK_PAT_MINT_AUTH_KEY?: string;
  CORELINK_MINT_URL?: string;
  CLW_ENDPOINT?: string;
  CLW_TENANT?: string;
}

// Constant-time string compare (no early-exit on first mismatch) so a bearer/
// signature check can't be timing-probed. Length may leak (fixed-length,
// high-entropy tokens); the byte loop is constant-time.
export function safeEqual(a: string, b: string): boolean {
  const ea = new TextEncoder().encode(a);
  const eb = new TextEncoder().encode(b);
  if (ea.length !== eb.length) return false;
  let diff = 0;
  for (let i = 0; i < ea.length; i++) diff |= ea[i] ^ eb[i];
  return diff === 0;
}

// Verify GitHub's X-Hub-Signature-256 (HMAC-SHA256 of the raw body) in constant
// time. Fail-closed on a missing/short/mismatched signature.
export async function verifyGithubHmac(secret: string, sig: string, body: string): Promise<boolean> {
  if (!sig.startsWith("sha256=")) return false;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return safeEqual(`sha256=${hex}`, sig);
}

// Mint a per-job CAS PAT via D-9 (corelink-server). Scope cas:rw, tenant-scoped
// (A6: per-job, never the tenant PAT). Throws on any failure — the caller falls
// open to a COLD spawn (north star: cache absent ⇒ slow, never broken).
async function mintCasPat(env: MintEnv, jobId: string): Promise<string> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/mint`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_PAT_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify({ owner_tenant: env.CLW_TENANT, job_id: jobId, scope: "cas:rw" }),
  });
  if (!resp.ok) throw new Error(`D-9 mint ${resp.status}`);
  const j = (await resp.json()) as { token?: string };
  if (!j.token) throw new Error("D-9 mint: no token");
  return j.token;
}

// Revoke a per-job CAS PAT via D-9 (corelink-server) — keyed by (owner_tenant,
// job_id), the SAME job_id the PAT was minted under. Mirrors the mint contract
// (header + base URL). Throws on any failure; the caller swallows it (the PAT is
// already TTL-bounded, so revoke is best-effort hardening that shrinks the
// post-job window — its failure is never user-visible). No KV/DO map needed:
// the job_id is the stable GitHub workflow_job.id, present on both the queued
// (mint) and completed (revoke) events.
async function revokeCasPat(env: MintEnv, jobId: string): Promise<void> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/revoke`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_PAT_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify({ owner_tenant: env.CLW_TENANT, job_id: jobId }),
  });
  if (!resp.ok) throw new Error(`D-9 revoke ${resp.status}`);
}

// Best-effort revoke of a completed job's per-job CAS PAT. No-op unless the mint
// is configured (no key ⇒ nothing was minted ⇒ nothing to revoke). Fail-OPEN:
// any error is swallowed (TTL expiry is the backstop) — never breaks a job or
// the webhook response. Returns true iff a revoke was actually issued+ack'd.
export async function maybeRevokeCasPat(env: MintEnv, jobId: string): Promise<boolean> {
  if (!env.CORELINK_PAT_MINT_AUTH_KEY || !env.CLW_TENANT) return false;
  try {
    await revokeCasPat(env, jobId);
    return true;
  } catch (e) {
    console.log(`revoke failed (PAT will TTL-expire): ${(e as Error).message}`);
    return false;
  }
}

// Build the per-job container env: always the JIT; cache-warm CLW_* WHEN the
// D-9 mint is configured AND succeeds. Any mint failure ⇒ cold env (fail-open).
// `jobId` is the stable GitHub workflow_job.id — the PAT is minted under it so a
// later workflow_job:completed can revoke the SAME PAT by (owner_tenant, job_id).
export async function buildContainerEnv(
  env: MintEnv,
  jit: string,
  jobId: string,
): Promise<Record<string, string>> {
  const containerEnv: Record<string, string> = { CORELINK_RUNNER_JITCONFIG: jit };
  if (env.CORELINK_PAT_MINT_AUTH_KEY && env.CLW_TENANT) {
    try {
      const casPat = await mintCasPat(env, jobId);
      containerEnv.CLW_ENDPOINT = env.CLW_ENDPOINT ?? "https://corelink-api.humangr.com";
      containerEnv.CLW_TENANT = env.CLW_TENANT;
      containerEnv.CLW_TOKEN = casPat; // per-job; never logged
      containerEnv.CLW_REF_DOMAIN = "runner";
    } catch (e) {
      // Fail-OPEN: spawn cold (no CLW_*). The entrypoint's cache-warm hook is
      // also fail-open, so a job ALWAYS runs — slow, never broken.
      console.log(`warm-mint failed, spawning COLD: ${(e as Error).message}`);
    }
  }
  return containerEnv;
}
