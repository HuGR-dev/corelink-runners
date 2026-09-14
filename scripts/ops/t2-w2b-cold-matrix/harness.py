#!/usr/bin/env python3
"""Finite, read-only natural sleep/wake acceptance harness for fabricd.

The harness deliberately has no deploy, rollout, restart, or delete code.  An
execution therefore consists of ten idle windows followed by one public
``GET /health`` per window. Provider control-plane reads prove the singleton
inactive lifecycle before the wake and its running transition afterward.

The default CLI mode writes a plan and performs no I/O.  ``--execute`` is
accepted only with ``--ack-execute`` (or one of its explicit aliases), and is
still read-only.  The acknowledgement is an operator acknowledgement of a
time-consuming live probe; it is not permission for mutation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import sys
import subprocess
import shutil
import tempfile
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Protocol
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen


APP_NAME = "corelink-fabricd-fabricdcontainer"
WORKER_NAME = "corelink-spawn-worker"
EXPECTED_DIGEST = "sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"
# Exact provenance pin for the digest. Hashes have no ordering; an exact
# digest-to-build binding is the local equivalent of checking ancestry from
# the #515 fix.
FIX_515_SHA = "313185850eeddc66bb4833598e4acc1e97ad128d"
EXPECTED_BUILD_SHA = "01560b697c87b92bf1572ceec175d5350034aebb"
SLEEP_AFTER_SECONDS = 300
ATTEMPTS = 10
SHA256_RE = re.compile(r"sha256:[0-9a-f]{64}$", re.IGNORECASE)
ISO_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$")


class HarnessError(RuntimeError):
    """A fail-closed harness error safe to print to an operator."""


class CapabilityError(HarnessError):
    """The provider did not expose a reliable witness required by this test."""


@dataclass(frozen=True)
class InstanceSnapshot:
    rows: list[dict[str, Any]]
    total: int | None
    complete: bool


class Provider(Protocol):
    """The deliberately small read-only provider surface used by the runner."""

    def list_apps(self) -> list[dict[str, Any]]: ...

    def list_instances(self, app_id: str) -> InstanceSnapshot: ...

    def instance_detail(self, app_id: str, instance_id: str) -> dict[str, Any]: ...

    def worker_witness(self, worker_name: str, expected_id: str) -> dict[str, Any]: ...

    def public_status(self, path: str, timeout_s: float) -> int: ...

    def public_witness(self, path: str, timeout_s: float) -> dict[str, Any]: ...


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def scalar(row: dict[str, Any], *names: str) -> Any:
    for name in names:
        if name in row:
            return row[name]
    return None


def digest_of(value: Any) -> str | None:
    """Return the immutable digest from a bare digest or an OCI reference."""
    if not isinstance(value, str):
        return None
    if SHA256_RE.fullmatch(value):
        return value.lower()
    match = re.search(r"@?(sha256:[0-9a-f]{64})$", value, re.IGNORECASE)
    return match.group(1).lower() if match else None


def digest_value(row: dict[str, Any]) -> str | None:
    direct = scalar(row, "image_digest", "imageDigest", "digest", "image", "Image")
    found = digest_of(direct)
    if found:
        return found
    config = row.get("configuration")
    if isinstance(config, dict):
        return digest_of(scalar(config, "image_digest", "imageDigest", "digest", "image"))
    return None


def instance_id(row: dict[str, Any]) -> str | None:
    value = scalar(row, "id", "instance_id", "instanceId", "INSTANCE")
    return value.strip() if isinstance(value, str) and value.strip() else None


def created_at(row: dict[str, Any]) -> str | int | float | None:
    value = scalar(row, "created_at", "createdAt", "created", "CREATED")
    if isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0:
        return value
    if isinstance(value, str) and (ISO_RE.fullmatch(value) or value.isdigit()):
        return value
    return None


def state_of(row: dict[str, Any]) -> str | None:
    raw = scalar(row, "state", "status", "STATE")
    if isinstance(raw, dict):
        raw = scalar(raw, "state", "phase", "status")
    # Cloudflare Containers' dashboard/instance endpoint may report placement state as
    # current_placement.status.container_status (the Wrangler adapter flattens
    # this to `state`, but the raw API does not).
    if raw is None:
        placement = row.get("current_placement")
        if isinstance(placement, dict):
            placement_status = placement.get("status")
            if isinstance(placement_status, dict):
                raw = scalar(placement_status, "container_status", "health", "state", "status")
    return raw.lower() if isinstance(raw, str) else None


def expected_digest(value: str | None) -> str:
    parsed = digest_of(value)
    if parsed is None:
        raise HarnessError("configured fabricd image is not an immutable sha256 digest")
    return parsed


def discover_app(provider: Provider, app_name: str, expected: str) -> dict[str, Any]:
    apps = provider.list_apps()
    matches = [row for row in apps if scalar(row, "name", "app_name") == app_name]
    if len(matches) != 1:
        raise HarnessError("provider did not expose exactly one fabricd application")
    app = matches[0]
    app_id = scalar(app, "id", "app_id")
    if not isinstance(app_id, str) or not app_id:
        raise CapabilityError("fabricd application identity is unavailable")
    configured = digest_value(app)
    if configured != expected:
        raise HarnessError("fabricd configured image digest does not match the expected digest")
    # A build identifier is retained when the provider publishes it.  It is a
    # safe provenance value and never contains credentials.
    build_sha = scalar(app, "build_sha", "buildSha", "git_sha", "gitSha", "commit_sha")
    if build_sha is not None and (not isinstance(build_sha, str) or not re.fullmatch(r"[0-9a-fA-F]{7,64}", build_sha)):
        raise CapabilityError("provider build SHA is malformed")
    return {"id": app_id, "name": app_name, "digest": configured, "build_sha": build_sha}


def require_zero(snapshot: InstanceSnapshot) -> None:
    """Require a complete, empty instance enumeration.

    An unfinished cursor or an opaque provider response is not evidence of
    scale-zero. This is intentionally stricter than merely checking that no
    row happened to be returned.
    """
    if not snapshot.complete or snapshot.total is None:
        raise CapabilityError("provider cannot reliably prove scale-zero (incomplete enumeration/count)")
    if snapshot.total != 0 or snapshot.rows:
        raise HarnessError("fabricd was not at scale-zero before wake")


def running_instance(snapshot: InstanceSnapshot, expected: str) -> dict[str, Any]:
    if not snapshot.complete:
        raise CapabilityError("provider instance enumeration is incomplete")
    running = [row for row in snapshot.rows if state_of(row) == "running"]
    if len(running) != 1:
        raise HarnessError("provider did not expose exactly one running fabricd instance")
    row = running[0]
    iid = instance_id(row)
    created = created_at(row)
    updated = lifecycle_updated_at(row)
    digest = digest_value(row)
    if iid is None or updated is None:
        raise CapabilityError("provider does not expose instance id and lifecycle update timestamp")
    if digest != expected:
        raise HarnessError("running fabricd instance digest does not match the expected digest")
    return {"id": iid, **({"created_at": created} if created is not None else {}), "state": "running", "digest": digest, "updated_at": updated}


def lifecycle_updated_at(row: dict[str, Any]) -> str | int | float | None:
    status = row.get("status")
    value = scalar(status, "updated_at", "updatedAt") if isinstance(status, dict) else None
    value = value if value is not None else scalar(row, "updated_at", "updatedAt")
    if isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0:
        return value
    if isinstance(value, str) and (ISO_RE.fullmatch(value) or value.isdigit()):
        return value
    return None


def timestamp_key(value: str | int | float) -> float:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    if isinstance(value, str) and ISO_RE.fullmatch(value):
        return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
    raise CapabilityError("provider lifecycle update timestamp is not RFC3339 or epoch")


def inactive_lifecycle(provider: Provider, app_id: str, snapshot: InstanceSnapshot, expected: str) -> dict[str, Any] | None:
    """Return the reusable inactive lifecycle witness, if it is ready."""
    if not snapshot.complete or snapshot.total is None:
        raise CapabilityError("provider cannot reliably prove scale-zero (incomplete enumeration/count)")
    if snapshot.total == 0 and not snapshot.rows:
        raise CapabilityError("provider did not expose the required inactive singleton lifecycle")
    refs = instance_refs(snapshot)
    if snapshot.total != len(refs) or len(refs) != 1:
        raise CapabilityError("provider exposed an ambiguous or multiple pre-wake lifecycle")
    detail = provider.instance_detail(app_id, refs[0])
    state = state_of(detail)
    if state != "inactive":
        if state in {"running", "starting", "pending", "provisioning", "stopping"}:
            return None
        raise CapabilityError("provider detailed pre-wake lifecycle state is unavailable")
    iid = instance_id(detail)
    created = created_at(detail)
    updated = lifecycle_updated_at(detail)
    digest = digest_value(detail)
    if iid is None or updated is None:
        raise CapabilityError("provider inactive lifecycle lacks id or update timestamp")
    if digest is None:
        raise CapabilityError("provider inactive lifecycle digest is unavailable")
    if digest != expected:
        raise HarnessError("inactive fabricd instance digest does not match the expected digest")
    return {
        "status": "scale_zero",
        "state": "inactive",
        "observed_at": utc_now(),
        "instances": [{"id": iid, **({"created_at": created} if created is not None else {}), "state": "inactive", "digest": digest, "updated_at": updated}],
    }


def instance_refs(snapshot: InstanceSnapshot) -> list[str]:
    """Extract ids from a complete list without trusting its stale state."""
    if not snapshot.complete or snapshot.total is None:
        raise CapabilityError("provider instance enumeration is incomplete")
    refs = [instance_id(row) for row in snapshot.rows]
    if any(value is None for value in refs):
        raise CapabilityError("provider instance listing does not expose stable ids")
    return [value for value in refs if value is not None]


def provenance(
    source_repo: Path | None = None,
    declared_sha: str | None = None,
    expected_digest_value: str | None = None,
) -> dict[str, Any]:
    expected = expected_digest(expected_digest_value or EXPECTED_DIGEST)
    path = Path(__file__).with_name("provenance.json")
    try:
        with path.open(encoding="utf-8") as stream:
            value = json.load(stream)
    except (OSError, json.JSONDecodeError) as exc:
        raise CapabilityError("versioned fabricd digest/build provenance is unavailable") from exc
    if not isinstance(value, dict) or value.get("digest") != expected or value.get("build_sha") != EXPECTED_BUILD_SHA or value.get("fix_515_sha") != FIX_515_SHA:
        raise CapabilityError("fabricd digest/build provenance does not prove the #515 ancestry")
    configured_repo = source_repo or (Path(os.environ["CORELINK_SOURCE_REPO"]) if os.environ.get("CORELINK_SOURCE_REPO") else None)
    if configured_repo is None:
        raise CapabilityError("--source-repo (or CORELINK_SOURCE_REPO) is required for provenance verification")
    repo = configured_repo
    if not repo.is_dir():
        raise CapabilityError("source repository checkout is unavailable")
    status = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=str(repo), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, check=False,
    )
    if status.returncode != 0 or status.stdout:
        raise CapabilityError("source repository checkout is dirty")
    head = subprocess.run(
        ["git", "rev-parse", "--verify", "HEAD"],
        cwd=str(repo), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, check=False,
    )
    source_sha = head.stdout.strip()
    if head.returncode != 0 or not re.fullmatch(r"[0-9a-fA-F]{40,64}", source_sha):
        raise CapabilityError("source repository SHA is unavailable")
    if declared_sha is not None and (not re.fullmatch(r"[0-9a-fA-F]{40,64}", declared_sha) or declared_sha.lower() != source_sha.lower()):
        raise CapabilityError("explicit source SHA does not match the clean checkout")
    check = subprocess.run(
        ["git", "merge-base", "--is-ancestor", FIX_515_SHA, EXPECTED_BUILD_SHA],
        cwd=str(repo), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
    )
    if check.returncode != 0:
        raise CapabilityError("local provenance build SHA does not prove #515 ancestry")
    return {"source": path.name, "source_repo": str(repo), "source_sha": source_sha.lower(), "digest": expected, "build_sha": EXPECTED_BUILD_SHA, "fix_515_sha": FIX_515_SHA, "fix_515_ancestor": True}


def safe_error(exc: BaseException) -> dict[str, str]:
    if isinstance(exc, CapabilityError):
        kind = "provider_capability"
    elif isinstance(exc, HarnessError):
        kind = "assertion"
    else:
        kind = "transport"
    message = str(exc)
    # Errors from injected test providers and future adapters are not trusted
    # evidence.  Scrub known credentials and credential-shaped text before
    # the small, operator-private failure field is written.
    for name in (
        "CLOUDFLARE_CONTAINERS_API_TOKEN",
        "CLOUDFLARE_API_TOKEN",
        "CF_API_TOKEN",
        "WRANGLER_AUTH_TOKEN",
        "FABRIC_INTERNAL_AUTH",
        "SPAWN_INTERNAL_AUTH",
    ):
        secret = os.environ.get(name, "")
        if secret:
            message = message.replace(secret, "[REDACTED]")
    # Also cover credential-shaped text from exceptions that did not originate
    # in our environment. In particular, urllib can include a rendered
    # Authorization header in ValueError/OSError text.
    message = re.sub(
        r"(?i)\bauthorization\b\s*[:=]\s*(?:bearer\s+)?\S+",
        "authorization=[REDACTED]",
        message,
    )
    message = re.sub(r"(?i)\bbearer\s+\S+", "bearer=[REDACTED]", message)
    message = re.sub(r"(?i)\b(token|secret|password|credential)\b\s*[:=]\s*\S+", r"\1=[REDACTED]", message)
    return {"kind": kind, "message": message[:240]}


def wrangler_oauth_token() -> str:
    """Ask Wrangler for its current OAuth access token, refreshing if needed.

    Wrangler owns the 0600 OAuth profile and refresh-token exchange.  The
    harness captures the token in memory only long enough to construct one
    request header; stdout/stderr are never forwarded to evidence or logs.
    """
    configured = os.environ.get("WRANGLER_AUTH_TOKEN_COMMAND", "").strip()
    if configured:
        argv = shlex.split(configured)
    else:
        # --no-install prevents an unexpected package download in a long run.
        argv = ["npx", "--no-install", "wrangler", "auth", "token", "--json"]
    if not argv:
        raise HarnessError("Wrangler auth command is empty")
    executable = shutil.which(argv[0])
    if executable is None:
        raise HarnessError("Wrangler auth command is unavailable")
    argv[0] = executable
    try:
        completed = subprocess.run(
            argv,
            cwd=os.environ.get("WRANGLER_AUTH_CWD") or None,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise HarnessError("Wrangler auth command failed") from exc
    if completed.returncode != 0:
        raise HarnessError("Wrangler auth command failed")
    try:
        payload = json.loads(completed.stdout)
        token = payload.get("token") if isinstance(payload, dict) else None
    except (TypeError, json.JSONDecodeError) as exc:
        raise HarnessError("Wrangler auth command returned invalid JSON") from exc
    if not isinstance(token, str) or not token.strip():
        raise HarnessError("Wrangler auth command returned no token")
    return token.strip()


class ReadOnlyHttpProvider:
    """Cloudflare/public HTTP adapter. Every request is hard-coded to GET."""

    def __init__(
        self,
        account: str,
        fabric_url: str,
        token: str | None = None,
        api_timeout_s: float = 30.0,
        token_provider: Callable[[], str] | None = None,
        auth_source: str = "api_token",
    ):
        parsed = urlsplit(fabric_url)
        if (
            not account or (token is None and token_provider is None) or parsed.scheme != "https" or not parsed.netloc
            or parsed.username is not None or parsed.password is not None
            or parsed.path not in ("", "/") or parsed.query or parsed.fragment
        ):
            raise HarnessError("account, token, and an exact HTTPS fabric origin are required")
        self.account = account
        self.fabric_url = f"https://{parsed.netloc}"
        self.token = token
        self._token_provider = token_provider or (lambda: token or "")
        self.auth_source = auth_source
        self.api_timeout_s = api_timeout_s

    def _request_token(self) -> str:
        try:
            token = self._token_provider()
        except HarnessError:
            raise
        except Exception as exc:
            raise HarnessError("provider authentication source failed") from exc
        if not isinstance(token, str) or not token.strip() or "\r" in token or "\n" in token:
            raise HarnessError("provider authentication source returned no token")
        return token.strip()

    def _api_payload(self, path: str) -> dict[str, Any]:
        try:
            request = Request(
                f"https://api.cloudflare.com/client/v4/accounts/{self.account}{path}",
                method="GET",
                headers={"Authorization": f"Bearer {self._request_token()}", "Accept": "application/json"},
            )
            with urlopen(request, timeout=self.api_timeout_s) as response:
                payload = json.loads(response.read(2 * 1024 * 1024))
        except HTTPError as exc:
            # Keep the evidence useful without copying the response body.  In
            # particular, Cloudflare may put authentication details in that
            # body, and the body is not trusted evidence anyway.
            status = int(exc.code)
            if status in (401, 403):
                message = "provider authentication rejected control-plane read"
            elif status == 429:
                message = "provider control-plane read was rate limited"
            elif 500 <= status <= 599:
                message = "provider control-plane unavailable"
            else:
                message = f"provider control-plane read failed (HTTP {status})"
            raise HarnessError(message) from exc
        except json.JSONDecodeError as exc:
            raise HarnessError("provider control-plane returned invalid JSON") from exc
        except (URLError, TimeoutError, ValueError, OSError) as exc:
            raise HarnessError("provider control-plane transport failed") from exc
        if not isinstance(payload, dict) or payload.get("success") is False:
            raise HarnessError("provider control-plane read was unsuccessful")
        return payload

    def _api_get(self, path: str) -> Any:
        return self._api_payload(path).get("result")

    @staticmethod
    def _safe_header(headers: Any, name: str) -> str | None:
        """Keep only bounded, printable response metadata; never copy a body."""
        value = headers.get(name) if headers is not None else None
        if not isinstance(value, str):
            return None
        value = re.sub(r"[\x00-\x1f\x7f]", "", value).strip()
        return value[:256] if value else None

    def _public_get(self, path: str, timeout_s: float) -> dict[str, Any]:
        request = Request(f"{self.fabric_url}{path}", method="GET", headers={"Accept": "application/json"})
        try:
            with urlopen(request, timeout=timeout_s) as response:
                response.read(1024)  # bounded discard; bodies never enter evidence
                return {
                    "http": int(response.status),
                    "content_type": self._safe_header(response.headers, "Content-Type"),
                    "server": self._safe_header(response.headers, "Server"),
                    "cf_ray": self._safe_header(response.headers, "CF-Ray"),
                    "x_request_id": self._safe_header(response.headers, "X-Request-ID"),
                }
        except HTTPError as exc:
            return {
                "http": int(exc.code),
                "content_type": self._safe_header(exc.headers, "Content-Type"),
                "server": self._safe_header(exc.headers, "Server"),
                "cf_ray": self._safe_header(exc.headers, "CF-Ray"),
                "x_request_id": self._safe_header(exc.headers, "X-Request-ID"),
            }
        except (URLError, TimeoutError) as exc:
            raise HarnessError("public fabricd read failed") from exc

    def list_apps(self) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        for page in range(1, 101):
            # Keep the app-scoped endpoint used by the recovery fleet tooling.
            # It returns the complete application object (including image).
            result = self._api_get(f"/containers/applications?per_page=100&page={page}")
            if isinstance(result, dict):
                current = result.get("applications", result.get("items"))
                info = result.get("result_info")
                total_pages = info.get("total_pages") if isinstance(info, dict) else None
            else:
                current, total_pages = result, None
            if not isinstance(current, list) or any(not isinstance(row, dict) for row in current):
                raise CapabilityError("provider application listing has an unsupported shape")
            rows.extend(current)
            if total_pages is None or page >= total_pages:
                return rows
        raise CapabilityError("provider application pagination exceeded the finite bound")

    def list_instances(self, app_id: str) -> InstanceSnapshot:
        rows: list[dict[str, Any]] = []
        cursor = ""
        total: int | None = None
        for _ in range(100):
            suffix = f"&page_token={quote(cursor, safe='')}" if cursor else ""
            payload = self._api_payload(f"/containers/applications/{app_id}/instances?per_page=2000{suffix}")
            result = payload.get("result")
            result_info = payload.get("result_info")
            if isinstance(result, dict):
                page = result.get("instances", result.get("items", result.get("result")))
                raw_total = scalar(result, "total", "count", "instance_count", "instances_count")
                if isinstance(raw_total, int) and not isinstance(raw_total, bool) and raw_total >= 0:
                    total = raw_total if total is None else total
                next_cursor = scalar(result, "cursor", "next_cursor", "nextCursor")
            else:
                page, next_cursor = result, None
            if isinstance(result_info, dict):
                next_cursor = scalar(result_info, "next_page_token", "nextPageToken", "cursor", "next_cursor") or next_cursor
            if not isinstance(page, list) or any(not isinstance(row, dict) for row in page):
                raise CapabilityError("provider instance listing has an unsupported shape")
            rows.extend(page)
            if not next_cursor or str(next_cursor) == cursor:
                # The Containers API has no total_count. Absence of its next
                # page token is the completeness witness; derive a count from
                # the complete, de-duplicated page set.
                unique: dict[str, dict[str, Any]] = {}
                anonymous: list[dict[str, Any]] = []
                for row in rows:
                    key = instance_id(row)
                    if key is None:
                        anonymous.append(row)
                    else:
                        unique[key] = row
                rows = list(unique.values()) + anonymous
                return InstanceSnapshot(rows, total if total is not None else len(rows), True)
            cursor = str(next_cursor)
        raise CapabilityError("provider instance pagination exceeded the finite bound")

    def instance_detail(self, app_id: str, instance_id: str) -> dict[str, Any]:
        result = self._api_get(f"/containers/applications/{app_id}/instances/{instance_id}")
        row = result.get("instance", result.get("result", result)) if isinstance(result, dict) else result
        if not isinstance(row, dict):
            raise CapabilityError("provider instance detail has an unsupported shape")
        return row

    def worker_witness(self, worker_name: str, expected_id: str) -> dict[str, Any]:
        # `versions` is metadata only and has no traffic percentages.  The
        # deployment listing is the read-only source of the active 100% bind.
        result = self._api_get(f"/workers/scripts/{worker_name}/deployments?per_page=100")
        rows = result.get("deployments", result.get("items", [])) if isinstance(result, dict) else result
        if not isinstance(rows, list):
            raise CapabilityError("Worker version listing has an unsupported shape")
        deployments = [row for row in rows if isinstance(row, dict)]
        if not deployments:
            raise HarnessError("Worker has no deployment witness")
        deployments.sort(key=lambda row: str(scalar(row, "created_on", "createdAt", "created") or ""), reverse=True)
        versions = deployments[0].get("versions")
        if not isinstance(versions, list):
            raise CapabilityError("Worker deployment listing has an unsupported shape")
        active = []
        for row in versions:
            if not isinstance(row, dict):
                continue
            pct = scalar(row, "percentage", "traffic_percentage", "trafficPercentage")
            version = scalar(row, "version_id", "versionId", "id")
            if pct in (100, "100"):
                active.append({"version": version, "percentage": 100})
        if len(active) != 1 or not isinstance(active[0]["version"], str):
            raise HarnessError("Worker is not uniquely active on the requested exact version")
        if active[0]["version"] != expected_id:
            raise HarnessError("Worker is not uniquely active on the exact requested version")
        return active[0]

    def public_status(self, path: str, timeout_s: float) -> int:
        return int(self.public_witness(path, timeout_s)["http"])

    def public_witness(self, path: str, timeout_s: float) -> dict[str, Any]:
        if path not in ("/health", "/v1/attestation/key"):
            raise HarnessError("harness attempted an unapproved public route")
        return self._public_get(path, timeout_s)


@dataclass
class RunConfig:
    app_name: str = APP_NAME
    worker_name: str = WORKER_NAME
    worker_id: str | None = None
    phase: str = "candidate"
    matrix_id: str = "b2-fabricd-sleepwake"
    attempt_id_prefix: str | None = None
    app_id: str | None = None
    source_repo: str | None = None
    source_sha: str | None = None
    expected_build_sha: str = EXPECTED_BUILD_SHA
    digest: str = EXPECTED_DIGEST
    fabric_url: str | None = None
    attempts: int = ATTEMPTS
    sleep_after_s: float = SLEEP_AFTER_SECONDS
    poll_deadline_s: float = 120.0
    poll_interval_s: float = 5.0
    wake_timeout_s: float = 45.0


class SleepWakeHarness:
    def __init__(self, provider: Provider, config: RunConfig | None = None, *, clock: Callable[[], float] = time.monotonic, sleeper: Callable[[float], None] = time.sleep):
        self.provider = provider
        self.config = config or RunConfig()
        self.clock = clock
        self.sleeper = sleeper
        self.expected = expected_digest(self.config.digest)

    def preflight(self) -> dict[str, Any]:
        if self.config.sleep_after_s != SLEEP_AFTER_SECONDS:
            raise CapabilityError("sleep_after_seconds must be exactly 300")
        if not isinstance(self.config.app_id, str) or not self.config.app_id.strip():
            raise CapabilityError("explicit current FABRIC_APP_ID is required")
        source = provenance(
            Path(self.config.source_repo) if self.config.source_repo else None,
            self.config.source_sha,
            self.expected,
        )
        app = discover_app(self.provider, self.config.app_name, self.expected)
        if app["id"] != self.config.app_id:
            raise HarnessError("fabricd application id does not match the phase binding")
        if source["build_sha"] != self.config.expected_build_sha:
            raise CapabilityError("configured build SHA does not match local provenance")
        # This call validates the provider's zero witness shape before a
        # potentially long run.  A nonzero current state is acceptable; the
        # attempt's bounded idle wait will establish zero immediately before
        # its wake.
        snapshot = self.provider.list_instances(app["id"])
        if not snapshot.complete or snapshot.total is None:
            raise CapabilityError("provider cannot expose a reliable zero/new-instance witness")
        if self.config.worker_id is None:
            raise CapabilityError("every phase requires an exact --worker-id binding")
        worker = self.provider.worker_witness(self.config.worker_name, self.config.worker_id)
        if worker.get("version") != self.config.worker_id:
            raise HarnessError("Worker active version does not match the exact phase binding")
        return {
            "app_id": app["id"],
            "app_name": app["name"],
            "configured_digest": app["digest"],
            "build_sha": source["build_sha"],
            "source_sha": source["source_sha"],
            "zero_witness_shape": "complete-enumeration-with-absent-next-page-token",
            "worker": worker,
            "phase": self.config.phase,
            "matrix_id": self.config.matrix_id,
            "build_binding": {"build_sha": source["build_sha"], "fix_515_ancestor": True, "pinned": True},
            "provenance": source,
        }

    def _wait_for_zero(self, app_id: str) -> dict[str, Any]:
        deadline = self.clock() + self.config.poll_deadline_s
        while True:
            snapshot = self.provider.list_instances(app_id)
            witness = inactive_lifecycle(self.provider, app_id, snapshot, self.expected)
            if witness is not None:
                return witness
            if self.clock() >= deadline:
                # Preserve the distinction between unsupported shape and a
                # running container that never naturally slept.
                require_zero(snapshot)
                raise HarnessError("scale-zero was not observed before the bounded deadline")
            self.sleeper(min(self.config.poll_interval_s, max(0.0, deadline - self.clock())))

    def _public_witness(self, path: str) -> dict[str, Any]:
        """Capture one bounded GET witness, with compatibility for test providers."""
        method = getattr(self.provider, "public_witness", None)
        if callable(method):
            value = method(path, self.config.wake_timeout_s)
            if not isinstance(value, dict) or not isinstance(value.get("http"), int):
                raise CapabilityError("public health witness has an unsupported shape")
            return value
        status = self.provider.public_status(path, self.config.wake_timeout_s)
        if not isinstance(status, int):
            raise CapabilityError("public health status has an unsupported shape")
        return {"http": status}

    def run(self, preflight: dict[str, Any] | None = None) -> list[dict[str, Any]]:
        witness = preflight or self.preflight()
        app_id = witness["app_id"]
        records: list[dict[str, Any]] = []
        for number in range(1, self.config.attempts + 1):
            attempt_id = (
                self.config.attempt_id_prefix
                if self.config.attempt_id_prefix and self.config.attempts == 1
                else f"{self.config.attempt_id_prefix}-{number:02d}"
                if self.config.attempt_id_prefix
                else str(uuid.uuid4())
            )
            started = utc_now()
            record: dict[str, Any] = {
                "attempt": number,
                "attempt_id": attempt_id,
                "started_at": started,
                "outcome": "RED",
                "phase": self.config.phase,
                "matrix_id": self.config.matrix_id,
                **({"source_repo": self.config.source_repo} if self.config.source_repo is not None else {}),
                **({"expected_worker_version": self.config.worker_id} if self.config.worker_id is not None else {}),
            }
            try:
                # No request is made during this idle window.  The five-minute
                # interval is the provider's sleepAfter contract.
                idle_started = utc_now()
                self.sleeper(self.config.sleep_after_s)
                record["idle_window"] = {"started_at": idle_started, "seconds": self.config.sleep_after_s}
                pre_wake = self._wait_for_zero(app_id)
                record["pre_wake"] = pre_wake
                current_app = discover_app(self.provider, self.config.app_name, self.expected)
                if current_app["id"] != app_id or current_app["digest"] != witness["configured_digest"]:
                    raise HarnessError("fabricd application identity or digest changed during the probe")
                current_worker = self.provider.worker_witness(self.config.worker_name, self.config.worker_id or "")
                if current_worker != witness["worker"]:
                    raise HarnessError("Worker active version changed during the phase")
                record["deployment"] = {
                    "configured_digest": witness["configured_digest"],
                    "build_sha": witness.get("build_sha"),
                    "worker": current_worker,
                    "phase": self.config.phase,
                    "matrix_id": self.config.matrix_id,
                    "provenance": witness["provenance"],
                }

                health_at = utc_now()
                health = self._public_witness("/health")
                health_status = health["http"]
                record["wake"] = {"route": "/health", **health, "observed_at": health_at}
                if health_status != 200:
                    raise HarnessError("fabricd health wake did not return HTTP 200")

                att_at = utc_now()
                attestation = self._public_witness("/v1/attestation/key")
                att_status = attestation["http"]
                record["attestation"] = {"route": "/v1/attestation/key", **attestation, "observed_at": att_at}
                if att_status != 200:
                    raise HarnessError("fabricd attestation endpoint did not return HTTP 200")

                deadline = self.clock() + self.config.poll_deadline_s
                instance: dict[str, Any] | None = None
                while self.clock() <= deadline:
                    snapshot = self.provider.list_instances(app_id)
                    try:
                        refs = instance_refs(snapshot)
                        if not refs:
                            raise HarnessError("no instance was listed yet")
                        # A list row can be stale/inactive; detailed state is
                        # the authoritative post-wake lifecycle witness.
                        details = [self.provider.instance_detail(app_id, ref) for ref in refs]
                        authoritative = [detail for detail in details if state_of(detail) == "running"]
                        if len(authoritative) != 1:
                            raise HarnessError("provider did not expose exactly one running fabricd instance")
                        instance = running_instance(InstanceSnapshot([authoritative[0]], 1, True), self.expected)
                        break
                    except CapabilityError:
                        raise
                    except HarnessError as exc:
                        # A running instance reporting the wrong immutable
                        # image is a conclusive failure, not a transient cold
                        # state that another poll could turn green.
                        if "digest" in str(exc):
                            raise
                        if self.clock() >= deadline:
                            raise
                        self.sleeper(min(self.config.poll_interval_s, max(0.0, deadline - self.clock())))
                if instance is None:
                    raise HarnessError("running instance was not observed before the bounded deadline")
                pre_instances = pre_wake.get("instances", [])
                if pre_wake.get("state") == "inactive":
                    if len(pre_instances) != 1:
                        raise CapabilityError("inactive pre-wake lifecycle witness is ambiguous")
                    pre_instance = pre_instances[0]
                    if instance["id"] != pre_instance["id"]:
                        raise HarnessError("wake did not resume the same inactive lifecycle")
                    if instance["digest"] != pre_instance.get("digest"):
                        raise HarnessError("wake changed the inactive lifecycle digest")
                    if "created_at" in pre_instance and "created_at" in instance and str(instance["created_at"]) != str(pre_instance["created_at"]):
                        raise HarnessError("wake changed the inactive lifecycle creation identity")
                    if timestamp_key(instance["updated_at"]) <= timestamp_key(pre_instance["updated_at"]):
                        raise HarnessError("wake did not advance the inactive lifecycle update timestamp")
                    record["transition"] = {
                        "from": "inactive",
                        "to": "running",
                        "same_lifecycle": True,
                        "pre_updated_at": pre_instance["updated_at"],
                        "post_updated_at": instance["updated_at"],
                        "observed_at": utc_now(),
                    }
                else:
                    raise CapabilityError("cold wake requires one inactive singleton lifecycle")
                record["instance"] = {**instance, "independent": True, "observed_at": utc_now()}
                record["finished_at"] = utc_now()
                record["outcome"] = "PASS"
            except Exception as exc:
                record["finished_at"] = utc_now()
                record["failure"] = safe_error(exc)
                records.append(record)
                break
            records.append(record)
        return records


def write_evidence(path: Path, artifact: dict[str, Any]) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.is_symlink() or (path.exists() and path.is_dir()):
        raise HarnessError("evidence output is a directory")
    payload = (json.dumps(artifact, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")
    # No raw provider payload, request headers, or command output is ever
    # written.  The artifact itself is operator-private evidence v1.
    # Commit through a private sibling and replace atomically.  This avoids
    # following a pre-existing symlink and never leaves a partially truncated
    # evidence document after an interrupted write.
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=str(path.parent))
    temporary_path = Path(temporary)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary_path, path)
        path.chmod(0o600)
    finally:
        temporary_path.unlink(missing_ok=True)


def plan_artifact(config: RunConfig) -> dict[str, Any]:
    return {
        "schema_version": "evidence/v1",
        "artifact_id": "b2-fabricd-sleepwake-plan-" + uuid.uuid4().hex,
        "kind": "probe-plan",
        "status": "PLAN_ONLY",
        "observed_at": utc_now(),
        "contract": {
            "attempts_required": config.attempts,
            "phase": config.phase,
            "matrix_id": config.matrix_id,
            "sleep_after_seconds": config.sleep_after_s,
            "app_id": config.app_id,
            "wake_route": "/health",
            "health_path": "/health",
            "fabric_origin": config.fabric_url,
            "attestation_route": "/v1/attestation/key",
            "expected_digest": config.digest,
            "fabric_url": config.fabric_url,
            "expected_build_sha": config.expected_build_sha,
            "fix_515_sha": FIX_515_SHA,
            "mutating_operations": [],
            "preflight_required": True,
        },
        "notes": "No network request or provider mutation was performed; run --preflight or --execute --ack-execute to obtain live read-only witnesses.",
    }


def result_artifact(config: RunConfig, preflight: dict[str, Any], records: list[dict[str, Any]], failure: dict[str, str] | None = None, fabric_url: str | None = None) -> dict[str, Any]:
    # A RED envelope must expose a structured cause at the top level. Keep
    # the attempt-local cause too, but make the consumer independent of
    # whether the failure happened inside or around an attempt.
    if failure is None:
        for record in records:
            candidate = record.get("failure") if isinstance(record, dict) else None
            if isinstance(candidate, dict) and isinstance(candidate.get("kind"), str) and isinstance(candidate.get("message"), str):
                failure = {"kind": candidate["kind"], "message": candidate["message"]}
                break
    passed = len(records) == config.attempts and all(row.get("outcome") == "PASS" for row in records)
    return {
        "schema_version": "evidence/v1",
        "artifact_id": "b2-fabricd-sleepwake-" + uuid.uuid4().hex,
        "kind": "probe",
        "status": "PASS" if passed and failure is None else "RED",
        "observed_at": utc_now(),
        "contract": {
            "attempts_required": config.attempts,
            "phase": config.phase,
            "matrix_id": config.matrix_id,
            "sleep_after_seconds": config.sleep_after_s,
            "app_id": preflight.get("app_id"),
            "source_sha": preflight.get("source_sha"),
            "wake_route": "/health",
            "health_path": "/health",
            "fabric_origin": config.fabric_url if config.fabric_url is not None else fabric_url,
            "attestation_route": "/v1/attestation/key",
            "expected_digest": config.digest,
            "expected_build_sha": config.expected_build_sha,
            "fix_515_sha": FIX_515_SHA,
            "mutating_operations": [],
            "fabric_url": config.fabric_url if config.fabric_url is not None else fabric_url,
        },
        "preflight": preflight,
        "attempts": records,
        **({"failure": failure} if failure else {}),
    }


def failure_artifact(config: RunConfig, failure: dict[str, str], fabric_url: str | None = None) -> dict[str, Any]:
    """Emit a coordinator-consumable RED artifact even before preflight.

    Argument/ack/provider failures can happen before the first real attempt.
    The coordinator still needs stable correlation, timestamps, and the exact
    Worker version binding; an empty or plain-text stderr result is ambiguous.
    """
    started = utc_now()
    attempt = {
        "attempt": 1,
        "attempt_id": f"{config.attempt_id_prefix}-01" if config.attempt_id_prefix else str(uuid.uuid4()),
        "phase": config.phase,
        "matrix_id": config.matrix_id,
        "started_at": started,
        "finished_at": utc_now(),
        "outcome": "RED",
        "deployment": {"worker": {"version": config.worker_id, "percentage": 100}},
        "failure": failure,
    }
    return result_artifact(
        config,
        {"status": "UNAVAILABLE", "provenance": {"source_repo": config.source_repo}},
        [attempt],
        failure,
        fabric_url,
    )


def build_provider_from_env(fabric_url: str | None = None) -> ReadOnlyHttpProvider:
    # Match the recovery tooling's explicit precedence.  Static API tokens are
    # preferred when supplied; otherwise ask Wrangler on EVERY control-plane
    # request so its local OAuth profile can refresh between preflight, deploy,
    # and the first witness.  The token value is never printed or persisted.
    token = (
        os.environ.get("CLOUDFLARE_CONTAINERS_API_TOKEN")
        or os.environ.get("CLOUDFLARE_API_TOKEN")
        or os.environ.get("CF_API_TOKEN", "")
    )
    if token:
        return ReadOnlyHttpProvider(
            os.environ.get("CLOUDFLARE_ACCOUNT_ID") or os.environ.get("CF_ACCOUNT_ID", ""),
            fabric_url or os.environ.get("FABRIC_URL", "https://corelink-fabricd.gmhelmold.workers.dev"),
            token,
            auth_source="api_token",
        )
    return ReadOnlyHttpProvider(
        os.environ.get("CLOUDFLARE_ACCOUNT_ID") or os.environ.get("CF_ACCOUNT_ID", ""),
        fabric_url or os.environ.get("FABRIC_URL", "https://corelink-fabricd.gmhelmold.workers.dev"),
        token_provider=wrangler_oauth_token,
        auth_source="wrangler_oauth_per_request",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="finite, read-only fabricd natural sleep/wake probe")
    parser.add_argument("--output", type=Path, default=Path("evidence.json"))
    parser.add_argument("--execute", action="store_true", help="run the ten live read-only attempts")
    parser.add_argument("--ack-execute", "--ack", "--ack-destructive", action="store_true", help="acknowledge the time-bounded live probe")
    parser.add_argument("--preflight", action="store_true", help="perform read-only capability preflight without running attempts")
    parser.add_argument("--attempts", type=int, default=ATTEMPTS)
    parser.add_argument("--single-attempt", action="store_true", help="internal coordinator mode: run exactly one bounded wake")
    parser.add_argument("--sleep-after-seconds", type=float, default=SLEEP_AFTER_SECONDS)
    parser.add_argument("--poll-deadline-seconds", type=float, default=120.0)
    parser.add_argument("--poll-interval-seconds", type=float, default=5.0)
    parser.add_argument("--digest", default=os.environ.get("EXPECTED_FABRICD_DIGEST"), help="explicit immutable Fabricd image digest (required for live/preflight)")
    parser.add_argument("--phase", choices=("candidate", "rollback"), default=os.environ.get("B2_PHASE", "candidate"))
    parser.add_argument("--matrix-id", default=os.environ.get("B2_MATRIX_ID", "b2-fabricd-sleepwake"))
    parser.add_argument("--attempt-id", default=os.environ.get("B2_ATTEMPT_ID"), help="optional correlation prefix; attempts become <prefix>-01..10")
    parser.add_argument("--app-id", default=os.environ.get("FABRIC_APP_ID"))
    parser.add_argument("--source-repo", default=os.environ.get("CORELINK_SOURCE_REPO"), help="git checkout used for the local #515 ancestry check")
    parser.add_argument("--source-sha", default=os.environ.get("CORELINK_SOURCE_SHA"), help="required exact SHA of the clean source checkout for live/preflight")
    parser.add_argument("--worker-id", default=os.environ.get("EXPECTED_WORKER_VERSION"))
    parser.add_argument("--expected-build-sha", default=os.environ.get("EXPECTED_FABRICD_BUILD_SHA", EXPECTED_BUILD_SHA))
    parser.add_argument("--fabric-url", default=os.environ.get("FABRIC_URL"), help="exact HTTPS fabricd origin (required for preflight/execute)")
    args = parser.parse_args(argv)
    config = RunConfig(attempts=args.attempts, sleep_after_s=args.sleep_after_seconds, poll_deadline_s=args.poll_deadline_seconds, poll_interval_s=args.poll_interval_seconds, digest=args.digest or EXPECTED_DIGEST, fabric_url=args.fabric_url, phase=args.phase, matrix_id=args.matrix_id, attempt_id_prefix=args.attempt_id, app_id=args.app_id, source_repo=args.source_repo, source_sha=args.source_sha, worker_id=args.worker_id, expected_build_sha=args.expected_build_sha)
    try:
        if config.attempts != ATTEMPTS and not (args.single_attempt and config.attempts == 1):
            raise HarnessError("the canonical harness requires exactly 10 attempts")
        if args.execute and not args.ack_execute:
            raise HarnessError("--execute requires --ack-execute")
        if args.ack_execute and not args.execute and not args.preflight:
            raise HarnessError("an execute acknowledgement requires --execute or --preflight")
        if (args.execute or args.preflight) and not args.digest:
            raise HarnessError("live/preflight mode requires an explicit --digest (or EXPECTED_FABRICD_DIGEST)")
        if (args.execute or args.preflight) and not config.source_sha:
            raise CapabilityError("live/preflight mode requires an explicit --source-sha")
        if not args.execute and not args.preflight:
            write_evidence(args.output, plan_artifact(config))
            print(f"PLAN_ONLY: wrote {args.output}")
            return 0
        provider = build_provider_from_env(args.fabric_url)
        harness = SleepWakeHarness(provider, config)
        witness = harness.preflight()
        if args.preflight and not args.execute:
            artifact = result_artifact(config, witness, [], {"kind": "preflight_only", "message": "capabilities verified; no attempts executed"})
            artifact["status"] = "PLAN_ONLY"
            write_evidence(args.output, artifact)
            print(f"PREFLIGHT_OK: wrote {args.output}")
            return 0
        records = harness.run(witness)
        artifact = result_artifact(config, witness, records, fabric_url=args.fabric_url)
        write_evidence(args.output, artifact)
        print(f"{artifact['status']}: wrote {args.output}")
        return 0 if artifact["status"] == "PASS" else 1
    except Exception as exc:
        artifact = failure_artifact(config, safe_error(exc), fabric_url=args.fabric_url)
        write_evidence(args.output, artifact)
        print(f"RED: {safe_error(exc)['message']}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except HarnessError as exc:
        print(f"RED: {exc}", file=sys.stderr)
        raise SystemExit(1)
