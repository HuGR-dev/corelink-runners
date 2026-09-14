#!/usr/bin/env node
// Network-free contract test for the A2.8 raw Containers rollout helper.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { once } from 'node:events';

const here = new URL('.', import.meta.url);
const helper = new URL('./a28-fabricd-same-config-rollout.mjs', here).pathname;
const account = '6a1fc1aa-bbbb-cccc-dddd-eeeeeeeeeeee';
const app = 'a0325be3-f845-460f-95f6-ae678ec46a94';
const digest = 'sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5';
const configuration = {
  image: `registry.cloudflare.com/corelink/fabricd@${digest}`,
  environment_variables: [{ name: 'UNCHANGED', value: 'opaque-provider-value' }],
  command: ['corelink-fabricd'],
  observability: { logs: { enabled: true } },
};

function invoke(args) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [helper, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = ''; let stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('close', code => resolve({ code, stdout, stderr }));
  });
}

async function serverFor(mode) {
  const calls = [];
  let versionPolls = 0;
  const server = createServer(async (req, res) => {
    let body = '';
    for await (const chunk of req) body += chunk;
    calls.push({ method: req.method, path: req.url, body, authorization: req.headers.authorization });
    assert.equal(req.headers.authorization, 'Bearer mock-oauth-token-not-a-secret');
    const respond = result => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ success: true, result })); };
    if (req.method === 'GET' && req.url.endsWith(`/applications/${app}`)) return respond({ id: app, account_id: account, version: 1, configuration });
    if (req.method === 'POST' && req.url.endsWith('/rollouts')) return respond({ id: `rollout-${calls.filter(c => c.method === 'POST').length}`, status: 'progressing', target_version: 2 });
    if (req.method === 'GET' && req.url.endsWith('/versions')) {
      if (mode === 'stalled') return;
      versionPolls += 1;
      return respond(mode === 'success' && versionPolls >= 2
        ? [{ version: 2, percentage: 100, configuration }]
        : [{ version: 2, percentage: 50, configuration }]);
    }
    res.statusCode = 404; res.end(JSON.stringify({ success: false }));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  return { server, calls, apiBase: `http://127.0.0.1:${port}/client/v4` };
}

const temp = await mkdtemp(join(tmpdir(), 'a28-rollout-selftest-'));
const fakeWrangler = join(temp, 'wrangler');
await writeFile(fakeWrangler, '#!/usr/bin/env node\nprocess.stdout.write(JSON.stringify({token:"mock-oauth-token-not-a-secret"}));\n', { mode: 0o700 });
await chmod(fakeWrangler, 0o700);

try {
  const success = await serverFor('success');
  const common = ['--execute', '--ack-destructive', '--account-id', account, '--application-id', app, '--expected-digest', digest, '--wrangler-command', fakeWrangler, '--api-base', success.apiBase, '--attempts', '3', '--poll-ms', '0'];
  const result = await invoke(common);
  success.server.close();
  assert.equal(result.code, 0, result.stderr);
  const posts = success.calls.filter(c => c.method === 'POST');
  assert.equal(posts.length, 1);
  const payload = JSON.parse(posts[0].body);
  assert.deepEqual(payload.target_configuration, configuration, 'POST must copy authenticated GET configuration exactly');
  assert.equal(payload.step_percentage, 100);
  assert.equal(success.calls.filter(c => c.path.endsWith('/versions')).length, 2, 'must poll until 100%');
  assert.equal(JSON.parse(result.stdout).status, 'completed');
  assert.equal(`${result.stdout}${result.stderr}`.includes('mock-oauth-token-not-a-secret'), false, 'OAuth token must never be emitted');

  const failure = await serverFor('timeout');
  const failed = await invoke([...common.slice(0, common.indexOf('--api-base')), '--api-base', failure.apiBase, '--attempts', '1', '--poll-ms', '0']);
  failure.server.close();
  assert.notEqual(failed.code, 0, 'timeout must fail');
  const rollbackPosts = failure.calls.filter(c => c.method === 'POST');
  assert.equal(rollbackPosts.length, 2, 'failure must issue a same-config rollback');
  assert.deepEqual(JSON.parse(rollbackPosts[1].body).target_configuration, configuration, 'rollback must restore original GET config');

  const stalled = await serverFor('stalled');
  const stalledResult = await invoke([...common.slice(0, common.indexOf('--api-base')), '--api-base', stalled.apiBase, '--attempts', '1', '--poll-ms', '0', '--request-timeout-ms', '1000']);
  stalled.server.close();
  assert.notEqual(stalledResult.code, 0, 'a stalled API response must fail closed');
  assert.match(stalledResult.stderr, /request timed out after 1000ms/);
  assert.ok(stalled.calls.some(c => c.path.endsWith('/versions')), 'stalled versions endpoint must have been reached');
  assert.equal(stalled.calls.filter(c => c.method === 'POST').length, 2, 'a timed-out poll must still request same-config rollback');

  const preflight = await serverFor('success');
  const readOnly = await invoke(['--account-id', account, '--application-id', app, '--expected-digest', digest, '--wrangler-command', fakeWrangler, '--api-base', preflight.apiBase]);
  preflight.server.close();
  assert.equal(readOnly.code, 0, readOnly.stderr);
  assert.equal(preflight.calls.filter(c => c.method === 'POST').length, 0, 'preflight must be read-only');
  process.stdout.write('A2.8 same-config raw rollout selftest: PASS\n');
} finally {
  await rm(temp, { recursive: true, force: true });
}
