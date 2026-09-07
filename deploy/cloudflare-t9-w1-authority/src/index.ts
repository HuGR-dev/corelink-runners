export interface Env {
  T9_AUTHORITY_DB: D1Database;
  T9_AUTHORITY_ID: string;
  T9_GRANT_KEY_ID: string;
  T9_GRANT_PUBLIC_KEY: string;
  T9_TERMINAL_PUBLIC_KEY: string;
  T9_TERMINAL_SIGNING_PRIVATE_KEY: string;
  T9_MAX_VCPU_MS: string;
  T9_MAX_WALL_MS: string;
  T9_EXPIRES_AT_MS: string;
  T9_EXECUTOR_PROTOCOL: string;
}

type Grant = {
  v: 1; key_id: string; tenant_id: string; workload_kind: "devenv"; workload_id: string;
  reservation_id: string; period_key: number; ceiling_vcpu_ms: string; vcpu_count: number;
  maximum_wall_ms: number; issued_at_ms: number; expires_at_ms: number;
};
type Receipt = {
  reservation_id: string; state: "cancelled"; materialized: false; actual_vcpu_ms: "0";
  evidence_digest: string; future_materialization_fence: string; terminal_authority: string;
  authority_signature: string;
};
type Reservation = { grant_digest: string; state: "prepared" | "cancelled"; receipt_json: string | null };

const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const WORKLOAD = /^[A-Za-z0-9:_./-]{1,256}$/;
const KEY = /^[A-Za-z0-9_-]{1,64}$/;
const DIGEST = /^[0-9a-f]{64}$/;
const DECIMAL = /^(0|[1-9][0-9]{0,18})$/;
const MAX_TOKEN_BYTES = 8192;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const expiry = parseNonnegative(env.T9_EXPIRES_AT_MS);
    if (request.method === "GET" && url.pathname === "/internal/v1/compute/acceptance-binding") {
      return json({
        authority_origin: url.origin, authority_id: env.T9_AUTHORITY_ID,
        grant_key_id: env.T9_GRANT_KEY_ID, grant_public_key: env.T9_GRANT_PUBLIC_KEY,
        terminal_public_key: env.T9_TERMINAL_PUBLIC_KEY, max_vcpu_ms: env.T9_MAX_VCPU_MS,
        max_wall_ms: env.T9_MAX_WALL_MS, expires_at_ms: env.T9_EXPIRES_AT_MS,
        executor_protocol: env.T9_EXECUTOR_PROTOCOL, compute_materialization: "disabled"
      });
    }
    if (Date.now() >= expiry) return json({ error: "acceptance_target_expired" }, 410);
    if (request.method !== "POST") return json({ error: "not_found" }, 404);
    const match = /^\/internal\/v1\/compute\/(reserve|activate|cancel|settle)$/.exec(url.pathname);
    if (!match) return json({ error: "not_found" }, 404);
    if ((request.headers.get("content-type") || "").split(";", 1)[0] !== "application/json") return json({ error: "invalid_request" }, 400);
    const token = request.headers.get("authorization")?.replace(/^ComputeGrant /, "");
    if (!token || token === request.headers.get("authorization")) return json({ error: "invalid_compute_grant" }, 401);
    let grant: Grant;
    let digest: string;
    try { ({ grant, digest } = await verifyGrant(token, env, expiry, match[1] === "cancel")); } catch { return json({ error: "invalid_compute_grant" }, 401); }
    let body: unknown;
    try { body = await request.json(); } catch { return json({ error: "invalid_request" }, 400); }
    if (match[1] === "reserve") return reserve(env, grant, digest, body);
    if (match[1] === "activate") return activate(env, grant, digest, body);
    if (match[1] === "settle") return json({ error: "executor_protocol_unpromoted" }, 503);
    if (!isEmptyObject(body)) return json({ error: "invalid_request" }, 400);
    return cancel(env, grant, digest);
  }
} satisfies ExportedHandler<Env>;

async function reserve(env: Env, grant: Grant, digest: string, body: unknown): Promise<Response> {
  if (!isEmptyObject(body)) return json({ error: "invalid_request" }, 400);
  const required = BigInt(grant.vcpu_count) * BigInt(grant.maximum_wall_ms);
  const ceiling = BigInt(grant.ceiling_vcpu_ms);
  const maximum = BigInt(parsePositive(env.T9_MAX_VCPU_MS));
  if (required > ceiling || required > maximum) return json({ error: "monthly_compute_refused" }, 429);
  const current = await env.T9_AUTHORITY_DB.prepare("SELECT grant_digest,state,receipt_json FROM reservations WHERE reservation_id=?1")
    .bind(grant.reservation_id).first<Reservation>();
  if (current) {
    if (current.grant_digest !== digest) return json({ error: "reservation_conflict" }, 409);
    return current.state === "prepared" ? json({ reservation_id: grant.reservation_id, state: "prepared" }) : json({ error: "reservation_cancelled" }, 409);
  }
  const used = await env.T9_AUTHORITY_DB.prepare("SELECT COALESCE(SUM(CAST(required_vcpu_ms AS INTEGER)),0) AS used FROM reservations WHERE tenant_id=?1 AND state='prepared'")
    .bind(grant.tenant_id).first<{ used: number }>();
  if (BigInt(used?.used ?? 0) + required > ceiling) return json({ error: "monthly_compute_refused" }, 429);
  await env.T9_AUTHORITY_DB.prepare("INSERT INTO reservations(reservation_id,grant_digest,tenant_id,workload_id,required_vcpu_ms,ceiling_vcpu_ms,state,created_at_ms) VALUES(?1,?2,?3,?4,?5,?6,'prepared',?7)")
    .bind(grant.reservation_id, digest, grant.tenant_id, grant.workload_id, required.toString(), grant.ceiling_vcpu_ms, Date.now()).run();
  return json({ reservation_id: grant.reservation_id, state: "prepared" });
}

async function activate(env: Env, grant: Grant, digest: string, body: unknown): Promise<Response> {
  if (!isEmptyObject(body)) return json({ error: "invalid_request" }, 400);
  const current = await env.T9_AUTHORITY_DB.prepare("SELECT grant_digest,state,receipt_json FROM reservations WHERE reservation_id=?1")
    .bind(grant.reservation_id).first<Reservation>();
  if (!current || current.grant_digest !== digest) return json({ error: "reservation_conflict" }, 409);
  // This fence must remain before any future executor integration: once cancel is
  // durable, an older grant can never be revived into provider materialization.
  if (current.state === "cancelled") return json({ error: "reservation_cancelled" }, 409);
  return json({ error: "executor_protocol_unpromoted" }, 503);
}

async function cancel(env: Env, grant: Grant, digest: string): Promise<Response> {
  const current = await env.T9_AUTHORITY_DB.prepare("SELECT grant_digest,state,receipt_json FROM reservations WHERE reservation_id=?1")
    .bind(grant.reservation_id).first<Reservation>();
  if (current?.grant_digest && current.grant_digest !== digest) return json({ error: "reservation_conflict" }, 409);
  if (current?.state === "cancelled" && current.receipt_json) return new Response(current.receipt_json, { headers: { "content-type": "application/json" } });
  const receipt = await signedCancellation(env, grant.reservation_id, digest);
  if (current) {
    await env.T9_AUTHORITY_DB.prepare("UPDATE reservations SET state='cancelled',receipt_json=?2 WHERE reservation_id=?1").bind(grant.reservation_id, JSON.stringify(receipt)).run();
  } else {
    await env.T9_AUTHORITY_DB.prepare("INSERT INTO reservations(reservation_id,grant_digest,tenant_id,workload_id,required_vcpu_ms,ceiling_vcpu_ms,state,created_at_ms,receipt_json) VALUES(?1,?2,?3,?4,'0',?5,'cancelled',?6,?7)")
      .bind(grant.reservation_id, digest, grant.tenant_id, grant.workload_id, grant.ceiling_vcpu_ms, Date.now(), JSON.stringify(receipt)).run();
  }
  return json(receipt);
}

async function signedCancellation(env: Env, reservationId: string, grantDigest: string): Promise<Receipt> {
  const evidence = await sha256(`t9-w1-cancel:${reservationId}:${grantDigest}`);
  const fence = await sha256(`t9-w1-fence:${reservationId}:${grantDigest}`);
  const unsigned = { reservation_id: reservationId, state: "cancelled" as const, materialized: false as const, actual_vcpu_ms: "0" as const, evidence_digest: evidence, future_materialization_fence: fence, terminal_authority: env.T9_AUTHORITY_ID };
  const privateKey = await crypto.subtle.importKey("pkcs8", fromBase64url(env.T9_TERMINAL_SIGNING_PRIVATE_KEY), { name: "Ed25519" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("Ed25519", privateKey, new TextEncoder().encode(JSON.stringify(unsigned)));
  return { ...unsigned, authority_signature: base64url(new Uint8Array(signature)) };
}

async function verifyGrant(token: string, env: Env, targetExpiry: number, allowExpired: boolean): Promise<{ grant: Grant; digest: string }> {
  if (new TextEncoder().encode(token).byteLength > MAX_TOKEN_BYTES) throw new Error("malformed");
  const [payloadPart, signaturePart, extra] = token.split(".");
  if (!payloadPart || !signaturePart || extra !== undefined) throw new Error("malformed");
  const payloadBytes = fromBase64url(payloadPart); const signature = fromBase64url(signaturePart);
  if (signature.byteLength !== 64) throw new Error("malformed");
  const raw: unknown = JSON.parse(new TextDecoder().decode(payloadBytes));
  if (!validGrant(raw, env, targetExpiry, allowExpired)) throw new Error("malformed");
  const publicKey = await crypto.subtle.importKey("raw", fromBase64url(env.T9_GRANT_PUBLIC_KEY), { name: "Ed25519" }, false, ["verify"]);
  if (!await crypto.subtle.verify("Ed25519", publicKey, signature, payloadBytes)) throw new Error("malformed");
  return { grant: raw, digest: await sha256(token) };
}

function validGrant(value: unknown, env: Env, targetExpiry: number, allowExpired: boolean): value is Grant {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const g = value as Record<string, unknown>; const keys = ["v","key_id","tenant_id","workload_kind","workload_id","reservation_id","period_key","ceiling_vcpu_ms","vcpu_count","maximum_wall_ms","issued_at_ms","expires_at_ms"];
  if (Object.keys(g).length !== keys.length || keys.some(key => !(key in g))) return false;
  const period = g.period_key as number; const month = period % 100;
  return g.v === 1 && g.key_id === env.T9_GRANT_KEY_ID && typeof g.tenant_id === "string" && UUID.test(g.tenant_id) && g.workload_kind === "devenv" && typeof g.workload_id === "string" && WORKLOAD.test(g.workload_id) && typeof g.reservation_id === "string" && UUID.test(g.reservation_id) && Number.isSafeInteger(period) && period >= 197001 && period <= 999912 && month >= 1 && month <= 12 && typeof g.ceiling_vcpu_ms === "string" && DECIMAL.test(g.ceiling_vcpu_ms) && BigInt(g.ceiling_vcpu_ms) > 0n && typeof g.vcpu_count === "number" && g.vcpu_count === 1 && typeof g.maximum_wall_ms === "number" && Number.isSafeInteger(g.maximum_wall_ms) && g.maximum_wall_ms > 0 && g.maximum_wall_ms <= parsePositive(env.T9_MAX_WALL_MS) && typeof g.issued_at_ms === "number" && typeof g.expires_at_ms === "number" && Number.isSafeInteger(g.issued_at_ms) && Number.isSafeInteger(g.expires_at_ms) && g.issued_at_ms <= g.expires_at_ms && (allowExpired || Date.now() < g.expires_at_ms) && g.expires_at_ms <= targetExpiry && g.expires_at_ms - g.issued_at_ms <= 90_000;
}

function isEmptyObject(value: unknown): value is Record<string, never> { return !!value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === 0; }
function parsePositive(value: string): number { if (!/^[1-9][0-9]*$/.test(value)) throw new Error("invalid authority configuration"); return Number(value); }
function parseNonnegative(value: string): number { if (!/^[0-9]+$/.test(value)) throw new Error("invalid authority configuration"); return Number(value); }
function json(value: unknown, status = 200): Response { return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json", "cache-control": "no-store" } }); }
function fromBase64url(value: string): Uint8Array { const padded = value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4); return Uint8Array.from(atob(padded), c => c.charCodeAt(0)); }
function base64url(bytes: Uint8Array): string { let binary = ""; for (const byte of bytes) binary += String.fromCharCode(byte); return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""); }
async function sha256(value: string): Promise<string> { const bytes = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value))); return Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join(""); }
