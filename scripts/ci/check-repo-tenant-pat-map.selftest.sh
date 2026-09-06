#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY'
import json
import os
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

guard = Path("scripts/ci/check-repo-tenant-pat-map.py").resolve()

class Handler(BaseHTTPRequestHandler):
    scenario = {}
    def do_GET(self):
        key = "deployments" if self.path.endswith("/deployments") else "settings" if self.path.endswith("/settings") else "other"
        value = self.scenario.get(key, (200, {}))
        status, body = value
        encoded = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)
    def log_message(self, *_):
        pass

server = HTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = "http://127.0.0.1:%d" % server.server_port

def run(map_value, deployments, settings):
    with tempfile.NamedTemporaryFile("w", suffix=".jsonc") as config:
        config.write('{"name":"corelink-spawn-worker","account_id":"acct","vars":{"REPO_TENANT_PAT_MAP":%s}}' % json.dumps(map_value))
        config.flush()
        Handler.scenario = {"deployments": deployments, "settings": settings}
        env = dict(os.environ, CLOUDFLARE_API_TOKEN="fixture", CLOUDFLARE_API_BASE=base)
        return subprocess.run(["python3", str(guard), config.name], env=env, text=True, capture_output=True)

def run_raw(raw):
    with tempfile.NamedTemporaryFile("w", suffix=".jsonc") as config:
        config.write(raw)
        config.flush()
        Handler.scenario = {"deployments": ok_deploy, "settings": live_empty}
        env = dict(os.environ, CLOUDFLARE_API_TOKEN="fixture", CLOUDFLARE_API_BASE=base)
        return subprocess.run(["python3", str(guard), config.name], env=env, text=True, capture_output=True)

ok_deploy = (200, {"success": True, "result": [{"id": "deployment"}]})
live_nonempty = (200, {"success": True, "result": {"bindings": [{"name": "REPO_TENANT_PAT_MAP", "type": "plain_text", "text": '{"a/b":"SECRET"}'}]}})
live_empty = (200, {"success": True, "result": {"bindings": []}})

assert run("{}", ok_deploy, live_nonempty).returncode != 0
assert run('{"a/b":"SECRET"}', ok_deploy, live_nonempty).returncode == 0
assert run("{}", ok_deploy, live_empty).returncode == 0
assert run("{", ok_deploy, live_empty).returncode != 0
assert run("{}", ok_deploy, (200, {"success": True, "result": {"bindings": [{"name": "REPO_TENANT_PAT_MAP", "type": "secret_text", "text": "x"}]}})).returncode != 0
assert run("{}", (200, {"success": True, "result": []}), live_empty).returncode != 0
assert run("{}", (403, {"success": False, "result": None}), live_empty).returncode != 0
assert run_raw('// "name":"decoy"\n{"name":"corelink-spawn-worker","account_id":"acct","vars":{"REPO_TENANT_PAT_MAP":"{}"}}').returncode == 0
bad = run_raw('{"name":"corelink-spawn-worker",')
assert bad.returncode != 0 and "decoy" not in bad.stdout + bad.stderr
Handler.scenario = {"deployments": (302, {"location": base + "/other"}), "settings": live_empty}
redirect = run("{}", Handler.scenario["deployments"], live_empty)
assert redirect.returncode != 0

workflow = Path(".github/workflows/deploy-spawn-worker.yml").read_text()
guard_at = workflow.index("scripts/ci/check-repo-tenant-pat-map.py")
deploy_at = workflow.index("npx wrangler deploy")
assert guard_at < deploy_at
print("check-repo-tenant-pat-map selftest: 7 scenarios passed; workflow ordering passed")
server.shutdown()
PY
