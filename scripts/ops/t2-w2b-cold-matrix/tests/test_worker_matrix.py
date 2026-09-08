import importlib.util
import io
import json
import stat
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from urllib.error import HTTPError


MODULE_PATH = Path(__file__).resolve().parents[1] / "worker_matrix.py"
HARNESS_DIR = MODULE_PATH.parent
SOURCE_REPO = str(HARNESS_DIR.parents[2])
SPEC = importlib.util.spec_from_file_location("worker_matrix", MODULE_PATH)
assert SPEC and SPEC.loader
matrix = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(matrix)
SLEEPWAKE_PATH = HARNESS_DIR / "harness.py"
SLEEP_SPEC = importlib.util.spec_from_file_location("sleepwake_harness", SLEEPWAKE_PATH)
assert SLEEP_SPEC and SLEEP_SPEC.loader
sleepwake = importlib.util.module_from_spec(SLEEP_SPEC)
sys.modules[SLEEP_SPEC.name] = sleepwake
SLEEP_SPEC.loader.exec_module(sleepwake)


class WorkerMatrixTests(unittest.TestCase):
    def setUp(self):
        self._real_monitor = matrix.monitor_active_version
        matrix.monitor_active_version = lambda wrangler, expected: self._real_monitor(
            wrangler,
            expected,
            interval_seconds=0,
            test_override=True,
            sleeper=lambda _seconds: None,
        )

    def tearDown(self):
        matrix.monitor_active_version = self._real_monitor

    def test_stability_monitor_records_two_ordered_timestamps_and_default_is_120(self):
        class Stable:
            def deployments(self):
                return [{"id": "deployment-stable", "created_on": "2026-09-08T00:00:00Z", "versions": [{"version_id": matrix.STABLE_VERSION, "percentage": 100}]}]

            def version(self, version_id):
                return {"id": version_id, "resources": {"bindings": [
                    {"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"},
                    {"name": "AUTOSCALER_REDRIVE_PAUSED", "text": "1"},
                ]}}

        sleeps = []
        stamps = iter(("2026-09-08T00:00:00Z", "2026-09-08T00:02:00Z"))
        samples = self._real_monitor(Stable(), matrix.STABLE_VERSION, sleeper=sleeps.append, clock=lambda: next(stamps))
        self.assertEqual(sleeps, [120])
        self.assertEqual([row["observed_at"] for row in samples], ["2026-09-08T00:00:00Z", "2026-09-08T00:02:00Z"])

    def test_zero_stability_interval_is_rejected_without_test_override(self):
        with self.assertRaisesRegex(matrix.Stop, "cannot be zero"):
            self._real_monitor(object(), matrix.STABLE_VERSION, interval_seconds=0)

    def test_operator_lock_serializes_and_refuses_second_holder(self):
        with tempfile.TemporaryDirectory() as td:
            lock_path = Path(td) / "matrix.lock"
            with matrix.operator_lock(lock_path):
                with self.assertRaisesRegex(matrix.Stop, "serialized lock"):
                    with matrix.operator_lock(lock_path):
                        pass

    def test_final_rollback_refuses_external_active_version(self):
        class ExternalWriter:
            rollback_calls = 0

            def deployments(self):
                return [{"id": "external-deployment", "created_on": "2026-09-08T00:00:00Z", "versions": [{"version_id": "external-version", "percentage": 100}]}]

            def rollback_stable(self):
                type(self).rollback_calls += 1
                return 0

        writer = ExternalWriter()
        with self.assertRaisesRegex(matrix.Stop, "external Worker writer"):
            matrix.guarded_rollback(writer, "our-candidate-version")
        self.assertEqual(ExternalWriter.rollback_calls, 0)

    def test_source_sha_requires_clean_checkout_and_explicit_declaration(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.email=test@example.invalid", "-c", "user.name=test", "commit", "--allow-empty", "-qm", "init"], check=True)
            head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
            self.assertEqual(matrix.source_sha(root, head), head)
            (root / "dirty.txt").write_text("dirty")
            with self.assertRaisesRegex(matrix.Stop, "dirty"):
                matrix.source_sha(root, head)

    def test_original_live_red_fixture_reproduces_unstructured_failure(self):
        fixture = Path(__file__).with_name("fixtures") / "worker-evidence-red-original.json"
        value = json.loads(fixture.read_text())
        self.assertEqual(value["schema_version"], "evidence/v1")
        self.assertEqual(value["status"], "RED")
        self.assertIsInstance(value["failure"], str)
        self.assertEqual(value["records"], [])

    def test_real_child_cli_failure_is_structured_for_consumer(self):
        source_repo = SOURCE_REPO
        request = {
            "matrix_run_id": "run-child-red",
            "phase": "candidate",
            "expected_version_id": "candidate-version",
            "fabric_origin": "https://fabric.example",
        }
        child = SLEEPWAKE_PATH
        with tempfile.TemporaryDirectory() as td:
            child_output = Path(td) / "child-red.json"
            child_process = subprocess.run([
                sys.executable, str(child), "--output", str(child_output), "--execute", "--ack-execute",
                "--attempts", "10", "--phase", "candidate", "--matrix-id", "run-child-red",
                "--attempt-id", "run-child-red-candidate", "--worker-id", "candidate-version",
                "--source-repo", source_repo, "--fabric-url", "https://fabric.example",
            ], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            self.assertNotEqual(child_process.returncode, 0)
            child_artifact = json.loads(child_output.read_text())
            self.assertEqual(child_artifact["schema_version"], "evidence/v1")
            self.assertEqual(child_artifact["status"], "RED")
            self.assertIsInstance(child_artifact["failure"], dict)
            old_run = matrix.subprocess.run
            try:
                def fake_run(argv, **kwargs):
                    output_path = Path(argv[argv.index("--output") + 1])
                    output_path.write_text(json.dumps(child_artifact))
                    output_path.chmod(0o600)
                    return types.SimpleNamespace(returncode=child_process.returncode)

                matrix.subprocess.run = fake_run
                with self.assertRaisesRegex(matrix.Stop, "returned nonzero or RED") as raised:
                    matrix.cold_witness([sys.executable, str(child)], request, source_repo)
            finally:
                matrix.subprocess.run = old_run
        partial = raised.exception.partial_evidence
        self.assertIsNotNone(partial)
        self.assertEqual(partial["status"], "RED")
        self.assertEqual(partial["failure"]["kind"], "provider_capability")
        self.assertEqual(len(partial["attempts"]), 1)
        self.assertIsInstance(partial["attempts"][0]["started_at"], str)
        self.assertEqual(partial["attempts"][0]["worker_version"], "candidate-version")

    def test_plan_default_is_finite_and_owner_only(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            output = root / "plan.json"
            rc = matrix.main(
                [
                    "--output",
                    str(output),
                    "--spawn-dir",
                    str(root),
                    "--fleet-url",
                    "https://fleet.invalid/internal/v1/fleet/busy",
                    "--fabric-origin",
                    "https://health.invalid/health",
                ]
            )
            self.assertEqual(rc, 0)
            plan = json.loads(output.read_text())
            self.assertEqual(plan["schema"], "corelink/b2-worker-matrix/v1")
            self.assertEqual(plan["cycles"], 10)
            self.assertEqual(plan["status"], "PLAN_ONLY")
            self.assertFalse(plan["mutated"])
            self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)

    def test_artifact_refuses_existing_symlink(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            target = root / "target"
            target.write_text("keep")
            output = root / "evidence.json"
            output.symlink_to(target)
            with self.assertRaisesRegex(matrix.Stop, "non-symlink"):
                matrix.artifact(output, {"status": "PLAN_ONLY"})
            self.assertEqual(target.read_text(), "keep")

    def test_fleet_gate_refuses_plain_http_before_sending_key(self):
        with tempfile.TemporaryDirectory() as td:
            key = Path(td) / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            old = matrix._fleet_open
            try:
                matrix._fleet_open = lambda *_args, **_kwargs: self.fail("network must not be attempted")
                with self.assertRaisesRegex(matrix.Stop, "requires HTTPS"):
                    matrix.fleet_idle("http://fleet.example", key)
            finally:
                matrix._fleet_open = old

    def test_companion_artifact_contract_validates_ten_wakes_and_argv(self):
        source_repo = SOURCE_REPO
        request = {
            "matrix_run_id": "run-1",
            "phase": "candidate",
            "expected_version_id": "candidate-version",
            "fabric_origin": "https://fabric.example",
            "fabric_app_id": matrix.FABRIC_APP_ID,
            "sleep_after_seconds": 300,
            "source_sha": "41293ef2457b0a728fcc25faad16bd0e1506ab62",
        }
        attempts = []
        for number in range(1, 11):
            attempts.append({
                "attempt": number,
                "attempt_id": f"candidate-{number:02d}",
                "started_at": f"2026-09-08T00:00:{number:02d}Z",
                "finished_at": f"2026-09-08T00:01:{number:02d}Z",
                "phase": "candidate",
                "matrix_id": "run-1",
                "outcome": "PASS",
                "pre_wake": {"status": "scale_zero", "state": "inactive", "observed_at": f"2026-09-08T00:02:{number:02d}Z", "instances": [{"id": "singleton-instance", "created_at": "2026-09-08T00:01:00Z", "state": "inactive", "digest": matrix.EXPECTED_FABRICD_DIGEST, "updated_at": f"2026-09-08T00:02:{number:02d}Z"}]},
                "wake": {"route": "/health", "http": 200, "observed_at": "2026-09-08T00:01:00Z"},
                "deployment": {"worker": {"version": "candidate-version", "percentage": 100}},
                "instance": {"id": "singleton-instance", "created_at": "2026-09-08T00:01:00Z", "state": "running", "digest": matrix.EXPECTED_FABRICD_DIGEST, "updated_at": f"2026-09-08T00:03:{number:02d}Z"},
            })
        response = {"schema_version": "evidence/v1", "status": "PASS", "contract": {"fabric_url": "https://fabric.example", "fabric_origin": "https://fabric.example", "wake_route": "/health", "health_path": "/health", "phase": "candidate", "matrix_id": "run-1", "attempts_required": 10, "app_id": matrix.FABRIC_APP_ID, "sleep_after_seconds": 300}, "preflight": {"provenance": {"source_repo": source_repo, "source_sha": request["source_sha"]}}, "attempts": attempts}
        seen = {}
        old_run = matrix.subprocess.run
        try:
            def fake_run(argv, **kwargs):
                seen["argv"] = argv
                output_path = Path(argv[argv.index("--output") + 1])
                output_path.write_text(json.dumps(response))
                output_path.chmod(0o600)
                return types.SimpleNamespace(returncode=0)

            matrix.subprocess.run = fake_run
            result = matrix.cold_witness(["python3", "sleepwake.py"], request, source_repo)
        finally:
            matrix.subprocess.run = old_run
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(len(result["attempts"]), 10)
        self.assertIn("--phase", seen["argv"])
        self.assertIn("--fabric-url", seen["argv"])
        self.assertIn("--attempts", seen["argv"])
        self.assertIn("--app-id", seen["argv"])
        self.assertIn("--source-sha", seen["argv"])
        self.assertIn("--sleep-after-seconds", seen["argv"])
        self.assertNotIn("--single-attempt", seen["argv"])

    def test_companion_artifact_rejects_duplicate_attempt_identity(self):
        source_repo = SOURCE_REPO
        request = {"matrix_run_id": "run-1", "phase": "rollback", "expected_version_id": matrix.STABLE_VERSION, "fabric_origin": "https://fabric.example"}
        attempts = []
        for number in range(1, 11):
            attempts.append({"attempt": number, "attempt_id": "rollback-01", "outcome": "PASS", "phase": "rollback", "matrix_id": "run-1", "started_at": f"2026-09-08T00:00:{number:02d}Z", "finished_at": f"2026-09-08T00:01:{number:02d}Z", "pre_wake": {"status": "scale_zero", "state": "inactive", "instances": [{"id": "same", "created_at": "2026-09-08T00:01:00Z", "state": "inactive", "digest": matrix.EXPECTED_FABRICD_DIGEST, "updated_at": f"2026-09-08T00:02:{number:02d}Z"}]}, "wake": {"route": "/health", "http": 200}, "deployment": {"worker": {"version": matrix.STABLE_VERSION, "percentage": 100}}, "instance": {"id": "same", "created_at": "2026-09-08T00:01:00Z", "state": "running", "digest": matrix.EXPECTED_FABRICD_DIGEST, "updated_at": f"2026-09-08T00:03:{number:02d}Z"}})
        response = {"schema_version": "evidence/v1", "status": "PASS", "contract": {"fabric_url": "https://fabric.example", "fabric_origin": "https://fabric.example", "wake_route": "/health", "health_path": "/health", "phase": "rollback", "matrix_id": "run-1", "attempts_required": 10}, "preflight": {"provenance": {"source_repo": source_repo}}, "attempts": attempts}
        old_run = matrix.subprocess.run
        try:
            def fake_run(argv, **kwargs):
                output_path = Path(argv[argv.index("--output") + 1])
                output_path.write_text(json.dumps(response))
                output_path.chmod(0o600)
                return types.SimpleNamespace(returncode=0)

            matrix.subprocess.run = fake_run
            with self.assertRaisesRegex(matrix.Stop, "reused"):
                matrix.cold_witness(["python3", "sleepwake.py"], request, source_repo)
        finally:
            matrix.subprocess.run = old_run

    def test_failed_companion_preserves_sanitized_partial_evidence(self):
        source_repo = SOURCE_REPO
        request = {"matrix_run_id": "run-red", "phase": "candidate", "expected_version_id": "candidate-version", "fabric_origin": "https://fabric.example"}
        response = {
            "schema_version": "evidence/v1",
            "status": "RED",
            "contract": {"fabric_url": "https://fabric.example", "fabric_origin": "https://fabric.example", "wake_route": "/health", "health_path": "/health", "phase": "candidate", "matrix_id": "run-red", "attempts_required": 10},
            "preflight": {"provenance": {"source_repo": source_repo}},
            "attempts": [{
                "attempt": 1,
                "attempt_id": "candidate-01",
                "phase": "candidate",
                "matrix_id": "run-red",
                "outcome": "RED",
                "started_at": "2026-09-08T00:00:00Z",
                "finished_at": "2026-09-08T00:01:00Z",
                "deployment": {"worker": {"version": "candidate-version", "percentage": 100}},
            }],
            "failure": {"kind": "harness", "message": "bounded witness failed"},
        }
        old_run = matrix.subprocess.run
        try:
            def fake_run(argv, **kwargs):
                output_path = Path(argv[argv.index("--output") + 1])
                output_path.write_text(json.dumps(response))
                output_path.chmod(0o600)
                return types.SimpleNamespace(returncode=1)

            matrix.subprocess.run = fake_run
            with self.assertRaisesRegex(matrix.Stop, "returned nonzero") as raised:
                matrix.cold_witness(["python3", "sleepwake.py"], request, source_repo)
        finally:
            matrix.subprocess.run = old_run
        partial = raised.exception.partial_evidence
        self.assertEqual(partial["status"], "RED")
        self.assertEqual(partial["attempts"][0]["worker_version"], "candidate-version")
        self.assertEqual(partial["attempts"][0]["started_at"], "2026-09-08T00:00:00Z")
        self.assertEqual(partial["failure"]["kind"], "harness")

    def test_real_sleepwake_red_artifact_flows_to_consumer_on_nonzero(self):
        """Exercise producer result_artifact -> worker consumer, including nested failure."""
        source_repo = SOURCE_REPO
        request = {"matrix_run_id": "run-producer-red", "phase": "candidate", "expected_version_id": "candidate-version", "fabric_origin": "https://fabric.example"}
        config = sleepwake.RunConfig(
            attempts=10,
            phase="candidate",
            matrix_id="run-producer-red",
            attempt_id_prefix="run-producer-red-candidate",
            source_repo=source_repo,
            worker_id="candidate-version",
            fabric_url="https://fabric.example",
        )
        produced = sleepwake.result_artifact(
            config,
            {"provenance": {"source_repo": source_repo}},
            [{
                "attempt": 1,
                "attempt_id": "run-producer-red-candidate-01",
                "started_at": "2026-09-08T00:00:00Z",
                "finished_at": "2026-09-08T00:01:00Z",
                "phase": "candidate",
                "matrix_id": "run-producer-red",
                "outcome": "RED",
                "expected_worker_version": "candidate-version",
                "failure": {"kind": "assertion", "message": "fabricd health wake did not return HTTP 200"},
            }],
            fabric_url="https://fabric.example",
        )
        self.assertEqual(produced["schema_version"], "evidence/v1")
        self.assertEqual(produced["status"], "RED")
        self.assertEqual(produced["failure"], {"kind": "assertion", "message": "fabricd health wake did not return HTTP 200"})
        old_run = matrix.subprocess.run
        try:
            def fake_run(argv, **kwargs):
                output_path = Path(argv[argv.index("--output") + 1])
                output_path.write_text(json.dumps(produced))
                output_path.chmod(0o600)
                return types.SimpleNamespace(returncode=1)

            matrix.subprocess.run = fake_run
            with self.assertRaisesRegex(matrix.Stop, "returned nonzero") as raised:
                matrix.cold_witness(["python3", "sleepwake.py"], request, source_repo)
        finally:
            matrix.subprocess.run = old_run
        partial = raised.exception.partial_evidence
        self.assertEqual(partial["status"], "RED")
        self.assertEqual(partial["attempts"][0]["worker_version"], None)
        self.assertEqual(partial["attempts"][0]["finished_at"], "2026-09-08T00:01:00Z")
        self.assertEqual(partial["failure"], {"kind": "assertion", "message": "fabricd health wake did not return HTTP 200"})

    def test_red_artifact_without_structured_cause_is_refused(self):
        fixture = Path(__file__).with_name("fixtures") / "sleepwake-red-missing-failure.json"
        request = {"matrix_run_id": "run-missing-cause", "phase": "candidate", "expected_version_id": "candidate-version", "fabric_origin": "https://fabric.example"}
        old_run = matrix.subprocess.run
        try:
            def fake_run(argv, **kwargs):
                output_path = Path(argv[argv.index("--output") + 1])
                output_path.write_bytes(fixture.read_bytes())
                output_path.chmod(0o600)
                return types.SimpleNamespace(returncode=1)

            matrix.subprocess.run = fake_run
            with self.assertRaisesRegex(matrix.Stop, "failure artifact has no structured failure") as raised:
                matrix.cold_witness(["python3", "sleepwake.py"], request, "test-checkout")
        finally:
            matrix.subprocess.run = old_run
        self.assertIsNone(raised.exception.partial_evidence)

    def test_coordinator_records_child_failure_and_restores_after_rc_nonzero(self):
        events = []

        class FakeWrangler:
            def __init__(self, spawn_dir):
                self.active = matrix.STABLE_VERSION

            def preflight(self):
                return {"npm_ci": 0, "typecheck": 0, "test": 0}

            def candidate_deploy(self):
                events.append("deploy")
                self.active = "candidate-version"
                return 0

            def rollback_stable(self):
                events.append("rollback")
                self.active = matrix.STABLE_VERSION
                return 0

            def deployments(self):
                return [{"id": "deployment-" + self.active, "created_on": "2026-09-08T00:00:00Z", "versions": [{"version_id": self.active, "percentage": 100}]}]

            def version(self, version_id):
                return {"id": version_id, "resources": {"bindings": [
                    {"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"},
                    {"name": "AUTOSCALER_REDRIVE_PAUSED", "text": "1"},
                ]}}

        partial = {
            "status": "RED",
            "phase": "candidate",
            "matrix_run_id": "run-coordinator-red",
            "expected_version_id": "candidate-version",
            "source_repo": SOURCE_REPO,
            "attempts": [{"attempt": 1, "attempt_id": "candidate-01", "started_at": "2026-09-08T00:00:00Z", "finished_at": "2026-09-08T00:01:00Z", "worker_version": "candidate-version"}],
            "failure": {"kind": "assertion", "message": "fabricd health wake did not return HTTP 200"},
        }
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "wrangler.jsonc").write_text("{}")
            key = root / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            output = root / "evidence.json"
            old_cls, old_fleet, old_sha, old_clean, old_cold = matrix.Wrangler, matrix.fleet_idle, matrix.source_sha, matrix.clean_checkout_sha, matrix.cold_witness
            try:
                matrix.Wrangler = FakeWrangler
                matrix.fleet_idle = lambda url, keyfile: {"busy": 0, "checked": 1, "unverifiable": 0}
                matrix.source_sha = lambda spawn_dir, declared=None: "c" * 40
                matrix.clean_checkout_sha = lambda path, label: "d" * 40
                def fake_cold(command, request, source_repo):
                    events.append("cold:" + request["phase"])
                    raise matrix.Stop("cold witness command returned nonzero or RED", partial_evidence=partial)

                matrix.cold_witness = fake_cold
                rc = matrix.main([
                    "--output", str(output), "--spawn-dir", str(root),
                    "--fleet-url", "https://fleet.example", "--fabric-origin", "https://fabric.example",
                    "--fleet-key-file", str(key), "--cold-witness-command", "mock",
                    "--source-repo", SOURCE_REPO,
                    "--matrix-run-id", "run-coordinator-red", "--source-sha", "c" * 40,
                    "--fabric-app-id", matrix.FABRIC_APP_ID, "--execute", "--ack-destructive",
                ])
            finally:
                matrix.Wrangler, matrix.fleet_idle, matrix.source_sha, matrix.clean_checkout_sha, matrix.cold_witness = old_cls, old_fleet, old_sha, old_clean, old_cold
            evidence = json.loads(output.read_text())
        self.assertEqual(rc, 1)
        self.assertEqual(evidence["status"], "RED")
        self.assertEqual(evidence["cold_witness_failures"][0]["failure"], partial["failure"])
        self.assertEqual(evidence["cold_witness_failures"][0]["attempts"][0]["finished_at"], "2026-09-08T00:01:00Z")
        self.assertEqual(evidence["final_worker_restore"]["status"], "PASS")
        self.assertEqual(events, ["deploy", "cold:candidate", "rollback"])

    def test_fleet_idle_requires_complete_strict_shape(self):
        with tempfile.TemporaryDirectory() as td:
            key = Path(td) / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            body = json.dumps({"busy": 0, "checked": 4, "unverifiable": 0, "runners": []}).encode()

            class Response:
                status = 200

                def __enter__(self):
                    return self

                def __exit__(self, *_):
                    return False

                def read(self, limit):
                    self.limit = limit
                    return body

            response = Response()
            seen = {}
            old = matrix._fleet_open
            try:
                def fake(request, timeout):
                    seen["url"] = request.full_url
                    seen["auth"] = request.headers["X-corelink-internal-auth"]
                    seen["agent"] = request.get_header("User-agent")
                    return response

                matrix._fleet_open = fake
                self.assertEqual(
                    matrix.fleet_idle("https://fleet.example", key),
                    {"busy": 0, "checked": 4, "unverifiable": 0},
                )
            finally:
                matrix._fleet_open = old
            self.assertEqual(seen["url"], "https://fleet.example/internal/v1/fleet/busy")
            self.assertEqual(seen["auth"], "secret")
            self.assertEqual(seen["agent"], matrix.USER_AGENT)
            self.assertEqual(response.limit, matrix.MAX_FLEET_BODY_BYTES + 1)

    def test_fleet_403_is_distinct_and_busy_is_refused(self):
        with tempfile.TemporaryDirectory() as td:
            key = Path(td) / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            old = matrix._fleet_open
            try:
                matrix._fleet_open = lambda request, timeout: (_ for _ in ()).throw(
                    HTTPError(request.full_url, 403, "Forbidden", {}, io.BytesIO(b"x"))
                )
                with self.assertRaisesRegex(matrix.Stop, "HTTP 403"):
                    matrix.fleet_idle("https://fleet.example", key)
            finally:
                matrix._fleet_open = old

    def test_latest_deployment_and_pauses_are_exact(self):
        rows = [
            {"id": "old", "created_on": "2026-09-08T00:00:00Z", "versions": [{"version_id": "old-v", "percentage": 100}]},
            {"id": "new", "created_on": "2026-09-08T01:00:00Z", "versions": [{"version_id": matrix.STABLE_VERSION, "percentage": 100}]},
        ]
        self.assertEqual(matrix.active_version(rows)[0], matrix.STABLE_VERSION)
        version = {
            "resources": {
                "bindings": [
                    {"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"},
                    {"name": "AUTOSCALER_REDRIVE_PAUSED", "text": "1"},
                ]
            }
        }
        self.assertEqual(matrix.pause_bindings(version)["AUTOSCALER_INTAKE_PAUSED"], "1")
        with self.assertRaises(matrix.Stop):
            matrix.pause_bindings({"resources": {"bindings": [{"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"}]}})

    def test_health_is_read_only_and_discards_body(self):
        class Response:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def read(self, limit):
                self.limit = limit
                return b"secret-looking-body"

        response = Response()
        old = matrix.urlopen
        try:
            matrix.urlopen = lambda request, timeout: response
            self.assertEqual(matrix.health("https://health.example/health"), {"status": 200})
        finally:
            matrix.urlopen = old
        self.assertEqual(response.limit, 256)

    def test_execute_matrix_is_exactly_ten_pairs_and_restores(self):
        events = []

        class FakeWrangler:
            def __init__(self, spawn_dir):
                self.deploy_calls = 0
                self.rollback_calls = 0
                self.active = matrix.STABLE_VERSION

            def candidate_deploy(self):
                events.append("candidate_deploy")
                self.deploy_calls += 1
                self.active = f"candidate-{self.deploy_calls}"
                return 0

            def preflight(self):
                return {"npm_ci": 0, "typecheck": 0, "test": 0}

            def rollback_stable(self):
                events.append("rollback")
                self.rollback_calls += 1
                self.active = matrix.STABLE_VERSION
                return 0

            def deployments(self):
                count = self.deploy_calls + self.rollback_calls
                return [{
                    "id": f"deployment-{count}-{self.rollback_calls}",
                    "created_on": f"2026-09-08T00:{count:02d}:00Z",
                    "versions": [{"version_id": self.active, "percentage": 100}],
                }]

            def version(self, version_id):
                return {"id": version_id, "resources": {"bindings": [
                    {"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"},
                    {"name": "AUTOSCALER_REDRIVE_PAUSED", "text": "1"},
                ]}}

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "wrangler.jsonc").write_text("{}")
            key = root / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            output = root / "evidence.json"
            old_cls, old_fleet, old_health, old_sha, old_clean = (
                matrix.Wrangler, matrix.fleet_idle, matrix.health, matrix.source_sha, matrix.clean_checkout_sha
            )
            old_monitor = matrix.monitor_active_version
            old_cold = matrix.cold_witness
            try:
                matrix.Wrangler = FakeWrangler
                def ordered_monitor(wrangler, expected):
                    result = old_monitor(wrangler, expected)
                    events.append("stability_sample_2")
                    return result
                matrix.monitor_active_version = ordered_monitor
                matrix.fleet_idle = lambda url, keyfile: (events.append("fleet") or {"busy": 0, "checked": 1, "unverifiable": 0})
                matrix.health = lambda url: {"status": 200}
                matrix.source_sha = lambda spawn_dir, declared=None: "a" * 40
                matrix.clean_checkout_sha = lambda path, label: "d" * 40
                def fake_cold(command, request, source_repo):
                    events.append("cold:" + request["phase"])
                    return {"status": "PASS", "phase": request["phase"], "matrix_run_id": request["matrix_run_id"], "expected_version_id": request["expected_version_id"], "attempts": [
                        {"attempt": n, "attempt_id": f"{request['phase']}-{n}", "started_at": "2026-09-08T00:00:00Z", "finished_at": "2026-09-08T00:01:00Z", "pre_wake": {"status": "scale_zero", "observed_at": "2026-09-08T00:00:00Z"}, "health": {"status": 200, "route": "/health", "observed_at": "2026-09-08T00:00:00Z", "phase": request["phase"], "attempt_id": f"{request['phase']}-{n}", "independent_request": True}, "instance": {"id": f"{request['phase']}-instance-{n}", "created_at": f"2026-09-08T00:{0 if request['phase'] == 'candidate' else 1:02d}:{n:02d}Z", "state": "running", "digest": matrix.EXPECTED_FABRICD_DIGEST, "independent": True}}
                        for n in range(1, 11)
                    ], "contract": {"fabric_origin": request.get("fabric_origin"), "wake_route": "/health", "health_path": "/health"}}
                matrix.cold_witness = fake_cold
                result_code = matrix.main([
                    "--output", str(output), "--spawn-dir", str(root),
                    "--fleet-url", "https://fleet.example", "--fabric-origin", "https://health.example",
                    "--fleet-key-file", str(key), "--cold-witness-command", "mock", "--source-repo", str(root),
                    "--source-sha", "a" * 40, "--fabric-app-id", matrix.FABRIC_APP_ID, "--execute", "--ack-destructive",
                ])
                self.assertEqual(result_code, 0, output.read_text())
            finally:
                matrix.Wrangler, matrix.fleet_idle, matrix.health, matrix.source_sha, matrix.clean_checkout_sha = (
                    old_cls, old_fleet, old_health, old_sha, old_clean
                )
                matrix.cold_witness = old_cold
                matrix.monitor_active_version = old_monitor
            evidence = json.loads(output.read_text())
            self.assertEqual(evidence["status"], "PASS")
            self.assertEqual(evidence["cycles_completed"], 10)
            self.assertEqual(evidence["candidate_version_id_cardinality"], 1)
            self.assertEqual(evidence["candidate_deployment_id_cardinality"], 1)
            self.assertEqual(len(evidence["records"]), 10)
            for record in evidence["records"]:
                self.assertEqual(record["prior"]["version_id"], matrix.STABLE_VERSION)
                self.assertEqual(record["candidate"]["traffic_percent"], 100)
                self.assertEqual(record["rollback"]["version_id"], matrix.STABLE_VERSION)
                self.assertEqual(record["rollback_target_version_id"], matrix.STABLE_VERSION)
                self.assertEqual(record["source_sha"], "a" * 40)
                self.assertEqual(record["candidate_health"]["phase"], "candidate")
                self.assertEqual(record["rollback_health"]["phase"], "rollback")
                self.assertTrue(record["candidate_health"]["independent_request"])
                self.assertTrue(record["rollback_health"]["independent_request"])
                self.assertEqual(record["candidate_health"]["attempt_id"], record["attempt_id"])
                self.assertEqual(record["rollback_health"]["attempt_id"], record["rollback_attempt_id"])
            self.assertEqual(evidence["final_worker_restore"]["status"], "PASS")
            self.assertEqual(evidence["contract"]["health_path"], "/health")
            self.assertEqual(evidence["contract"]["wake_route"], "/health")
            self.assertEqual(events.count("candidate_deploy"), 1)
            self.assertEqual(events.count("stability_sample_2"), 1)
            self.assertEqual(events.count("rollback"), 1)
            self.assertEqual(events.count("cold:candidate"), 1)
            self.assertEqual(events.count("cold:rollback"), 1)
            self.assertLess(events.index("stability_sample_2"), events.index("candidate_deploy"))
            self.assertLess(events.index("candidate_deploy"), events.index("cold:candidate"))
            self.assertLess(events.index("cold:candidate"), events.index("rollback"))
            self.assertLess(events.index("rollback"), events.index("cold:rollback"))

    def test_execute_failure_after_deploy_attempt_restores_stable(self):
        class FakeWrangler:
            rollback_calls = 0

            def __init__(self, spawn_dir):
                self.deploy_calls = 0

            def candidate_deploy(self):
                self.deploy_calls += 1
                return 7

            def preflight(self):
                return {"npm_ci": 0, "typecheck": 0, "test": 0}

            def rollback_stable(self):
                type(self).rollback_calls += 1
                return 0

            def deployments(self):
                return [{"id": "stable-deployment", "created_on": "2026-09-08T00:00:00Z", "versions": [{"version_id": matrix.STABLE_VERSION, "percentage": 100}]}]

            def version(self, version_id):
                return {"id": version_id, "resources": {"bindings": [
                    {"name": "AUTOSCALER_INTAKE_PAUSED", "text": "1"},
                    {"name": "AUTOSCALER_REDRIVE_PAUSED", "text": "1"},
                ]}}

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "wrangler.jsonc").write_text("{}")
            key = root / "fleet.key"
            key.write_text("secret")
            key.chmod(0o600)
            output = root / "evidence.json"
            old_cls, old_fleet, old_health, old_sha, old_clean = matrix.Wrangler, matrix.fleet_idle, matrix.health, matrix.source_sha, matrix.clean_checkout_sha
            old_cold = matrix.cold_witness
            try:
                matrix.Wrangler = FakeWrangler
                matrix.fleet_idle = lambda url, keyfile: {"busy": 0, "checked": 1, "unverifiable": 0}
                matrix.health = lambda url: {"status": 200}
                matrix.source_sha = lambda spawn_dir, declared=None: "b" * 40
                matrix.clean_checkout_sha = lambda path, label: "d" * 40
                matrix.cold_witness = lambda command, request, source_repo: {
                    "status": "PASS", **request, "health": {"status": 200, "observed_at": "2026-09-08T00:00:00Z", "phase": request["phase"], "attempt_id": request["attempt_id"], "independent_request": True}
                }
                self.assertEqual(matrix.main([
                    "--output", str(output), "--spawn-dir", str(root),
                    "--fleet-url", "https://fleet.example", "--fabric-origin", "https://health.example",
                    "--fleet-key-file", str(key), "--cold-witness-command", "mock", "--source-repo", str(root),
                    "--source-sha", "b" * 40, "--fabric-app-id", matrix.FABRIC_APP_ID, "--execute", "--ack-destructive",
                ]), 1)
            finally:
                matrix.Wrangler, matrix.fleet_idle, matrix.health, matrix.source_sha, matrix.clean_checkout_sha = old_cls, old_fleet, old_health, old_sha, old_clean
                matrix.cold_witness = old_cold
            evidence = json.loads(output.read_text())
            self.assertEqual(evidence["status"], "RED")
            self.assertTrue(evidence["mutation_started"])
            self.assertEqual(evidence["final_worker_restore"]["status"], "PASS")
            self.assertIn("candidate Worker deploy returned nonzero", evidence["failure"])


if __name__ == "__main__":
    unittest.main()
