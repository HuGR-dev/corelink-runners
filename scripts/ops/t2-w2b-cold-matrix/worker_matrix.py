#!/usr/bin/env python3
"""Finite, fail-closed candidate/rollback matrix for the spawn Worker.

The default mode only writes a small plan artifact.  The execute path is
explicitly gated and uses the documented local Wrangler deploy equivalent.
The harness has no container control surface: all non-Worker checks are
read-only HTTPS health probes.
"""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener, urlopen


STABLE_VERSION = "1418af47-d71a-488f-89a6-cbb9173402bd"
SPAWN_NAME = "corelink-spawn-worker"
CYCLES = 10
SLEEP_AFTER_SECONDS = 300
SCHEMA = "corelink/b2-worker-matrix/v1"
FLEET_BUSY_PATH = "/internal/v1/fleet/busy"
USER_AGENT = "corelink-b2-worker-matrix/1"
MAX_FLEET_BODY_BYTES = 1 << 20
VERSION_PROPAGATION_TIMEOUT_SECONDS = 120
VERSION_STABILITY_INTERVAL_SECONDS = 120
MAX_COLD_WITNESS_OUTPUT_BYTES = 1 << 20
CURRENT_FABRICD_DIGEST = "sha256:fda312dd86f1a3777f6f2b408af229dbe698e169b91bf2949357d10587f1f210"
SHA256_DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$", re.IGNORECASE)
DEFAULT_LOCK_FILE = Path.home() / ".corelink" / "locks" / "t2-w2b-worker-matrix.lock"


class Stop(RuntimeError):
    """A refusal that must stop the matrix without guessing success."""

    def __init__(self, message: str, *, partial_evidence: dict[str, Any] | None = None):
        super().__init__(message)
        self.partial_evidence = partial_evidence


def fail(message: str) -> None:
    raise Stop(message)


def fabricd_digest(value: str | None, *, required: bool = True) -> str | None:
    """Normalize the one digest pin shared by coordinator and companion."""
    if value is None or not value.strip():
        if required:
            fail("an explicit current Fabricd digest is required")
        return None
    normalized = value.strip().lower()
    if not SHA256_DIGEST_RE.fullmatch(normalized):
        fail("Fabricd digest must be an immutable sha256 reference")
    return normalized


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def artifact(path: Path, data: dict[str, Any]) -> None:
    """Write only structured, redacted evidence with owner-only permissions."""
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if os.path.lexists(path):
        existing = os.lstat(path)
        if stat.S_ISLNK(existing.st_mode) or not stat.S_ISREG(existing.st_mode):
            fail("evidence output must be a regular non-symlink file")
    payload = (json.dumps(data, indent=2, sort_keys=True) + "\n").encode()
    fd, temporary = tempfile.mkstemp(prefix=".worker-matrix-", dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        fd = -1
        os.replace(temporary, path)
        os.chmod(path, 0o600)
    finally:
        if fd >= 0:
            os.close(fd)
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def secure_read(path: Path, label: str) -> bytearray:
    """Read one current-UID regular 0600 file without following symlinks."""
    try:
        before = os.lstat(path)
        if stat.S_ISLNK(before.st_mode) or not stat.S_ISREG(before.st_mode):
            fail(f"{label} is not a regular file")
        if before.st_uid != os.getuid() or stat.S_IMODE(before.st_mode) != 0o600:
            fail(f"{label} must be current-UID mode 0600")
        fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            after = os.fstat(fd)
            if (after.st_dev, after.st_ino, after.st_uid, stat.S_IMODE(after.st_mode)) != (
                before.st_dev,
                before.st_ino,
                os.getuid(),
                0o600,
            ):
                fail(f"{label} changed while opened")
            data = bytearray(os.read(fd, after.st_size + 1))
        finally:
            os.close(fd)
    except OSError as exc:
        raise Stop(f"{label} cannot be opened safely") from exc
    if not data:
        fail(f"{label} is empty")
    return data


@contextlib.contextmanager
def operator_lock(path: Path):
    """Serialize the one mutation-capable matrix operator process."""
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if os.path.lexists(path):
        existing = os.lstat(path)
        if stat.S_ISLNK(existing.st_mode) or not stat.S_ISREG(existing.st_mode):
            fail("operator lock must be a regular non-symlink file")
    fd = os.open(path, os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0), 0o600)
    try:
        os.fchmod(fd, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise Stop("another matrix operator already holds the serialized lock") from exc
        yield fd
    finally:
        try:
            fcntl.flock(fd, fcntl.LOCK_UN)
        finally:
            os.close(fd)


def fleet_busy_endpoint(url: str) -> str:
    """Accept either the complete CI endpoint or its origin, exactly once."""
    endpoint = url.rstrip("/")
    return endpoint if endpoint.endswith(FLEET_BUSY_PATH) else endpoint + FLEET_BUSY_PATH


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, *_args, **_kwargs):
        raise Stop("fleet gate redirect refused")


def _fleet_open(request: Request, timeout: int):
    if not request.full_url.lower().startswith("https://"):
        fail("fleet gate requires HTTPS")
    return build_opener(_NoRedirect).open(request, timeout=timeout)


def fleet_idle(url: str, keyfile: Path) -> dict[str, int]:
    """Require an authoritative, complete idle snapshot before every deploy."""
    key = secure_read(keyfile, "fleet key")
    secret = ""
    try:
        secret = bytes(key).decode("utf-8").strip()
        if not secret:
            fail("fleet key is empty")
        endpoint = fleet_busy_endpoint(url)
        if not endpoint.lower().startswith("https://"):
            fail("fleet gate requires HTTPS")
        request = Request(
            endpoint,
            headers={
                "Accept": "application/json",
                "User-Agent": USER_AGENT,
                "X-Corelink-Internal-Auth": secret,
            },
        )
        try:
            with _fleet_open(request, timeout=30) as response:
                if response.status != 200:
                    fail("fleet gate did not return 200")
                body = response.read(MAX_FLEET_BODY_BYTES + 1)
        except HTTPError as exc:
            raise Stop(f"fleet gate returned HTTP {exc.code}") from exc
        except (URLError, TimeoutError) as exc:
            raise Stop("fleet gate is unavailable") from exc
        if len(body) > MAX_FLEET_BODY_BYTES:
            fail("fleet gate response is too large")
        try:
            value = json.loads(body)
        except json.JSONDecodeError as exc:
            raise Stop("fleet gate response was non-JSON") from exc
    finally:
        key[:] = b"\0" * len(key)
        secret = ""

    if not isinstance(value, dict):
        fail("fleet gate body shape changed")
    runners = value.get("runners")
    if not isinstance(runners, list) or not all(
        isinstance(row, dict)
        and isinstance(row.get("name"), str)
        and isinstance(row.get("repo"), str)
        for row in runners
    ):
        fail("fleet gate body shape changed")
    counts = {name: value.get(name) for name in ("busy", "checked", "unverifiable")}
    if not all(isinstance(v, int) and not isinstance(v, bool) and v >= 0 for v in counts.values()):
        fail("fleet counts malformed")
    if counts["busy"] != len(runners):
        fail("fleet busy count does not match runners")
    if counts["busy"] != 0 or counts["unverifiable"] != 0:
        fail("fleet is not provably idle")
    return counts  # type: ignore[return-value]


def _rfc3339(value: Any) -> datetime:
    if not isinstance(value, str):
        fail("deployment created_on is absent")
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise Stop("deployment created_on is not RFC3339") from exc


def latest_deployment(rows: list[dict[str, Any]]) -> dict[str, Any]:
    if not rows:
        fail("deployment list is empty")
    stamped = [(_rfc3339(row.get("created_on")), row) for row in rows]
    newest = max(stamp for stamp, _ in stamped)
    matches = [row for stamp, row in stamped if stamp == newest]
    if len(matches) != 1:
        fail("latest deployment timestamp is ambiguous")
    return matches[0]


def active_version(rows: list[dict[str, Any]]) -> tuple[str, dict[str, Any]]:
    deployment = latest_deployment(rows)
    deployment_id = deployment.get("id")
    if not isinstance(deployment_id, str) or not deployment_id:
        fail("latest deployment has no stable identity")
    versions = deployment.get("versions")
    if not isinstance(versions, list) or len(versions) != 1:
        fail("active deployment does not have exactly one version")
    item = versions[0]
    if not isinstance(item, dict):
        fail("active deployment version row is malformed")
    if item.get("percentage") != 100 or not isinstance(item.get("version_id"), str):
        fail("active deployment is not exactly one 100% version")
    return item["version_id"], deployment


def pause_bindings(version: dict[str, Any]) -> dict[str, str]:
    bindings = ((version.get("resources") or {}).get("bindings"))
    if not isinstance(bindings, list):
        fail("version bindings are absent")
    wanted = {"AUTOSCALER_INTAKE_PAUSED", "AUTOSCALER_REDRIVE_PAUSED"}
    selected = [
        binding
        for binding in bindings
        if isinstance(binding, dict) and binding.get("name") in wanted
    ]
    names = [binding.get("name") for binding in selected]
    if len(selected) != 2 or set(names) != wanted or len(set(names)) != 2:
        fail("active version does not witness both pause bindings")
    if any(binding.get("text") != "1" for binding in selected):
        fail("active version does not witness exact paused 1/1")
    return {str(binding["name"]): "1" for binding in selected}


class Wrangler:
    """Worker-only Wrangler adapter; no container control operations exist here."""

    def __init__(self, spawn_dir: Path):
        self.spawn_dir = spawn_dir

    @staticmethod
    def _json(argv: list[str], cwd: Path) -> Any:
        try:
            process = subprocess.run(
                argv,
                cwd=cwd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                timeout=90,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise Stop("Wrangler JSON command timed out") from exc
        if process.returncode != 0:
            raise Stop("Wrangler JSON command failed")
        try:
            return json.loads(process.stdout)
        except json.JSONDecodeError as exc:
            raise Stop("Wrangler JSON response was not parseable") from exc
        finally:
            process.stdout = ""

    @staticmethod
    def _mutate(argv: list[str], cwd: Path) -> int:
        """Run a permitted Worker mutation while retaining no command output."""
        try:
            return subprocess.run(
                argv,
                cwd=cwd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=600,
                check=False,
            ).returncode
        except subprocess.TimeoutExpired as exc:
            raise Stop("Wrangler mutation command timed out") from exc

    @staticmethod
    def _check(argv: list[str], cwd: Path, label: str) -> int:
        """Run one documented local gate without retaining its output."""
        try:
            result = subprocess.run(
                argv,
                cwd=cwd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=900,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise Stop(f"{label} timed out") from exc
        return result.returncode

    def preflight(self) -> dict[str, int]:
        """Run the workflow's local test gates once before the ten-cycle matrix."""
        checks = (
            ("npm_ci", ["npm", "ci"]),
            ("typecheck", ["npm", "run", "typecheck"]),
            ("test", ["npm", "run", "test"]),
        )
        result: dict[str, int] = {}
        for label, command in checks:
            code = self._check(command, self.spawn_dir, label)
            result[label] = code
            if code != 0:
                fail(f"local preflight {label} returned nonzero")
        return result

    def deployments(self) -> list[dict[str, Any]]:
        rows = self._json(
            [
                "npx",
                "--no-install",
                "wrangler",
                "deployments",
                "list",
                "--json",
                "--name",
                SPAWN_NAME,
                "--config",
                "wrangler.jsonc",
            ],
            self.spawn_dir,
        )
        if not isinstance(rows, list) or not all(isinstance(row, dict) for row in rows):
            fail("deployment list shape changed")
        return rows

    def version(self, version_id: str) -> dict[str, Any]:
        row = self._json(
            [
                "npx",
                "--no-install",
                "wrangler",
                "versions",
                "view",
                version_id,
                "--name",
                SPAWN_NAME,
                "--json",
                "--config",
                "wrangler.jsonc",
            ],
            self.spawn_dir,
        )
        if not isinstance(row, dict) or row.get("id") != version_id:
            fail("version JSON does not identify requested version")
        return row

    def candidate_deploy(self) -> int:
        # This is the local equivalent of the workflow's final deploy step. The
        # workflow's test gates run once in preflight; they are not rerun per
        # candidate cycle.
        return self._mutate(
            [
                "npx",
                "--no-install",
                "wrangler",
                "deploy",
                "--name",
                SPAWN_NAME,
                "--config",
                "wrangler.jsonc",
            ],
            self.spawn_dir,
        )

    def rollback_stable(self) -> int:
        return self._mutate(
            [
                "npx",
                "--no-install",
                "wrangler",
                "rollback",
                STABLE_VERSION,
                "--name",
                SPAWN_NAME,
                "--yes",
            ],
            self.spawn_dir,
        )


def source_sha(spawn_dir: Path, declared: str | None = None) -> str:
    """Bind every record to the source selected for the local deploy."""
    try:
        process = subprocess.run(
            ["git", "-C", str(spawn_dir), "rev-parse", "--verify", "HEAD"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise Stop("source SHA lookup timed out") from exc
    value = process.stdout.strip()
    process.stdout = ""
    if process.returncode != 0 or not re.fullmatch(r"[0-9a-fA-F]{40,64}", value):
        fail("source SHA could not be proven")
    status = subprocess.run(
        ["git", "-C", str(spawn_dir), "status", "--porcelain=v1", "--untracked-files=all"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, timeout=30, check=False,
    )
    if status.returncode != 0 or status.stdout:
        fail("source checkout is dirty")
    if declared is None:
        fail("an explicit source SHA is required")
    value = value.lower()
    if declared is not None:
        if not re.fullmatch(r"[0-9a-fA-F]{40,64}", declared) or declared.lower() != value:
            fail("declared source SHA does not match git HEAD")
    return value


def clean_checkout_sha(path: Path, label: str) -> str:
    """Prove a companion checkout is a clean, immutable worktree before mutation."""
    if not path.is_dir():
        fail(f"{label} checkout is unavailable")
    head = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "--verify", "HEAD"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, timeout=30, check=False,
    )
    value = head.stdout.strip()
    if head.returncode != 0 or not re.fullmatch(r"[0-9a-fA-F]{40,64}", value):
        fail(f"{label} checkout SHA could not be proven")
    status = subprocess.run(
        ["git", "-C", str(path), "status", "--porcelain=v1", "--untracked-files=all"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, timeout=30, check=False,
    )
    if status.returncode != 0 or status.stdout:
        fail(f"{label} checkout is dirty")
    return value.lower()


def worker_witness(
    wrangler: Wrangler,
    *,
    expected: str | None = None,
    distinct: str | None = None,
) -> dict[str, Any]:
    version_id, deployment = active_version(wrangler.deployments())
    if expected is not None and version_id != expected:
        fail("active Worker is not the exact stable version")
    if distinct is not None and version_id == distinct:
        fail("candidate Worker is not distinct from stable version")
    version = wrangler.version(version_id)
    return {
        "deployment_id": deployment["id"],
        "deployment_created_on": deployment["created_on"],
        "version_id": version_id,
        "traffic_percent": 100,
        "paused": pause_bindings(version),
    }


def monitor_active_version(
    wrangler: Wrangler,
    expected: str,
    *,
    samples: int = 2,
    interval_seconds: float | None = None,
    sleeper: Callable[[float], None] | None = None,
    clock: Callable[[], str] | None = None,
    test_override: bool = False,
) -> list[dict[str, Any]]:
    """Take temporally separated read-only samples and refuse a moving version."""
    if samples != 2:
        fail("active Worker stability monitor requires exactly two samples")
    interval = VERSION_STABILITY_INTERVAL_SECONDS if interval_seconds is None else interval_seconds
    if interval < 0:
        fail("active Worker stability interval cannot be negative")
    if interval == 0 and not test_override:
        fail("active Worker stability interval cannot be zero in live mode")
    if not test_override and interval != VERSION_STABILITY_INTERVAL_SECONDS:
        fail("active Worker stability interval override is test-only")
    sleep = sleeper or time.sleep
    timestamp = clock or now
    observations: list[dict[str, Any]] = []
    for index in range(samples):
        if index:
            sleep(interval)
        witness = worker_witness(wrangler, expected=expected)
        observations.append({**witness, "observed_at": timestamp()})
    if len({(row["deployment_id"], row["version_id"]) for row in observations}) != 1:
        fail("active Worker version changed during stability monitor")
    if lifecycle_timestamp(observations[1]["observed_at"]) <= lifecycle_timestamp(observations[0]["observed_at"]):
        fail("active Worker stability samples are not chronologically ordered")
    return observations


def guarded_rollback(wrangler: Wrangler, candidate_id: str | None) -> None:
    """Rollback only when the observed active version is ours or already stable."""
    current, _ = active_version(wrangler.deployments())
    if current == STABLE_VERSION:
        return
    if candidate_id is None or current != candidate_id:
        fail("external Worker writer detected; final rollback refused")
    if wrangler.rollback_stable() != 0:
        fail("Worker rollback returned nonzero")


def health(url: str) -> dict[str, Any]:
    """Perform a GET health read and retain only its status code."""
    if not url.lower().startswith("https://"):
        fail("health endpoint requires HTTPS")
    try:
        with urlopen(
            Request(url, headers={"Accept": "text/plain", "User-Agent": USER_AGENT}),
            timeout=30,
        ) as response:
            status = response.status
            response.read(256)
    except HTTPError as exc:
        raise Stop(f"health endpoint returned HTTP {exc.code}") from exc
    except (URLError, TimeoutError) as exc:
        raise Stop("health endpoint unavailable") from exc
    if status != 200:
        fail("health endpoint did not return 200")
    return {"status": 200}


def health_phase(url: str, phase: str, attempt_id: str) -> dict[str, Any]:
    """Record a distinct bounded GET for each matrix phase."""
    result = health(url)
    return {
        **result,
        "phase": phase,
        "attempt_id": attempt_id,
        "observed_at": now(),
        "independent_request": True,
    }


def validate_fabric_origin(value: Any) -> str:
    """Return a canonical HTTPS origin, rejecting every path and credential."""
    if not isinstance(value, str):
        fail("fabric origin is unsafe")
    parsed = urlsplit(value)
    try:
        hostname = parsed.hostname
        _ = parsed.port
    except ValueError as exc:
        raise Stop("fabric origin is unsafe") from exc
    if (
        parsed.scheme != "https"
        or not parsed.netloc
        or not hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
    ):
        fail("fabric origin is unsafe")
    # Preserve an explicitly configured port while removing the equivalent
    # root slash; the companion uses the same canonical origin.
    host = parsed.netloc
    return f"https://{host}"


def lifecycle_timestamp(value: Any) -> float:
    if isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0:
        return float(value)
    if isinstance(value, str):
        if value.isdigit():
            return float(value)
        try:
            return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
        except ValueError:
            pass
    fail("cold witness lifecycle timestamp is malformed")


def _child_contract(
    result: dict[str, Any],
    fabric_origin: str,
    source_repo: str,
    *,
    expected_digest: str,
    fabric_app_id: str | None = None,
    sleep_after_seconds: int | None = None,
    source_sha_value: str | None = None,
) -> dict[str, Any]:
    """Validate the non-secret contract before discarding a child artifact."""
    if result.get("schema_version") != "evidence/v1":
        fail("cold witness failure artifact schema is unsupported")
    contract = result.get("contract")
    if (
        not isinstance(contract, dict)
        or contract.get("fabric_url") != fabric_origin
        or contract.get("fabric_origin") != fabric_origin
        or contract.get("wake_route") != "/health"
        or contract.get("health_path") != "/health"
    ):
        fail("cold witness failure artifact contract is unsupported")
    if fabric_app_id is not None and contract.get("app_id") != fabric_app_id:
        fail("cold witness failure artifact app binding is unsupported")
    if contract.get("expected_digest") != expected_digest:
        fail("cold witness artifact digest binding is unsupported")
    if sleep_after_seconds is not None and contract.get("sleep_after_seconds") != sleep_after_seconds:
        fail("cold witness failure artifact sleep window is unsupported")
    preflight = result.get("preflight")
    provenance = preflight.get("provenance") if isinstance(preflight, dict) else None
    if isinstance(provenance, dict):
        if provenance.get("source_repo") != source_repo:
            fail("cold witness failure artifact provenance is unsupported")
        if source_sha_value is not None and provenance.get("source_sha") != source_sha_value:
            fail("cold witness failure artifact source SHA is unsupported")
    else:
        fail("cold witness failure artifact provenance is unsupported")
    return contract


def _partial_child_evidence(result: dict[str, Any], request: dict[str, Any], expected_digest: str, source_repo: str) -> dict[str, Any]:
    """Keep only bounded, structured fields from a failed child phase."""
    fabric_origin = validate_fabric_origin(request.get("fabric_origin"))
    contract = _child_contract(
        result,
        fabric_origin,
        source_repo,
        expected_digest=expected_digest,
        fabric_app_id=request.get("fabric_app_id"),
        sleep_after_seconds=request.get("sleep_after_seconds"),
        source_sha_value=request.get("source_sha"),
    )
    if contract.get("phase") != request.get("phase") or contract.get("matrix_id") != request.get("matrix_run_id") or contract.get("attempts_required") != CYCLES:
        fail("cold witness contract correlation does not match request")
    if result.get("status") not in ("RED", "PASS"):
        fail("cold witness failure artifact status is unsupported")
    attempts = result.get("attempts")
    if not isinstance(attempts, list) or len(attempts) > CYCLES:
        fail("cold witness failure artifact attempts are unsupported")
    sanitized: list[dict[str, Any]] = []
    for index, attempt in enumerate(attempts, start=1):
        if not isinstance(attempt, dict):
            fail("cold witness failure artifact attempt is unsupported")
        number = attempt.get("attempt")
        if number != index:
            fail("cold witness failure artifact attempt numbering changed")
        if attempt.get("phase") != request.get("phase") or attempt.get("matrix_id") != request.get("matrix_run_id"):
            fail("cold witness failure artifact correlation changed")
        attempt_id = attempt.get("attempt_id")
        if not isinstance(attempt_id, str) or not attempt_id:
            fail("cold witness failure artifact attempt identity is absent")
        deployment = attempt.get("deployment")
        worker = deployment.get("worker") if isinstance(deployment, dict) else None
        worker_version = worker.get("version") if isinstance(worker, dict) else None
        if worker_version is not None and not isinstance(worker_version, str):
            fail("cold witness failure artifact Worker version is malformed")
        instance = attempt.get("instance")
        instance_safe: dict[str, Any] | None = None
        if isinstance(instance, dict) and isinstance(instance.get("id"), str) and instance.get("id"):
            instance_safe = {
                "id": instance["id"],
                "state": instance.get("state"),
                "digest": instance.get("digest") if instance.get("digest") == expected_digest else None,
                "updated_at": instance.get("updated_at"),
            }
            if isinstance(instance.get("created_at"), (str, int, float)) and not isinstance(instance.get("created_at"), bool):
                instance_safe["created_at"] = instance["created_at"]
        wake = attempt.get("wake")
        wake_safe = None
        if isinstance(wake, dict) and wake.get("route") == "/health" and isinstance(wake.get("http"), int) and not isinstance(wake.get("http"), bool):
            wake_safe = {"route": "/health", "http": wake["http"], "observed_at": wake.get("observed_at")}
        pre_wake = attempt.get("pre_wake")
        pre_wake_safe = None
        if isinstance(pre_wake, dict):
            pre_instances = pre_wake.get("instances")
            pre_safe = []
            if isinstance(pre_instances, list):
                for row in pre_instances[:1]:
                    if isinstance(row, dict):
                        pre_safe.append({"id": row.get("id"), "created_at": row.get("created_at"), "state": row.get("state"), "digest": row.get("digest"), "updated_at": row.get("updated_at")})
            pre_wake_safe = {"status": pre_wake.get("status"), "state": pre_wake.get("state"), "observed_at": pre_wake.get("observed_at"), "instances": pre_safe}
        sanitized.append({
            "attempt": number,
            "attempt_id": attempt_id,
            "phase": request["phase"],
            "matrix_id": request["matrix_run_id"],
            "outcome": attempt.get("outcome") if attempt.get("outcome") in ("PASS", "RED") else "RED",
            "started_at": attempt.get("started_at"),
            "finished_at": attempt.get("finished_at"),
            "expected_version_id": request["expected_version_id"],
            "worker_version": worker_version,
            "pre_wake": pre_wake_safe,
            "health": wake_safe,
            "instance": instance_safe,
        })
    # The companion places attempt-specific failures inside the RED attempt;
    # only preflight/outer failures are top-level. Preserve the first
    # structured cause without retaining the provider payload or logs.
    child_failure = result.get("failure")
    if not isinstance(child_failure, dict):
        for failed_attempt in attempts:
            if isinstance(failed_attempt, dict) and isinstance(failed_attempt.get("failure"), dict):
                child_failure = failed_attempt["failure"]
                break
    failure_safe = None
    if isinstance(child_failure, dict):
        kind = child_failure.get("kind")
        message = child_failure.get("message")
        if isinstance(kind, str) and isinstance(message, str):
            failure_safe = {"kind": kind[:80], "message": message[:240]}
    if failure_safe is None:
        fail("cold witness failure artifact has no structured failure")
    return {
        "status": result.get("status"),
        "phase": request["phase"],
        "matrix_run_id": request["matrix_run_id"],
        "expected_version_id": request["expected_version_id"],
        "source_repo": source_repo,
        "contract": {"fabric_origin": fabric_origin, "wake_route": "/health", "health_path": "/health"},
        "attempts": sanitized,
        "failure": failure_safe,
    }


def cold_witness(command: list[str] | None, request: dict[str, Any], source_repo: str) -> dict[str, Any]:
    """Invoke one ten-attempt sleep/wake phase through a private JSON artifact."""
    if not command:
        fail("cold witness command is required for execution")
    if any(not isinstance(part, str) or not part for part in command):
        fail("cold witness command is malformed")
    expected_digest = request.get("expected_digest")
    expected_digest = fabricd_digest(expected_digest)
    fabric_origin = validate_fabric_origin(request.get("fabric_origin"))
    temporary_fd, temporary_name = tempfile.mkstemp(prefix=".b2-cold-witness-", suffix=".json")
    os.close(temporary_fd)
    temporary_path = Path(temporary_name)
    temporary_path.unlink()
    child_args = command + [
        "--output", str(temporary_path),
        "--execute", "--ack-execute", "--attempts", str(CYCLES),
        "--phase", str(request["phase"]),
        "--matrix-id", str(request["matrix_run_id"]),
        "--attempt-id", str(request["matrix_run_id"] + "-" + str(request["phase"])),
        "--worker-id", str(request["expected_version_id"]),
        "--source-repo", source_repo,
        "--fabric-url", fabric_origin,
        "--digest", expected_digest,
    ]
    if "fabric_app_id" in request:
        child_args.extend(["--app-id", str(request["fabric_app_id"])])
    if "source_sha" in request:
        child_args.extend(["--source-sha", str(request["source_sha"])])
    if "sleep_after_seconds" in request:
        child_args.extend(["--sleep-after-seconds", str(request["sleep_after_seconds"])])
    try:
        process = subprocess.run(
            child_args,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=6600,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        temporary_path.unlink(missing_ok=True)
        raise Stop("cold witness command timed out") from exc
    try:
        if not temporary_path.is_file() or temporary_path.is_symlink():
            fail("cold witness artifact is absent or unsafe")
        if temporary_path.stat().st_size > MAX_COLD_WITNESS_OUTPUT_BYTES:
            fail("cold witness response is too large")
        raw = secure_read(temporary_path, "cold witness artifact")
        try:
            result = json.loads(bytes(raw))
        finally:
            raw[:] = b"\0" * len(raw)
    except (OSError, json.JSONDecodeError) as exc:
        raise Stop("cold witness response was unavailable or non-JSON") from exc
    finally:
        temporary_path.unlink(missing_ok=True)
    if not isinstance(result, dict):
        fail("cold witness response was not an object")
    if process.returncode != 0 or result.get("status") != "PASS":
        try:
            partial = _partial_child_evidence(result, request, expected_digest, source_repo)
        except Stop as exc:
            raise Stop(f"cold witness failure artifact invalid: {exc}") from exc
        raise Stop("cold witness command returned nonzero or RED", partial_evidence=partial)
    attempts = result.get("attempts")
    if not isinstance(attempts, list) or len(attempts) != CYCLES:
        fail("cold witness did not provide exactly ten attempts")
    provenance = result.get("preflight", {}).get("provenance") if isinstance(result.get("preflight"), dict) else None
    if not isinstance(provenance, dict) or provenance.get("source_repo") != source_repo:
        fail("cold witness source repository does not match request")
    contract = _child_contract(
        result,
        fabric_origin,
        source_repo,
        expected_digest=expected_digest,
        fabric_app_id=request.get("fabric_app_id"),
        sleep_after_seconds=request.get("sleep_after_seconds"),
        source_sha_value=request.get("source_sha"),
    )
    if contract.get("phase") != request.get("phase") or contract.get("matrix_id") != request.get("matrix_run_id") or contract.get("attempts_required") != CYCLES:
        fail("cold witness contract correlation does not match request")
    seen_attempt_ids: set[str] = set()
    seen_time_pairs: set[tuple[str, str]] = set()
    sanitized: list[dict[str, Any]] = []
    for index, attempt in enumerate(attempts, start=1):
        if not isinstance(attempt, dict) or attempt.get("outcome") != "PASS":
            fail("cold witness attempt did not prove PASS")
        if attempt.get("attempt") != index or attempt.get("phase") != request.get("phase") or attempt.get("matrix_id") != request.get("matrix_run_id"):
            fail("cold witness attempt correlation does not match request")
        attempt_id = attempt.get("attempt_id")
        started_at = attempt.get("started_at")
        finished_at = attempt.get("finished_at")
        if not isinstance(attempt_id, str) or not attempt_id or attempt_id in seen_attempt_ids:
            fail("cold witness reused an attempt identity")
        if not isinstance(started_at, str) or not isinstance(finished_at, str) or lifecycle_timestamp(finished_at) <= lifecycle_timestamp(started_at):
            fail("cold witness attempt timestamps are not an increasing pair")
        if (started_at, finished_at) in seen_time_pairs:
            fail("cold witness reused an attempt timestamp pair")
        seen_attempt_ids.add(attempt_id)
        seen_time_pairs.add((started_at, finished_at))
        deployment = attempt.get("deployment")
        worker = deployment.get("worker") if isinstance(deployment, dict) else None
        if not isinstance(worker, dict) or worker.get("version") != request.get("expected_version_id") or worker.get("percentage") != 100:
            fail("cold witness deployment is not bound to exact 100% Worker version")
        health_result = attempt.get("wake")
        if not isinstance(health_result, dict) or health_result.get("route") != "/health" or health_result.get("http") != 200:
            fail("cold witness has no HTTP 200 health proof")
        pre_wake = attempt.get("pre_wake")
        if not isinstance(pre_wake, dict) or pre_wake.get("status") != "scale_zero" or pre_wake.get("state") != "inactive":
            fail("cold witness has no complete scale-zero proof")
        pre_instances = pre_wake.get("instances")
        if not isinstance(pre_instances, list) or len(pre_instances) != 1 or not isinstance(pre_instances[0], dict):
            fail("cold witness has no singleton inactive lifecycle proof")
        pre_instance = pre_instances[0]
        if pre_instance.get("state") != "inactive" or pre_instance.get("digest") != expected_digest or not isinstance(pre_instance.get("id"), str) or not pre_instance.get("id"):
            fail("cold witness inactive lifecycle identity is malformed")
        pre_updated = pre_instance.get("updated_at")
        if pre_updated is None:
            fail("cold witness inactive lifecycle update timestamp is absent")
        instance = attempt.get("instance")
        if not isinstance(instance, dict):
            fail("cold witness instance proof is absent")
        if not isinstance(instance.get("id"), str) or not instance["id"]:
            fail("cold witness instance identity is absent")
        if instance.get("state") != "running" or instance.get("digest") != expected_digest or instance["id"] != pre_instance["id"]:
            fail("cold witness instance is not running on the expected digest")
        post_updated = instance.get("updated_at")
        if post_updated is None or lifecycle_timestamp(post_updated) <= lifecycle_timestamp(pre_updated):
            fail("cold witness lifecycle update timestamp did not advance")
        sanitized.append({
            "attempt": index,
            "attempt_id": attempt_id,
            "started_at": started_at,
            "finished_at": finished_at,
            "pre_wake": {"status": "scale_zero", "state": "inactive", "observed_at": pre_wake.get("observed_at"), "instances": [{"id": pre_instance["id"], "created_at": pre_instance.get("created_at"), "state": "inactive", "digest": expected_digest, "updated_at": pre_updated}]},
            "health": {"status": 200, "route": "/health", "observed_at": health_result.get("observed_at"), "independent_request": True},
            "transition": {"from": "inactive", "to": "running", "same_lifecycle": True, "pre_updated_at": pre_updated, "post_updated_at": post_updated},
            "instance": {"id": instance["id"], **({"created_at": instance["created_at"]} if "created_at" in instance else {}), "state": "running", "digest": expected_digest, "updated_at": post_updated, "independent": True},
        })
    return {
        "status": "PASS",
        "phase": request["phase"],
        "matrix_run_id": request["matrix_run_id"],
        "expected_version_id": request["expected_version_id"],
        "contract": {"fabric_origin": fabric_origin, "wake_route": "/health", "health_path": "/health"},
        "attempts": sanitized,
    }


def companion_descriptor(command: list[str] | None) -> dict[str, Any]:
    """Describe the companion without retaining arbitrary argument secrets."""
    if not command or any(
        len(part) > 256 or "=" in part or not re.fullmatch(r"[A-Za-z0-9_./:+@-]+", part)
        for part in command
    ):
        fail("cold witness command contains unsafe arguments")
    return {
        "protocol": "sleepwake-evidence/v1",
        "argv": command,
        "argv_sha256": hashlib.sha256("\0".join(command).encode()).hexdigest(),
        "timeout_seconds": 6600,
        "attempts_per_phase": CYCLES,
    }


def poll(predicate: Callable[[], Any], timeout: int, message: str) -> Any:
    """Bound retries around eventual consistency without making the run open-ended."""
    deadline = time.monotonic() + timeout
    last_error: Stop | None = None
    while True:
        try:
            value = predicate()
            if value is not None:
                return value
        except Stop as exc:
            last_error = exc
        if time.monotonic() >= deadline:
            if last_error is not None:
                raise Stop(f"{message}: {last_error}")
            fail(message)
        time.sleep(3)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--spawn-dir", type=Path, required=True)
    parser.add_argument("--fleet-url", required=True)
    parser.add_argument(
        "--fabric-origin",
        dest="fabric_origin",
        required=True,
        help="exact HTTPS origin for the companion read-only /health probe (no path)",
    )
    parser.add_argument("--fleet-key-file", type=Path)
    parser.add_argument("--source-sha", help="required immutable git SHA for the candidate source")
    parser.add_argument("--fabric-app-id", default=os.environ.get("FABRIC_APP_ID"), help="required current fabricd application ID")
    parser.add_argument(
        "--fabricd-digest",
        "--expected-fabricd-digest",
        dest="fabricd_digest",
        default=os.environ.get("EXPECTED_FABRICD_DIGEST"),
        help="immutable Fabricd image digest shared with the companion",
    )
    parser.add_argument("--lock-file", type=Path, default=Path(os.environ.get("B2_WORKER_MATRIX_LOCK_FILE", str(DEFAULT_LOCK_FILE))))
    parser.add_argument("--matrix-run-id", help="stable non-secret ID used to join companion evidence")
    parser.add_argument("--source-repo", required=False, help="checkout passed to the companion sleep/wake harness")
    parser.add_argument(
        "--cold-witness-command",
        nargs="+",
        help="argv for the JSON sleep/wake companion protocol (no shell is used)",
    )
    parser.add_argument("--execute", action="store_true")
    parser.add_argument(
        "--ack-destructive",
        "--ack-execute",
        "--ack",
        dest="ack_destructive",
        action="store_true",
        help="acknowledge the bounded live matrix",
    )
    args = parser.parse_args(argv)
    if args.execute and not args.fabricd_digest:
        fail("live mode requires an explicit --fabricd-digest (or EXPECTED_FABRICD_DIGEST)")
    expected_fabricd_digest = fabricd_digest(args.fabricd_digest or CURRENT_FABRICD_DIGEST)
    matrix_run_id = args.matrix_run_id or ("b2-worker-" + uuid.uuid4().hex)
    if not re.fullmatch(r"[A-Za-z0-9._:-]{1,128}", matrix_run_id):
        fail("matrix run ID is malformed")

    plan = {
        "schema": SCHEMA,
        "schema_version": "evidence/v1",
        "artifact_id": "b2-worker-matrix-plan-" + uuid.uuid4().hex,
        "matrix_run_id": matrix_run_id,
        "kind": "probe-plan",
        "observed_at": now(),
        "cycles": CYCLES,
        "stable_version": STABLE_VERSION,
        "contract": {
            "cycles_required": CYCLES,
            "sleep_after_seconds": SLEEP_AFTER_SECONDS,
            "stability_interval_seconds": VERSION_STABILITY_INTERVAL_SECONDS,
            "fabric_app_id": args.fabric_app_id,
            "expected_fabricd_digest": expected_fabricd_digest,
            "rollback_target_version_id": STABLE_VERSION,
            "mutating_operations": ["spawn_worker_deploy", "spawn_worker_rollback"],
            "raw_logs_retained": False,
        },
        "mutated": False,
        "raw_logs_retained": False,
        "status": "PLAN_ONLY",
    }
    if not args.execute:
        artifact(args.output, plan)
        return 0
    if not args.ack_destructive or args.fleet_key_file is None:
        fail("live mode requires --ack-destructive and --fleet-key-file")
    if not args.source_sha:
        fail("live mode requires an explicit --source-sha")
    if not isinstance(args.fabric_app_id, str) or not re.fullmatch(r"[A-Za-z0-9-]{16,128}", args.fabric_app_id):
        fail("live mode requires the explicit current FABRIC_APP_ID")
    # Validate and canonicalize the companion origin before any preflight or
    # mutation state is entered. The companion itself only appends /health.
    args.fabric_origin = validate_fabric_origin(args.fabric_origin)
    if not args.cold_witness_command:
        fail("live mode requires --cold-witness-command")
    companion = companion_descriptor(args.cold_witness_command)
    if not args.source_repo or not Path(args.source_repo).is_dir():
        fail("live mode requires an existing --source-repo")
    if not (args.spawn_dir / "wrangler.jsonc").is_file():
        fail("spawn Worker Wrangler configuration is required")

    wrangler = Wrangler(args.spawn_dir)
    candidate_ids: set[str] = set()
    deployment_ids: set[str] = set()
    records: list[dict[str, Any]] = []
    candidate_attempts: list[dict[str, Any]] = []
    rollback_attempts: list[dict[str, Any]] = []
    preflight_checks: dict[str, int] = {}
    child_failures: list[dict[str, Any]] = []
    failure: str | None = None
    mutation_started = False
    stable_proven = False
    worker_restore: dict[str, Any] = {"status": "UNATTEMPTED"}
    version_monitor: list[dict[str, Any]] = []
    external_writer_detected = False
    lock_context = operator_lock(args.lock_file)
    lock_context.__enter__()

    try:
        # Read-only preflight. A failure here must not trigger a rollback.
        selected_source_sha = source_sha(args.spawn_dir, args.source_sha)
        companion_source_sha = clean_checkout_sha(Path(args.source_repo), "companion source")
        preflight_checks = wrangler.preflight()
        prior = worker_witness(wrangler, expected=STABLE_VERSION)
        candidate_fleet = fleet_idle(args.fleet_url, args.fleet_key_file)
        # Keep the second stability sample immediately adjacent to the
        # candidate mutation. No fleet/API operation is allowed after it.
        version_monitor = monitor_active_version(wrangler, STABLE_VERSION)
        mutation_started = True
        if wrangler.candidate_deploy() != 0:
            fail("candidate Worker deploy returned nonzero")
        candidate = poll(
            lambda: worker_witness(wrangler, distinct=STABLE_VERSION),
            VERSION_PROPAGATION_TIMEOUT_SECONDS,
            "candidate Worker witness timed out",
        )
        candidate_id = candidate["version_id"]
        candidate_deployment_id = candidate["deployment_id"]
        candidate_ids.add(candidate_id)
        deployment_ids.add(candidate_deployment_id)
        # The candidate is held active while one companion invocation performs
        # ten independent cold witnesses.
        candidate_phase = cold_witness(
            args.cold_witness_command,
            {"matrix_run_id": matrix_run_id, "phase": "candidate", "expected_version_id": candidate_id, "fabric_origin": args.fabric_origin, "fabric_app_id": args.fabric_app_id, "expected_digest": expected_fabricd_digest, "sleep_after_seconds": SLEEP_AFTER_SECONDS, "source_sha": companion_source_sha},
            args.source_repo,
        )
        for attempt in candidate_phase["attempts"]:
            candidate_attempts.append({"cycle": attempt["attempt"], "matrix_run_id": matrix_run_id, "attempt_id": attempt["attempt_id"], "started_at": attempt["started_at"], "source_sha": selected_source_sha, "expected_version_id": candidate_id, "witness": attempt})

        rollback_fleet = fleet_idle(args.fleet_url, args.fleet_key_file)
        guarded_rollback(wrangler, candidate_id)
        rollback = poll(
            lambda: worker_witness(wrangler, expected=STABLE_VERSION),
            VERSION_PROPAGATION_TIMEOUT_SECONDS,
            "stable Worker rollback witness timed out",
        )
        stable_proven = True
        # The stable target is held active while one companion invocation
        # performs ten distinct cold witnesses.
        rollback_phase = cold_witness(
            args.cold_witness_command,
            {"matrix_run_id": matrix_run_id, "phase": "rollback", "expected_version_id": STABLE_VERSION, "fabric_origin": args.fabric_origin, "fabric_app_id": args.fabric_app_id, "expected_digest": expected_fabricd_digest, "sleep_after_seconds": SLEEP_AFTER_SECONDS, "source_sha": companion_source_sha},
            args.source_repo,
        )
        for attempt in rollback_phase["attempts"]:
            rollback_attempts.append({"cycle": attempt["attempt"], "matrix_run_id": matrix_run_id, "attempt_id": attempt["attempt_id"], "started_at": attempt["started_at"], "source_sha": selected_source_sha, "expected_version_id": STABLE_VERSION, "witness": attempt})
        if len(rollback_attempts) != len(candidate_attempts):
            fail("candidate and rollback cold witness counts differ")
        # Pair the two ten-attempt phase artifacts by ordinal cycle.
        for candidate_attempt, rollback_attempt in zip(candidate_attempts, rollback_attempts):
            records.append(
                {
                    "cycle": candidate_attempt["cycle"],
                    "matrix_run_id": matrix_run_id,
                    "attempt_id": candidate_attempt["attempt_id"],
                    "rollback_attempt_id": rollback_attempt["attempt_id"],
                    "started_at": candidate_attempt["started_at"],
                    "observed_at": now(),
                    "source_sha": selected_source_sha,
                    "rollback_target_version_id": STABLE_VERSION,
                    "prior": prior,
                    "fleet": candidate_fleet,
                    "rollback_fleet": rollback_fleet,
                    "candidate": candidate,
                    "candidate_cold_witness": candidate_attempt["witness"],
                    "candidate_health": candidate_attempt["witness"]["health"],
                    "rollback": rollback,
                    "rollback_cold_witness": rollback_attempt["witness"],
                    "rollback_health": rollback_attempt["witness"]["health"],
                }
            )
    except Stop as exc:
        failure = str(exc)
        if exc.partial_evidence is not None:
            partial = dict(exc.partial_evidence)
            if "selected_source_sha" in locals():
                partial["source_sha"] = selected_source_sha
            child_failures.append(partial)
    finally:
        # Once a mutation began, always make one bounded attempt to restore the
        # stable Worker. Read-only preflight failures remain untouched.
        if mutation_started:
            try:
                if not stable_proven:
                    current, _ = active_version(wrangler.deployments())
                    if current != STABLE_VERSION:
                        if "candidate_id" not in locals() or current != candidate_id:
                            external_writer_detected = True
                            fail("external Worker writer detected; final rollback refused")
                        guarded_rollback(wrangler, candidate_id)
                final = poll(
                    lambda: worker_witness(wrangler, expected=STABLE_VERSION),
                    VERSION_PROPAGATION_TIMEOUT_SECONDS,
                    "final stable Worker witness timed out",
                )
                worker_restore = {"status": "PASS", "witness": final}
            except Stop as exc:
                worker_restore = {"status": "RED", "reason": str(exc)}
        else:
            worker_restore = {"status": "NOT_REQUIRED_PREMUTATION"}
        lock_context.__exit__(None, None, None)

    passed = (
        failure is None
        and len(records) == CYCLES
        and len(candidate_ids) == 1
        and len(deployment_ids) == 1
        and len(candidate_attempts) == CYCLES
        and len(rollback_attempts) == CYCLES
        and worker_restore.get("status") == "PASS"
    )
    evidence: dict[str, Any] = {
        "schema": SCHEMA,
        "schema_version": "evidence/v1",
        "artifact_id": "b2-worker-matrix-" + uuid.uuid4().hex,
        "matrix_run_id": matrix_run_id,
        "kind": "probe",
        "observed_at": now(),
        "cycles_required": CYCLES,
        "cycles_completed": len(records),
        "candidate_version_ids": sorted(candidate_ids),
        "candidate_version_id_cardinality": len(candidate_ids),
        "candidate_deployment_ids": sorted(deployment_ids),
        "candidate_deployment_id_cardinality": len(deployment_ids),
        "candidate_cold_witnesses_required": CYCLES,
        "candidate_cold_witnesses_recorded": len(candidate_attempts),
        "rollback_cold_witnesses_required": CYCLES,
        "rollback_cold_witnesses_recorded": len(rollback_attempts),
        "candidate_attempts": candidate_attempts,
        "rollback_attempts": rollback_attempts,
        "cold_witness_failures": child_failures,
        "companion": companion,
        "records": records,
        "preflight": preflight_checks,
        "source_sha": locals().get("selected_source_sha"),
        "companion_source_sha": locals().get("companion_source_sha"),
        "stable_version": STABLE_VERSION,
        "contract": {
            "cycles_required": CYCLES,
            "rollback_target_version_id": STABLE_VERSION,
            "stability_interval_seconds": VERSION_STABILITY_INTERVAL_SECONDS,
            "health_method": "GET",
            "fabric_origin": args.fabric_origin,
            "fabric_app_id": args.fabric_app_id,
            "expected_fabricd_digest": expected_fabricd_digest,
            "sleep_after_seconds": SLEEP_AFTER_SECONDS,
            "health_path": "/health",
            "wake_route": "/health",
            "mutating_operations": ["spawn_worker_deploy", "spawn_worker_rollback"],
        },
        "mutation_started": mutation_started,
        "mutated": mutation_started,
        "final_worker_restore": worker_restore,
        "version_stability_monitor": version_monitor,
        "external_writer_detected": external_writer_detected,
        "raw_logs_retained": False,
        "status": "PASS" if passed else "RED",
    }
    if failure is not None:
        evidence["failure"] = failure
    artifact(args.output, evidence)
    return 0 if passed else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Stop as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        raise SystemExit(2)
