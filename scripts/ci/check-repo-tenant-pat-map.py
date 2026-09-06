#!/usr/bin/env python3
"""Fail closed if a deploy would remove an active Option-C PAT map."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


class GuardError(Exception):
    pass


_STRING = r'"(?:\\.|[^"\\])*"'
_REPO = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$")


def _jsonc_string(text: str, key: str) -> str:
    match = re.search(r'"' + re.escape(key) + r'"\s*:\s*(' + _STRING + r')', text)
    if not match:
        raise GuardError("candidate configuration is missing a required field")
    try:
        value = json.loads(match.group(1))
    except (TypeError, ValueError):
        raise GuardError("candidate configuration contains malformed JSONC")
    if not isinstance(value, str):
        raise GuardError("candidate configuration field has an invalid type")
    return value


def _parse_map(value: str) -> int:
    try:
        parsed = json.loads(value)
    except (TypeError, ValueError):
        raise GuardError("REPO_TENANT_PAT_MAP is malformed")
    if not isinstance(parsed, dict):
        raise GuardError("REPO_TENANT_PAT_MAP has an invalid shape")
    seen: set[str] = set()
    for key, secret in parsed.items():
        if not isinstance(key, str) or not _REPO.fullmatch(key.strip()):
            raise GuardError("REPO_TENANT_PAT_MAP contains an invalid repository key")
        canonical = key.strip().lower()
        if canonical in seen:
            raise GuardError("REPO_TENANT_PAT_MAP contains duplicate repository keys")
        seen.add(canonical)
        if not isinstance(secret, str) or not secret:
            raise GuardError("REPO_TENANT_PAT_MAP contains an invalid secret binding")
    return len(parsed)


def _request(base: str, account: str, script: str, suffix: str, token: str) -> object:
    path = "/accounts/%s/workers/scripts/%s/%s" % (
        urllib.parse.quote(account, safe=""), urllib.parse.quote(script, safe=""), suffix
    )
    request = urllib.request.Request(
        base.rstrip("/") + path,
        headers={"Authorization": "Bearer " + token, "Accept": "application/json"},
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.load(response)
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, ValueError, OSError):
        raise GuardError("Cloudflare API request failed")
    if not isinstance(payload, dict) or payload.get("success") is not True:
        raise GuardError("Cloudflare API returned an unsuccessful response")
    return payload.get("result")


def _active_deployment(result: object) -> None:
    if not isinstance(result, list) or not result or not isinstance(result[0], dict):
        raise GuardError("Cloudflare has no unambiguous active deployment")
    active = result[0]
    if not any(isinstance(active.get(field), str) and active[field] for field in ("id", "version_id")):
        raise GuardError("Cloudflare returned an ambiguous active deployment")


def _live_count(result: object) -> int:
    if not isinstance(result, dict) or not isinstance(result.get("bindings"), list):
        raise GuardError("Cloudflare settings response is malformed")
    matches = [binding for binding in result["bindings"] if isinstance(binding, dict) and binding.get("name") == "REPO_TENANT_PAT_MAP"]
    if len(matches) > 1:
        raise GuardError("Cloudflare settings contain duplicate map bindings")
    if not matches:
        return 0
    binding = matches[0]
    if binding.get("type") != "plain_text" or not isinstance(binding.get("text"), str):
        raise GuardError("Cloudflare map binding is malformed")
    return _parse_map(binding["text"])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("config", nargs="?", default="deploy/cloudflare/wrangler.jsonc")
    args = parser.parse_args()
    token = os.environ.get("CLOUDFLARE_API_TOKEN", "")
    if not token:
        print("::error::Cloudflare API token is missing", file=sys.stderr)
        return 1
    try:
        text = Path(args.config).read_text(encoding="utf-8")
        account = _jsonc_string(text, "account_id")
        script = _jsonc_string(text, "name")
        candidate = _parse_map(_jsonc_string(text, "REPO_TENANT_PAT_MAP"))
        base = os.environ.get("CLOUDFLARE_API_BASE", "https://api.cloudflare.com/client/v4")
        _active_deployment(_request(base, account, script, "deployments", token))
        live = _live_count(_request(base, account, script, "settings", token))
        if live > 0 and candidate == 0:
            print("::error::REPO_TENANT_PAT_MAP depletion guard failed (live=nonempty candidate=empty)", file=sys.stderr)
            return 1
        print("REPO_TENANT_PAT_MAP guard passed: candidate_count=%d live_count=%d" % (candidate, live))
        return 0
    except (OSError, GuardError):
        print("::error::REPO_TENANT_PAT_MAP deployment guard failed closed", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
