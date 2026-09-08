from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError, URLError

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from harness import (
    CapabilityError,
    EXPECTED_DIGEST,
    failure_artifact,
    result_artifact,
    HarnessError,
    InstanceSnapshot,
    ReadOnlyHttpProvider,
    RunConfig,
    SleepWakeHarness,
    discover_app,
    require_zero,
    state_of,
    write_evidence,
    safe_error,
)

SOURCE_REPO = str(Path(__file__).resolve().parents[4])
HARNESS_SCRIPT = Path(__file__).resolve().parents[1] / "harness.py"
os.environ.setdefault("CORELINK_SOURCE_REPO", SOURCE_REPO)


class FakeProvider:
    def __init__(self, snapshots: list[InstanceSnapshot], *, app_digest: str = EXPECTED_DIGEST):
        self.snapshots = iter(snapshots)
        self.app_digest = app_digest
        self.routes: list[str] = []
        self.detail_rows = {}

    def list_apps(self):
        # Cloudflare's application object does not expose a build SHA. Build
        # provenance is verified locally from provenance.json + git.
        return [{"id": "app-1", "name": "corelink-fabricd-fabricdcontainer", "image": self.app_digest}]

    def list_instances(self, app_id: str):
        try:
            snapshot = next(self.snapshots)
            for row in snapshot.rows:
                if "id" in row:
                    self.detail_rows.setdefault(row["id"], row)
            return snapshot
        except StopIteration:
            # A deterministic fake should never silently invent a successful
            # provider response once its finite fixture has been consumed.
            raise AssertionError("fixture exhausted")

    def instance_detail(self, app_id: str, instance_id: str):
        return self.detail_rows[instance_id]

    def worker_witness(self, worker_name: str, expected_id: str):
        version = getattr(self, "worker_version", "1418-version")
        return {"version": version, "percentage": 100}

    def public_status(self, path: str, timeout_s: float):
        self.routes.append(path)
        return 200


class LifecycleProvider(FakeProvider):
    def list_instances(self, app_id: str):
        try:
            snapshot = next(self.snapshots)
        except StopIteration:
            raise AssertionError("fixture exhausted")
        for row in snapshot.rows:
            if "id" in row:
                self.detail_rows[row["id"]] = row
        return snapshot


class SequencedDetailProvider(LifecycleProvider):
    def __init__(self, snapshots, details):
        super().__init__(snapshots)
        self.details = iter(details)

    def instance_detail(self, app_id: str, instance_id: str):
        return next(self.details)


def zero() -> InstanceSnapshot:
    return InstanceSnapshot([], 0, True)


def running(number: int, digest: str = EXPECTED_DIGEST) -> InstanceSnapshot:
    return InstanceSnapshot(
        [{"id": f"instance-{number}", "created_at": f"2026-09-08T00:00:{number:02d}Z", "state": "running", "status": {"state": "running", "updated_at": f"2026-09-08T00:00:{number:02d}Z"}, "image": digest}],
        1,
        True,
    )


def inactive(number: int, digest: str = EXPECTED_DIGEST) -> InstanceSnapshot:
    return InstanceSnapshot([{"id": f"instance-{number}", "created_at": f"2026-09-08T00:00:{number:02d}Z", "state": "inactive", "status": {"state": "inactive", "updated_at": "2026-09-08T00:00:00Z"}, "image": digest}], 1, True)


class ApiFixtureProvider(ReadOnlyHttpProvider):
    """Capture adapter paths while returning sanitized raw API fixtures."""

    def __init__(self, payloads):
        super().__init__("account", "https://fabric.example", "token")
        self.payloads = iter(payloads)
        self.paths = []

    def _api_payload(self, path):
        self.paths.append(path)
        return next(self.payloads)


class HarnessTests(unittest.TestCase):
    def config(self, **overrides):
        values = {"sleep_after_s": 300, "poll_deadline_s": 1, "poll_interval_s": 0.001, "worker_id": "1418-version", "app_id": "app-1"}
        values.update(overrides)
        return RunConfig(**values)

    def test_ten_independent_natural_wakes_and_single_health_each(self):
        snapshots = [running(0)]
        for number in range(1, 11):
            snapshots.append(InstanceSnapshot([{"id": "reusable-instance", "state": "inactive", "status": {"state": "inactive", "updated_at": f"2026-09-08T00:00:{number:02d}Z"}, "image": EXPECTED_DIGEST}], 1, True))
            snapshots.append(InstanceSnapshot([{"id": "reusable-instance", "state": "running", "status": {"state": "running", "updated_at": f"2026-09-08T00:01:{number:02d}Z"}, "image": EXPECTED_DIGEST}], 1, True))
        provider = LifecycleProvider(snapshots)
        harness = self.harness_for(provider, self.config())
        witness = harness.preflight()
        records = harness.run(witness)
        self.assertEqual(len(records), 10)
        self.assertTrue(all(row["outcome"] == "PASS" for row in records))
        self.assertEqual(provider.routes, [route for _ in range(10) for route in ("/health", "/v1/attestation/key")])
        self.assertEqual(len({row["instance"]["id"] for row in records}), 1)
        self.assertTrue(all(row["pre_wake"]["status"] == "scale_zero" for row in records))

    def test_stale_list_state_is_checked_against_authoritative_detail(self):
        # The provider list may lag during a wake. The detail endpoint remains
        # authoritative for running state and digest.
        stale = InstanceSnapshot([{"id": "instance-1", "created_at": "2026-09-08T00:00:01Z", "state": "starting", "image": EXPECTED_DIGEST}], 1, True)
        provider = SequencedDetailProvider([running(0), inactive(1), stale], [inactive(1).rows[0], {"id": "instance-1", "created_at": "2026-09-08T00:00:01Z", "state": "running", "status": {"state": "running", "updated_at": "2026-09-08T00:00:02Z"}, "image": EXPECTED_DIGEST}])
        harness = self.harness_for(provider, self.config())
        records = harness.run(harness.preflight())
        self.assertEqual(records[0]["outcome"], "PASS")

    def harness_for(self, provider, config):
        now = [0.0]

        def clock():
            return now[0]

        def sleep(seconds):
            now[0] += seconds

        return SleepWakeHarness(provider, config, clock=clock, sleeper=sleep)

    def test_preflight_requires_exact_five_minute_sleep_window(self):
        provider = FakeProvider([running(0)])
        harness = self.harness_for(provider, self.config(sleep_after_s=299))
        with self.assertRaisesRegex(CapabilityError, "exactly 300"):
            harness.preflight()

    def test_preflight_requires_explicit_current_fabric_app_id(self):
        provider = FakeProvider([running(0)])
        harness = self.harness_for(provider, self.config(app_id=None))
        with self.assertRaisesRegex(CapabilityError, "FABRIC_APP_ID"):
            harness.preflight()

    def test_preflight_binds_app_and_source_sha_in_evidence(self):
        provider = FakeProvider([running(0)])
        config = self.config()
        witness = self.harness_for(provider, config).preflight()
        self.assertEqual(witness["app_id"], "app-1")
        self.assertRegex(witness["source_sha"], r"^[0-9a-f]{40,64}$")
        artifact = result_artifact(config, witness, [])
        self.assertEqual(artifact["contract"]["app_id"], "app-1")
        self.assertEqual(artifact["contract"]["sleep_after_seconds"], 300)
        self.assertEqual(artifact["contract"]["source_sha"], witness["source_sha"])

    def test_reused_inactive_lifecycle_is_allowed(self):
        provider = LifecycleProvider([running(0), inactive(1), running(1), inactive(1), running(1)])
        harness = self.harness_for(provider, self.config(attempts=2))
        records = harness.run(harness.preflight())
        self.assertEqual([row["outcome"] for row in records], ["PASS", "PASS"])

    def test_provider_without_explicit_zero_count_fails_preflight(self):
        provider = FakeProvider([InstanceSnapshot([], None, True)])
        harness = SleepWakeHarness(provider, self.config())
        with self.assertRaises(CapabilityError):
            harness.preflight()

    def test_candidate_binds_exact_worker_id(self):
        provider = FakeProvider([running(0)])
        provider.worker_version = "candidate-version-20260908"
        harness = self.harness_for(provider, self.config(phase="candidate", worker_id="candidate-version-20260908"))
        self.assertEqual(harness.preflight()["worker"]["version"], "candidate-version-20260908")

    def test_candidate_rejects_missing_exact_worker_id(self):
        provider = FakeProvider([running(0)])
        harness = self.harness_for(provider, self.config(phase="candidate", worker_id=None))
        with self.assertRaises(CapabilityError):
            harness.preflight()

    def test_rollback_rejects_missing_exact_worker_id(self):
        provider = FakeProvider([running(0)])
        harness = self.harness_for(provider, self.config(phase="rollback", worker_id=None))
        with self.assertRaises(CapabilityError):
            harness.preflight()

    def test_rollback_rejects_worker_abbreviation_or_mismatch(self):
        provider = FakeProvider([running(0)])
        harness = self.harness_for(provider, self.config(phase="rollback", worker_id="1418"))
        with self.assertRaises(HarnessError):
            harness.preflight()

    def test_source_repo_is_mandatory_for_preflight(self):
        previous = os.environ.pop("CORELINK_SOURCE_REPO", None)
        try:
            provider = FakeProvider([running(0)])
            harness = SleepWakeHarness(provider, self.config(source_repo=None))
            with self.assertRaises(CapabilityError):
                harness.preflight()
        finally:
            if previous is not None:
                os.environ["CORELINK_SOURCE_REPO"] = previous

    def test_fabric_url_requires_exact_https_origin(self):
        for value in ("http://fabric.example", "https://fabric.example/v1", "https://u:p@fabric.example", "https://fabric.example?x=1"):
            with self.assertRaises(HarnessError):
                ReadOnlyHttpProvider("account", value, "token")

    def test_fabric_url_is_the_artifact_contract_binding(self):
        from harness import plan_artifact

        artifact = plan_artifact(self.config(fabric_url="https://fabric.example"))
        self.assertEqual(artifact["contract"]["fabric_origin"], "https://fabric.example")
        self.assertEqual(artifact["contract"]["health_path"], "/health")

    def test_nonempty_zero_snapshot_fails_closed(self):
        with self.assertRaises(Exception):
            require_zero(InstanceSnapshot([{"id": "still-here"}], 1, True))

    def test_postwake_digest_mismatch_is_red(self):
        provider = LifecycleProvider([running(0), inactive(1), running(1, "sha256:" + "0" * 64)])
        harness = self.harness_for(provider, self.config())
        records = harness.run(harness.preflight())
        self.assertEqual(records[0]["outcome"], "RED")
        self.assertIn("digest", records[0]["failure"]["message"])

    def test_evidence_is_private_and_does_not_contain_token(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.json"
            write_evidence(path, {"schema_version": "evidence/v1", "status": "PASS", "note": "no token"})
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            self.assertNotIn("CF_API_TOKEN", path.read_text())

    def test_cli_default_is_plan_only_and_never_requires_network(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "plan.json"
            env = os.environ.copy()
            env.pop("CF_API_TOKEN", None)
            result = subprocess.run([sys.executable, str(HARNESS_SCRIPT), "--output", str(path)], env=env, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(path.read_text())["status"], "PLAN_ONLY")

    def test_execute_requires_ack(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, str(HARNESS_SCRIPT), "--execute", "--output", str(Path(directory) / "ack-test.json")], capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires --ack-execute", result.stderr)

    def test_cloudflare_container_paths_and_cursor_shape(self):
        provider = ApiFixtureProvider([
            {"success": True, "result": [{"id": "app-1", "name": "corelink-fabricd-fabricdcontainer", "image": EXPECTED_DIGEST}], "result_info": {}},
            {"success": True, "result": {"instances": [{"id": "i-1", "created_at": "2026-09-08T00:00:01Z", "current_placement": {"status": {"container_status": "running"}}, "image": EXPECTED_DIGEST}]}, "result_info": {"per_page": 2000, "next_page_token": "next/page"}},
            {"success": True, "result": {"instances": [{"id": "i-2", "created_at": "2026-09-08T00:00:02Z", "current_placement": {"status": {"container_status": "running"}}, "image": EXPECTED_DIGEST}]}, "result_info": {"per_page": 2000}},
        ])
        apps = provider.list_apps()
        snapshot = provider.list_instances(apps[0]["id"])
        self.assertEqual(snapshot.total, 2)
        self.assertTrue(snapshot.complete)
        self.assertEqual(provider.paths, [
            "/containers/applications?per_page=100&page=1",
            "/containers/applications/app-1/instances?per_page=2000",
            "/containers/applications/app-1/instances?per_page=2000&page_token=next%2Fpage",
        ])
        self.assertEqual([state_of(row) for row in snapshot.rows], ["running", "running"])

    def test_worker_witness_reads_latest_deployment_percentage(self):
        provider = ApiFixtureProvider([{
            "success": True,
            "result": {"deployments": [
                {"created_on": "2026-09-08T01:00:00Z", "versions": [{"version_id": "old", "percentage": 100}]},
                {"created_on": "2026-09-08T02:00:00Z", "versions": [{"version_id": "new", "percentage": 100}]},
            ]},
            "result_info": {},
        }])
        self.assertEqual(provider.worker_witness("corelink-spawn-worker", "new"), {"version": "new", "percentage": 100})
        self.assertEqual(provider.paths, ["/workers/scripts/corelink-spawn-worker/deployments?per_page=100"])

    def test_provider_accepts_recovery_token_and_account_names(self):
        with patch.dict(os.environ, {
            "CLOUDFLARE_CONTAINERS_API_TOKEN": "container-token",
            "CLOUDFLARE_API_TOKEN": "plain-token",
            "CLOUDFLARE_ACCOUNT_ID": "container-account",
            "CF_API_TOKEN": "legacy-token",
            "CF_ACCOUNT_ID": "legacy-account",
        }, clear=False):
            from harness import build_provider_from_env
            provider = build_provider_from_env("https://fabric.example")
        self.assertEqual(provider.token, "container-token")
        self.assertEqual(provider.account, "container-account")
        self.assertEqual(provider.auth_source, "api_token")

    def test_provider_refreshes_token_for_each_control_plane_request(self):
        tokens = iter(("short-lived-1", "short-lived-2"))
        provider = ReadOnlyHttpProvider(
            "account",
            "https://fabric.example",
            token_provider=lambda: next(tokens),
            auth_source="wrangler_oauth_per_request",
        )
        responses = []

        class Response:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def read(self, _limit):
                return b'{"success":true,"result":{}}'

        def capture(request, timeout):
            responses.append(request.headers["Authorization"])
            return Response()

        with patch("harness.urlopen", side_effect=capture):
            provider._api_payload("/one")
            provider._api_payload("/two")
        self.assertEqual(responses, ["Bearer short-lived-1", "Bearer short-lived-2"])
        self.assertEqual(provider.auth_source, "wrangler_oauth_per_request")

    def test_wrangler_oauth_command_returns_token_without_logging_command_output(self):
        from harness import wrangler_oauth_token

        completed = subprocess.CompletedProcess(
            ["wrangler", "auth", "token", "--json"],
            0,
            '{"type":"oauth","token":"oauth-access-value"}\n',
            "",
        )
        with patch.dict(os.environ, {"WRANGLER_AUTH_TOKEN_COMMAND": "wrangler auth token --json"}, clear=False), \
             patch("harness.shutil.which", return_value="/bin/wrangler"), \
             patch("harness.subprocess.run", return_value=completed) as run:
            self.assertEqual(wrangler_oauth_token(), "oauth-access-value")
        run.assert_called_once()
        self.assertEqual(run.call_args.kwargs["stderr"], subprocess.DEVNULL)
        self.assertEqual(run.call_args.kwargs["stdin"], subprocess.DEVNULL)

    def test_provider_falls_back_to_per_request_wrangler_auth(self):
        from harness import build_provider_from_env

        with patch.dict(
            os.environ,
            {
                "CLOUDFLARE_ACCOUNT_ID": "account",
                "CLOUDFLARE_CONTAINERS_API_TOKEN": "",
                "CLOUDFLARE_API_TOKEN": "",
                "CF_API_TOKEN": "",
            },
            clear=False,
        ), patch("harness.wrangler_oauth_token", return_value="oauth-access") as token:
            provider = build_provider_from_env("https://fabric.example")
            self.assertIsNone(provider.token)
            self.assertEqual(provider.auth_source, "wrangler_oauth_per_request")
            self.assertEqual(provider._request_token(), "oauth-access")
            token.assert_called_once_with()

    def test_wrangler_auth_failure_is_redacted_and_never_exposes_output(self):
        from harness import wrangler_oauth_token

        completed = subprocess.CompletedProcess(
            ["wrangler", "auth", "token", "--json"],
            1,
            "oauth-access-value",
            "secret refresh detail",
        )
        with patch.dict(os.environ, {"WRANGLER_AUTH_TOKEN_COMMAND": "wrangler auth token --json"}, clear=False), \
             patch("harness.shutil.which", return_value="/bin/wrangler"), \
             patch("harness.subprocess.run", return_value=completed):
            with self.assertRaisesRegex(HarnessError, "Wrangler auth command failed") as raised:
                wrangler_oauth_token()
            self.assertNotIn("oauth-access-value", str(raised.exception))
            self.assertNotIn("secret refresh detail", str(raised.exception))

    def test_provider_errors_are_classified_without_response_body(self):
        provider = ReadOnlyHttpProvider("account", "https://fabric.example", "token")
        for status, expected in ((401, "authentication rejected"), (429, "rate limited"), (503, "unavailable"), (400, "HTTP 400")):
            with self.subTest(status=status), patch("harness.urlopen", side_effect=HTTPError("https://api.example", status, "secret body", {}, None)):
                with self.assertRaisesRegex(HarnessError, expected) as raised:
                    provider._api_payload("/containers/applications")
                self.assertNotIn("secret body", str(raised.exception))

        with patch("harness.urlopen", side_effect=URLError("secret transport detail")):
            with self.assertRaisesRegex(HarnessError, "transport failed") as raised:
                provider._api_payload("/containers/applications")
                self.assertNotIn("secret transport detail", str(raised.exception))

    def test_bearer_value_error_is_redacted_from_structured_failure(self):
        sentinel = "review-secret-static-token"
        provider = ReadOnlyHttpProvider("account", "https://fabric.example", sentinel)
        with patch.dict(os.environ, {
            "CLOUDFLARE_CONTAINERS_API_TOKEN": sentinel,
            "CLOUDFLARE_API_TOKEN": sentinel,
            "CF_API_TOKEN": sentinel,
        }, clear=False), patch("harness.urlopen", side_effect=ValueError(f"Invalid header value b'Bearer {sentinel}'")):
            with self.assertRaises(HarnessError) as raised:
                provider._api_payload("/containers/applications")
        failure = safe_error(raised.exception)
        self.assertNotIn(sentinel, json.dumps(failure))

    def test_control_plane_token_with_newline_is_rejected(self):
        provider = ReadOnlyHttpProvider("account", "https://fabric.example", "token\nleak")
        with self.assertRaises(HarnessError):
            provider._api_payload("/containers/applications")

    def test_urlopen_value_error_cannot_escape_with_bearer(self):
        sentinel = "sentinel-api-token-transport"
        provider = ReadOnlyHttpProvider("account", "https://fabric.example", sentinel)
        with patch("harness.urlopen", side_effect=ValueError(f"invalid header Authorization: Bearer {sentinel}")):
            with self.assertRaisesRegex(HarnessError, "transport failed") as raised:
                provider._api_payload("/containers/applications")
        self.assertNotIn(sentinel, str(raised.exception))

    def test_all_supported_token_aliases_are_absent_from_failure_artifact(self):
        sentinels = {
            "CLOUDFLARE_CONTAINERS_API_TOKEN": "containers-sentinel",
            "CLOUDFLARE_API_TOKEN": "api-sentinel",
            "CF_API_TOKEN": "legacy-sentinel",
            "WRANGLER_AUTH_TOKEN": "wrangler-sentinel",
        }
        with patch.dict(os.environ, sentinels, clear=False):
            failure = safe_error(ValueError("Authorization: Bearer containers-sentinel; token=api-sentinel; secret=legacy-sentinel; credential=wrangler-sentinel"))
        config = self.config(source_repo=None)
        artifact = json.dumps(failure_artifact(config, failure), sort_keys=True)
        for sentinel in sentinels.values():
            self.assertNotIn(sentinel, artifact)

    def test_cli_failure_has_no_token_in_artifact_stdout_or_stderr(self):
        from harness import main

        sentinel = "cli-bearer-sentinel"
        with tempfile.TemporaryDirectory() as directory:
            output_path = Path(directory) / "evidence.json"
            stdout = io.StringIO()
            stderr = io.StringIO()
            with patch("harness.provenance", return_value={"build_sha": "ok"}), \
                 patch("harness.build_provider_from_env", side_effect=ValueError(f"Authorization: Bearer {sentinel}")), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                result = main(["--preflight", "--output", str(output_path), "--source-repo", directory, "--worker-id", "worker"])
            self.assertEqual(result, 1)
            self.assertNotIn(sentinel, output_path.read_text())
            self.assertNotIn(sentinel, stdout.getvalue())
            self.assertNotIn(sentinel, stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
