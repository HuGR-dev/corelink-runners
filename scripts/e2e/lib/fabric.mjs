// Live-fabric client for the e2e suite — the surfaces a REAL USER can reach.
//
// Real-user fidelity: this hits the public HTTP surface exactly as a user would (a PAT in
// the Authorization header, the /v1 routes). It never fabricates privileged /internal calls
// as a *stimulus*. The PAT is loaded from env or the OOB secret file and is NEVER logged.
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const BASE = process.env.FABRIC_BASE_URL || 'https://corelink-fabricd.gmhelmold.workers.dev';

// The standing f0005 tenant PAT (proven live at review). Env wins; else the OOB file.
export function loadPat() {
  if (process.env.FABRIC_F0005_PAT) return process.env.FABRIC_F0005_PAT.trim();
  try {
    return readFileSync(join(homedir(), '.hugit/secrets/f0005-runners-item4-acquiring-pat.txt'), 'utf8').trim();
  } catch {
    return null;
  }
}

// Live cells only run when explicitly asked (E2E_LIVE=1) — never in the pure CI gate.
export const LIVE = process.env.E2E_LIVE === '1';

// A single request. Returns {status, text, json, headers} — the caller asserts BEHAVIOR.
export async function req(method, path, { pat, body, headers = {} } = {}) {
  const h = { ...headers };
  if (pat) h.authorization = `Bearer ${pat}`;
  if (body !== undefined) h['content-type'] = 'application/json';
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: h,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000),
  });
  const text = await res.text();
  let json;
  try { json = JSON.parse(text); } catch { /* not json */ }
  const outHeaders = {};
  for (const [k, v] of res.headers) outHeaders[k] = v;
  return { status: res.status, text, json, headers: outHeaders };
}

// A deliberately-invalid image so an authed acquire stops at server-side image validation
// (400 = PAST auth) and never spawns a real box. The no-side-effect probe.
export const DUMMY_ACQUIRE = {
  image_digest: 'sha256:0000000000000000000000000000000000000000000000000000000000000000',
  net_policy: 'deny-all',
  tmp_root: '/tmp/e2e-probe',
  expiry_ms: 60000,
};

// Assert a captured body carries NO secret-shaped material (G2 non-leak invariant).
const SECRET_RE = /(bearer\s+[a-z0-9._-]{16,})|(pat_[a-z0-9]{12,})|(-----BEGIN)|(sk-[a-z0-9]{16,})/i;
export function bodyLeaksSecret(text) {
  return SECRET_RE.test(text || '');
}
