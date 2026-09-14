#!/usr/bin/env node
/**
 * Restart Fabricd containers without publishing a Worker version.
 *
 * The Containers API accepts a rollout target configuration.  This helper reads
 * the authenticated application's exact configuration and sends that object
 * back as the target. It never invokes the Worker deployment command, so it cannot change
 * Worker code or bindings.
 *
 * The OAuth credential is obtained from Wrangler's existing auth store and is
 * held only in this Node process.  In particular, it is never put in argv,
 * environment, a file, stderr, or the JSON result.
 */
import { spawnSync } from 'node:child_process';

const DEFAULT_API = 'https://api.cloudflare.com/client/v4';
const DEFAULT_ATTEMPTS = 30;
const DEFAULT_POLL_MS = 2_000;
const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;

function die(message) { throw new Error(message); }

function parseArgs(argv) {
  const opts = { apiBase: DEFAULT_API, attempts: DEFAULT_ATTEMPTS, pollMs: DEFAULT_POLL_MS, requestTimeoutMs: DEFAULT_REQUEST_TIMEOUT_MS, execute: false, ack: false, wranglerCommand: 'npx' };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--execute') opts.execute = true;
    else if (arg === '--ack-destructive') opts.ack = true;
    else if (arg === '--account-id') opts.accountId = argv[++i];
    else if (arg === '--application-id') opts.applicationId = argv[++i];
    else if (arg === '--expected-digest') opts.expectedDigest = argv[++i];
    else if (arg === '--wrangler-dir') opts.wranglerDir = argv[++i];
    else if (arg === '--wrangler-command') opts.wranglerCommand = argv[++i];
    else if (arg === '--api-base') opts.apiBase = argv[++i];
    else if (arg === '--attempts') opts.attempts = Number(argv[++i]);
    else if (arg === '--poll-ms') opts.pollMs = Number(argv[++i]);
    else if (arg === '--request-timeout-ms') opts.requestTimeoutMs = Number(argv[++i]);
    else if (arg === '--help' || arg === '-h') {
      process.stdout.write('Usage: a28-fabricd-same-config-rollout.mjs --execute --ack-destructive --account-id ID --application-id ID --expected-digest sha256:... [--wrangler-dir DIR] [--request-timeout-ms 30000]\n');
      process.exit(0);
    } else die(`unknown or incomplete option: ${arg}`);
  }
  for (const key of ['accountId', 'applicationId', 'expectedDigest']) if (!opts[key]) die(`missing --${key.replace(/[A-Z]/g, c => `-${c.toLowerCase()}`)}`);
  if (!/^sha256:[0-9a-f]{64}$/.test(opts.expectedDigest)) die('expected digest must be sha256 followed by 64 lowercase hex characters');
  if (!Number.isInteger(opts.attempts) || opts.attempts < 1 || opts.attempts > 300) die('attempts must be an integer from 1 to 300');
  if (!Number.isInteger(opts.pollMs) || opts.pollMs < 0 || opts.pollMs > 60_000) die('poll-ms must be an integer from 0 to 60000');
  if (!Number.isInteger(opts.requestTimeoutMs) || opts.requestTimeoutMs < 1 || opts.requestTimeoutMs > 120_000) die('request-timeout-ms must be an integer from 1 to 120000');
  if (opts.execute !== opts.ack) die('both --execute and --ack-destructive are required for a provider mutation');
  return opts;
}

function oauthToken(opts) {
  // `npx --no-install wrangler auth token --json` is Wrangler's supported
  // reader for its local OAuth auth store. Its stdout is captured, never echoed.
  const args = opts.wranglerCommand === 'npx'
    ? ['--no-install', 'wrangler', 'auth', 'token', '--json']
    : ['auth', 'token', '--json'];
  const authEnv = { ...process.env };
  // Auth must come from Wrangler's local OAuth store, never from an API-token
  // environment variable inherited from an interactive shell or CI process.
  delete authEnv.CLOUDFLARE_API_TOKEN;
  delete authEnv.CF_API_TOKEN;
  const result = spawnSync(opts.wranglerCommand, args, {
    cwd: opts.wranglerDir,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    env: authEnv,
  });
  if (result.status !== 0) die('Wrangler OAuth token is unavailable (run wrangler login; token was not printed)');
  let parsed;
  try { parsed = JSON.parse(result.stdout); } catch { die('Wrangler OAuth token response was not JSON'); }
  const token = parsed.token ?? parsed.access_token;
  if (typeof token !== 'string' || !/^[A-Za-z0-9._~+/=-]{16,}$/.test(token)) die('Wrangler OAuth token response was invalid');
  return token;
}

function apiPath(opts, suffix = '') {
  return `${opts.apiBase.replace(/\/$/, '')}/accounts/${encodeURIComponent(opts.accountId)}/containers/applications/${encodeURIComponent(opts.applicationId)}${suffix}`;
}

async function api(token, url, requestTimeoutMs, init = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), requestTimeoutMs);
  try {
    const response = await fetch(url, {
      ...init,
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', ...(init.headers ?? {}) },
      signal: controller.signal,
    });
    let body;
    try { body = await response.json(); } catch { die(`Cloudflare API returned non-JSON HTTP ${response.status}`); }
    if (!response.ok || body?.success !== true) die(`Cloudflare API request failed with HTTP ${response.status}`);
    return body.result;
  } catch (error) {
    if (controller.signal.aborted) die(`Cloudflare API request timed out after ${requestTimeoutMs}ms`);
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(k => [k, stable(value[k])]));
  return value;
}

function assertApplication(app, opts) {
  if (!app || typeof app !== 'object' || Array.isArray(app)) die('application GET response was invalid');
  if (app.id !== opts.applicationId || app.account_id !== opts.accountId) die('application GET identity did not match requested account/application');
  if (!app.configuration || typeof app.configuration !== 'object' || Array.isArray(app.configuration)) die('application GET had no configuration');
  if (typeof app.configuration.image !== 'string' || !app.configuration.image.endsWith(opts.expectedDigest)) die('application GET image digest did not match approved digest');
  if (!Number.isInteger(app.version)) die('application GET version was invalid');
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function run(opts) {
  let token = oauthToken(opts);
  let originalConfiguration;
  let mutationMayHaveStarted = false;
  try {
    const before = await api(token, apiPath(opts), opts.requestTimeoutMs);
    assertApplication(before, opts);
    originalConfiguration = before.configuration;
    const baselineVersion = before.version;
    const plan = { account_id: opts.accountId, application_id: opts.applicationId, before_version: baselineVersion, image_digest: opts.expectedDigest, worker_change: false, binding_change: false };
    if (!opts.execute) {
      process.stdout.write(`${JSON.stringify({ mode: 'preflight', ...plan })}\n`);
      return;
    }
    const payload = { description: 'A2.8 same-configuration Fabricd secret pickup', strategy: 'rolling', kind: 'full_auto', step_percentage: 100, target_configuration: originalConfiguration };
    mutationMayHaveStarted = true; // an HTTP failure can still be ambiguous server-side
    const rollout = await api(token, apiPath(opts, '/rollouts'), opts.requestTimeoutMs, { method: 'POST', body: JSON.stringify(payload) });
    if (!rollout || typeof rollout !== 'object' || !['pending', 'progressing', 'completed'].includes(rollout.status)) die('rollout creation returned an unsafe status');
    // The public API has no GET-by-rollout-id operation. Polling versions is its
    // status surface: the target version must carry the exact copied config at
    // 100% before this operation is considered complete.
    for (let attempt = 1; attempt <= opts.attempts; attempt += 1) {
      const versions = await api(token, apiPath(opts, '/versions'), opts.requestTimeoutMs);
      if (!Array.isArray(versions)) die('application versions response was invalid');
      const target = versions.find(v => v?.version === rollout.target_version);
      if (target && target.percentage === 100 && JSON.stringify(stable(target.configuration)) === JSON.stringify(stable(originalConfiguration))) {
        process.stdout.write(`${JSON.stringify({ mode: 'executed', ...plan, rollout_id: rollout.id, target_version: rollout.target_version, status: 'completed', polls: attempt })}\n`);
        return;
      }
      if (attempt < opts.attempts) await sleep(opts.pollMs);
    }
    throw new Error('rollout did not reach the exact same configuration at 100% before timeout');
  } catch (error) {
    // If creation or polling fails after the first GET, issue an explicit
    // same-config rollback. This API-only request also cannot touch Worker code
    // or bindings. Do not mask the original failure if rollback itself fails.
    try {
      if (!opts.execute || !mutationMayHaveStarted || !originalConfiguration) throw new Error('no rollout was started');
      const app = await api(token, apiPath(opts), opts.requestTimeoutMs);
      assertApplication(app, opts);
      await api(token, apiPath(opts, '/rollouts'), opts.requestTimeoutMs, {
        method: 'POST',
        body: JSON.stringify({ description: 'A2.8 rollback to authenticated same Fabricd configuration', strategy: 'rolling', kind: 'full_auto', step_percentage: 100, target_configuration: originalConfiguration }),
      });
    } catch { /* caller receives a red result; no secret or token is disclosed */ }
    throw error;
  } finally {
    // Make the short-lived credential unreachable before the process exits.
    // eslint-disable-next-line no-param-reassign
    token = undefined;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  run(parseArgs(process.argv.slice(2))).catch(error => {
    process.stderr.write(`same-config Fabricd rollout refused: ${error.message}\n`);
    process.exitCode = 1;
  });
}

export { parseArgs, run, stable };
