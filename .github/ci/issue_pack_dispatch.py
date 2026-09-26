#!/usr/bin/env python3
"""Run a bounded issue pack using controls from the trusted main checkout."""
from __future__ import annotations

import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request
import unittest
from datetime import datetime
from pathlib import Path
from typing import Any
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
CATALOG_PATH = ROOT / ".github/ci/issue_pack_catalog.json"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
MAX_COMMANDS = 8
MAX_OUTPUT = 8192
REPO = "HuGR-dev/corelink-runners"
FIXED_COMMANDS = {
    ("trusted-readme-truth-check",),
    ("npm", "ci"),
    ("npm", "run", "typecheck"),
    ("npm", "test", "--", "--run", "test/index.test.ts"),
    ("npm", "test", "--", "--run", "test/devenv-credentials.test.ts"),
    ("npm", "run", "test:coverage"),
    ("npx", "vitest", "run", "test/billing-recovery.test.ts"),
    ("node", "--test", "deploy/cloudflare/test/historical-settlement-reconcile.mjs"),
    ("npx", "vitest", "run", "test/index.test.ts", "test/containment-intake.test.ts",
     "test/devenv-do.test.ts", "test/billing-recovery.test.ts",
     "test/usage-ledger-backfill.test.ts", "test/usage-event-conformance.test.ts"),
    ("cargo", "test", "--locked", "--lib", "-p", "corelink-fabric-server",
     "corelink_billing::tests"),
    ("cargo", "test", "--locked", "--lib", "-p", "corelink-runners-contracts",
     "conformance_"),
    ("actionlint", "-config-file", ".github/ci/actionlint-runner.yaml",
     ".github/workflows/build-cf-container-images.yml"),
    ("bash", "scripts/ci/runner-image-static-check.selftest.sh"),
    ("bash", "scripts/ci/runner-image-build-validation.selftest.sh"),
    ("cargo", "deny", "check"),
    ("cargo", "audit", "--deny", "warnings"),
    ("cargo", "check", "--workspace", "--all-targets", "--locked"),
    ("cargo", "test", "-p", "corelink-fabric-server", "--test", "server_bin",
     "config_pg_tls", "--locked"),
}


def catalog() -> dict[str, dict[str, Any]]:
    value = json.loads(CATALOG_PATH.read_text())
    if not isinstance(value, dict) or not value:
        raise ValueError("pack catalog must be a non-empty object")
    for pack_id, pack in value.items():
        if not re.fullmatch(r"issue-[0-9]+", pack_id):
            raise ValueError(f"invalid catalog key: {pack_id}")
        validate_pack_definition(pack_id, pack)
        paths, commands = pack.get("paths"), pack.get("commands")
        if not isinstance(paths, list) or not paths or any(
            not isinstance(path, str) or not path or path.startswith("/")
            or ".." in Path(path).parts or "*" in path or "?" in path
            for path in paths
        ):
            raise ValueError(f"{pack_id}: paths must be explicit repository-relative paths")
        if len(paths) != len(set(paths)):
            raise ValueError(f"{pack_id}: duplicate path")
        validate_commands(commands)
    return value


def validate_pack_definition(pack_id: str, pack: dict[str, Any]) -> None:
    issue_text = pack_id.removeprefix("issue-")
    if (not isinstance(pack, dict) or not isinstance(pack.get("issue"), int)
            or isinstance(pack.get("issue"), bool) or pack.get("issue") != int(issue_text)):
        raise ValueError(f"{pack_id}: issue metadata must match the catalog key")
    pull_request = pack.get("pull_request")
    if pull_request is not None and (
        not isinstance(pull_request, int) or isinstance(pull_request, bool) or pull_request < 1
    ):
        raise ValueError(f"{pack_id}: pull_request must be a positive integer")
    if not isinstance(pack.get("exact_paths", False), bool):
        raise ValueError(f"{pack_id}: exact_paths must be boolean")


def validate_commands(commands: Any) -> None:
    if not isinstance(commands, list) or not 1 <= len(commands) <= MAX_COMMANDS:
        raise ValueError("command list must be bounded")
    for command in commands:
        if (not isinstance(command, list) or not command
                or any(not isinstance(arg, str) or not arg for arg in command)):
            raise ValueError("commands must be non-empty argument arrays")
        if tuple(command) not in FIXED_COMMANDS:
            raise ValueError("command is not in the trusted command allowlist")
        if any(arg in {";", "&&", "||", "|", "`", "$()"} for arg in command):
            raise ValueError("shell syntax is not accepted")


def validate_inputs(pack_id: str, candidate_sha: str, base_sha: str,
                    pr_number: str, trusted_ref: str, repo: str,
                    pr_base_ref: str, pr_head_sha: str, pr_base_sha: str) -> dict[str, Any]:
    packs = catalog()
    if pack_id not in packs:
        raise ValueError("pack ID is not allowlisted")
    if not SHA_RE.fullmatch(candidate_sha) or not SHA_RE.fullmatch(base_sha):
        raise ValueError("candidate and base must be lowercase 40-hex SHAs")
    if not re.fullmatch(r"[1-9][0-9]{0,8}", pr_number):
        raise ValueError("PR number must be a positive decimal integer")
    if trusted_ref != "refs/heads/main":
        raise ValueError("dispatcher must run from protected main")
    if repo != REPO:
        raise ValueError("repository does not match this catalog")
    if pr_base_ref != "main" or pr_head_sha != candidate_sha or pr_base_sha != base_sha:
        raise ValueError("GitHub PR metadata does not bind the requested main base and exact head")
    pack = packs[pack_id]
    if pack.get("pull_request") is not None and pr_number != str(pack["pull_request"]):
        raise ValueError("PR number does not match the issue pack binding")
    return pack


def bind_pr_metadata() -> int:
    """Fetch PR metadata in a token-only step; candidate processes never inherit this token."""
    output = Path(os.environ["GITHUB_OUTPUT"])
    values: dict[str, str] = {
        "pr_binding_ok": "false",
        "pr_base_ref": "",
        "pr_head_sha": "",
        "pr_base_sha": "",
        "latest_ci_run_id": "",
        "latest_ci_run_number": "",
        "latest_ci_date": "",
    }
    try:
        pr_number = os.environ["PR_NUMBER"]
        candidate_sha = os.environ["CANDIDATE_SHA"]
        base_sha = os.environ["TARGET_BASE_SHA"]
        repository = os.environ["REPOSITORY"]
        if repository != REPO or not re.fullmatch(r"[1-9][0-9]{0,8}", pr_number):
            raise ValueError("invalid repository or PR number")
        token = os.environ["GH_TOKEN"]
        request = urllib.request.Request(
            f"https://api.github.com/repos/{REPO}/pulls/{pr_number}",
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "User-Agent": "corelink-exact-issue-pack",
                "X-GitHub-Api-Version": "2022-11-28",
            },
        )
        with urllib.request.urlopen(request, timeout=20) as response:
            pull = json.loads(response.read(1_000_001))
        pack = catalog().get(os.environ.get("PACK_ID", ""))
        if pack is None:
            raise ValueError("pack ID is not allowlisted")
        head_sha = pull["head"]["sha"]
        actual_base_sha = pull["base"]["sha"]
        base_ref = pull["base"]["ref"]
        if (pull.get("number") != int(pr_number)
                or (pack.get("pull_request") is not None
                    and pull.get("number") != pack["pull_request"])
                or pull.get("state") != "open" or pull.get("merged_at") is not None
                or pull["base"]["repo"]["full_name"] != REPO
                or pull["head"]["repo"]["full_name"] != REPO
                or pull["head"]["sha"] != candidate_sha
                or actual_base_sha != base_sha or base_ref != "main"):
            raise ValueError("PR is closed, stale, or not bound to the requested main head")
        if not SHA_RE.fullmatch(head_sha) or not SHA_RE.fullmatch(actual_base_sha):
            raise ValueError("GitHub returned a malformed PR SHA")
        values.update({
            "pr_binding_ok": "true",
            "pr_base_ref": base_ref,
            "pr_head_sha": head_sha,
            "pr_base_sha": actual_base_sha,
        })
        if os.environ.get("PACK_ID") == "issue-578":
            runs_url = (
                f"https://api.github.com/repos/{REPO}/actions/workflows/ci.yml/runs"
                "?branch=main&status=success&per_page=1"
            )
            runs_request = urllib.request.Request(
                runs_url,
                headers={
                    "Accept": "application/vnd.github+json",
                    "Authorization": f"Bearer {token}",
                    "User-Agent": "corelink-exact-issue-pack",
                    "X-GitHub-Api-Version": "2022-11-28",
                },
            )
            with urllib.request.urlopen(runs_request, timeout=20) as response:
                runs = json.loads(response.read(1_000_001)).get("workflow_runs", [])
            if not runs:
                raise ValueError("no successful main CI run is recorded")
            latest = runs[0]
            updated_date = workflow_run_date(latest)
            values.update({
                "latest_ci_run_id": str(latest["id"]),
                "latest_ci_run_number": str(latest["run_number"]),
                "latest_ci_date": updated_date,
            })
    except Exception as exc:
        # Do not put API error text in outputs; it can contain untrusted PR data.
        values["pr_binding_ok"] = "false"
        values["pr_base_ref"] = ""
        values["pr_head_sha"] = ""
        values["pr_base_sha"] = ""
        values["latest_ci_run_id"] = ""
        values["latest_ci_run_number"] = ""
        values["latest_ci_date"] = ""
        print(f"PR metadata binding failed: {type(exc).__name__}", file=sys.stderr)
    with output.open("a", encoding="utf-8") as stream:
        for key, value in values.items():
            stream.write(f"{key}={value}\n")
    return 0


def validate_changed_paths(pack: dict[str, Any], paths: list[str]) -> None:
    if not paths:
        raise ValueError("candidate diff from the PR merge base is empty")
    unexpected = sorted(set(paths) - set(pack["paths"]))
    if unexpected:
        raise ValueError(f"candidate changed paths outside the pack: {unexpected}")
    if pack.get("exact_paths") and set(paths) != set(pack["paths"]):
        missing = sorted(set(pack["paths"]) - set(paths))
        raise ValueError(f"candidate diff does not match the exact pack surface: {missing}")


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *args], check=True, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    return result.stdout.strip()


def changed_paths(candidate_dir: Path, base_sha: str, candidate_sha: str) -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(candidate_dir), "diff", "--no-renames", "--name-only",
         "-z", base_sha, candidate_sha],
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    return [part.decode("utf-8", "strict") for part in result.stdout.split(b"\0") if part]


def bounded_process(command: list[str], cwd: Path,
                    env: dict[str, str] | None = None) -> tuple[int, str]:
    process = subprocess.Popen(command, cwd=cwd, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    tail = bytearray()
    assert process.stdout is not None
    while chunk := process.stdout.read(4096):
        tail.extend(chunk)
        if len(tail) > MAX_OUTPUT:
            del tail[:-MAX_OUTPUT]
    return process.wait(), bytes(tail).decode("utf-8", "replace")


def preflight(candidate_dir: Path, pack: dict[str, Any], candidate_sha: str,
              base_sha: str, trusted_sha: str, pr_number: str) -> list[str]:
    if git(ROOT, "rev-parse", "HEAD") != trusted_sha:
        raise ValueError("trusted checkout does not match the dispatch commit")
    git(ROOT, "merge-base", "--is-ancestor", trusted_sha, "refs/remotes/origin/main")
    git(ROOT, "fetch", "--no-tags", "origin",
        f"+refs/pull/{pr_number}/head:refs/remotes/origin/issue-pack/{pr_number}/head",
        f"+refs/pull/{pr_number}/merge:refs/remotes/origin/issue-pack/{pr_number}/merge")
    pr_head = f"refs/remotes/origin/issue-pack/{pr_number}/head"
    pr_merge = f"refs/remotes/origin/issue-pack/{pr_number}/merge"
    if git(ROOT, "rev-parse", pr_head) != candidate_sha:
        raise ValueError("candidate SHA is not the exact current head of the supplied PR")
    merge_parents = git(ROOT, "rev-list", "--parents", "-n", "1", pr_merge).split()
    if len(merge_parents) != 3 or merge_parents[1:] != [base_sha, candidate_sha]:
        raise ValueError("base SHA and candidate SHA do not bind to the supplied PR")
    if git(candidate_dir, "rev-parse", "HEAD") != candidate_sha:
        raise ValueError("candidate checkout does not match the requested SHA")
    merge_base = git(candidate_dir, "merge-base", base_sha, candidate_sha)
    paths = changed_paths(candidate_dir, merge_base, candidate_sha)
    validate_changed_paths(pack, paths)
    diff_code, diff_output = bounded_process(
        ["git", "-C", str(candidate_dir), "diff", "--check", merge_base, candidate_sha],
        ROOT,
    )
    if diff_code:
        raise ValueError(f"git diff --check failed: {diff_output}")
    return paths


def candidate_env() -> dict[str, str]:
    """Only pass build-tool environment to candidate commands; no GitHub token."""
    keys = ("PATH", "HOME", "CI", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
            "CARGO_TERM_COLOR", "RUSTFLAGS", "NPM_CONFIG_CACHE")
    env = {key: os.environ[key] for key in keys if key in os.environ}
    env["CI"] = "true"
    env["NPM_CONFIG_AUDIT"] = "false"
    env["NPM_CONFIG_FUND"] = "false"
    return env


def run_catalog_command(command: list[str], candidate_dir: Path,
                        base_sha: str, candidate_sha: str) -> tuple[int, str]:
    if command == ["trusted-readme-truth-check"]:
        readme = (candidate_dir / "README.md").read_text()
        ok = validate_readme_truth(
            readme, os.environ.get("LATEST_CI_RUN_NUMBER", ""),
            os.environ.get("LATEST_CI_RUN_ID", ""), os.environ.get("LATEST_CI_DATE", ""),
        )
        return (0 if ok else 1, "README latest successful main CI run and non-enforcement statement verified")
    if command[0] == "actionlint":
        trusted_command = actionlint_invocation(command, candidate_dir)
        return bounded_process(trusted_command, ROOT, candidate_env())
    cwd = candidate_dir / "deploy/cloudflare" if command[0] in {"npm", "npx"} else candidate_dir
    return bounded_process(command, cwd, candidate_env())


def actionlint_invocation(command: list[str], candidate_dir: Path) -> list[str]:
    """Lint candidate workflow data with the dispatcher's trusted label config."""
    expected = [
        "actionlint", "-config-file", ".github/ci/actionlint-runner.yaml",
        ".github/workflows/build-cf-container-images.yml",
    ]
    if command != expected:
        raise ValueError("actionlint command is not the fixed trusted invocation")
    config = ROOT / ".github/ci/actionlint-runner.yaml"
    workflow = candidate_dir / ".github/workflows/build-cf-container-images.yml"
    return ["actionlint", "-config-file", str(config), str(workflow)]


def workflow_run_date(run: dict[str, Any]) -> str:
    """Validate a successful main workflow run and return its updated_at date."""
    if (run.get("status") != "completed" or run.get("conclusion") != "success"
            or run.get("head_branch") != "main"):
        raise ValueError("latest workflow run is not a completed successful main run")
    updated_at = run.get("updated_at")
    if not isinstance(updated_at, str) or not updated_at.strip():
        raise ValueError("latest workflow run has no update timestamp")
    try:
        updated = datetime.fromisoformat(updated_at.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError("latest workflow run has an invalid update timestamp") from exc
    if "T" not in updated_at or updated.tzinfo is None:
        raise ValueError("latest workflow run update timestamp must include time and timezone")
    return updated.date().isoformat()


def validate_readme_truth(readme: str, latest_run_number: str,
                          latest_run_id: str, latest_run_date: str) -> bool:
    run_link = re.search(
        r"\[#([0-9]+)\]\(https://github\.com/HuGR-dev/corelink-runners/actions/runs/([0-9]+)\)",
        readme,
    )
    return bool(
        run_link
        and latest_run_number
        and latest_run_id
        and latest_run_date
        and run_link.group(1) == latest_run_number
        and run_link.group(2) == latest_run_id
        and f"passed on {latest_run_date}" in readme
        and "CI success is not currently enforced before merge." in readme
    )


def write_receipt(path: Path, receipt: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")


def environment_pack() -> dict[str, Any]:
    pack = validate_inputs(
        os.environ.get("PACK_ID", ""), os.environ.get("CANDIDATE_SHA", ""),
        os.environ.get("TARGET_BASE_SHA", ""), os.environ.get("PR_NUMBER", ""),
        os.environ.get("TRUSTED_REF", ""), os.environ.get("REPOSITORY", ""),
        os.environ.get("PR_BASE_REF", ""), os.environ.get("PR_HEAD_SHA", ""),
        os.environ.get("PR_BASE_SHA", ""),
    )
    if os.environ.get("PR_BINDING_OK") != "true":
        raise ValueError("GitHub PR metadata binding did not pass")
    return pack


def run_preflight(candidate_dir: Path) -> int:
    try:
        pack = environment_pack()
        if os.environ.get("CANDIDATE_CHECKOUT_OUTCOME") != "success":
            raise ValueError("exact candidate checkout did not complete")
        paths = preflight(
            candidate_dir, pack, os.environ["CANDIDATE_SHA"],
            os.environ["TARGET_BASE_SHA"], os.environ["TRUSTED_SHA"],
            os.environ["PR_NUMBER"],
        )
        output = Path(os.environ["GITHUB_OUTPUT"])
        output.write_text(f"preflight_ok=true\ncandidate_paths={json.dumps(paths)}\n")
        print(json.dumps({"preflight_ok": True, "changed_paths": paths}, sort_keys=True))
        return 0
    except Exception as exc:
        print(f"preflight failed: {type(exc).__name__}: {exc}"[:MAX_OUTPUT], file=sys.stderr)
        return 1


def run_candidate(candidate_dir: Path) -> int:
    try:
        pack = environment_pack()
        if os.environ.get("PREFLIGHT_OK") != "true":
            raise ValueError("trusted preflight did not pass")
        if git(ROOT, "rev-parse", "HEAD") != os.environ["TRUSTED_SHA"]:
            raise ValueError("candidate job trusted checkout does not match dispatch commit")
        if git(candidate_dir, "rev-parse", "HEAD") != os.environ["CANDIDATE_SHA"]:
            raise ValueError("candidate checkout does not match requested SHA")
        for command in pack["commands"]:
            try:
                code, output = run_catalog_command(
                    command, candidate_dir, os.environ["TARGET_BASE_SHA"],
                    os.environ["CANDIDATE_SHA"],
                )
            except Exception as exc:
                print(f"command failed to start: {type(exc).__name__}"[:MAX_OUTPUT], file=sys.stderr)
                return 1
            print(json.dumps({
                "command": command,
                "exit_code": code,
                "output_tail": output[-MAX_OUTPUT:],
            }, sort_keys=True))
            if code:
                return code
        return 0
    except Exception as exc:
        print(f"candidate pack failed: {type(exc).__name__}: {exc}"[:MAX_OUTPUT], file=sys.stderr)
        return 1


def finalize_receipt() -> int:
    pack_id = os.environ.get("PACK_ID", "")[:64]
    pack = catalog().get(pack_id)
    prepare_result = os.environ.get("PREPARE_JOB_RESULT", "unknown")
    candidate_result = os.environ.get("CANDIDATE_JOB_RESULT", "unknown")
    metadata_binding = os.environ.get("PR_BINDING_OK") == "true"
    preflight_ok = os.environ.get("PREFLIGHT_OK") == "true"
    passed = (pack is not None and prepare_result == "success" and metadata_binding
              and preflight_ok and candidate_result == "success")
    try:
        changed_paths = json.loads(os.environ.get("CANDIDATE_PATHS", "[]"))
        if not isinstance(changed_paths, list) or any(not isinstance(path, str) for path in changed_paths):
            changed_paths = []
    except json.JSONDecodeError:
        changed_paths = []
    commands = [] if pack is None else [
        {
            "command": command,
            "status": "passed" if candidate_result == "success" and preflight_ok
            else "unavailable_due_to_candidate_job_failure",
            "exit_code": 0 if candidate_result == "success" and preflight_ok else None,
        }
        for command in pack["commands"]
    ]
    receipt: dict[str, Any] = {
        "schema_version": 1,
        "pack_id": pack_id,
        "issue": None if pack is None else pack["issue"],
        "candidate_sha": os.environ.get("CANDIDATE_SHA", "")[:256],
        "base_sha": os.environ.get("TARGET_BASE_SHA", "")[:256],
        "pr_number": os.environ.get("PR_NUMBER", "")[:32],
        "pr_base_ref": os.environ.get("PR_BASE_REF", "")[:128],
        "metadata_binding": metadata_binding,
        "trusted_controls_sha": os.environ.get("TRUSTED_SHA", "")[:256],
        "trusted_ref": os.environ.get("TRUSTED_REF", "")[:256],
        "run_url": os.environ.get("RUN_URL", "")[:512],
        "prepare_job_result": prepare_result,
        "candidate_job_result": candidate_result,
        "latest_successful_ci_run": (
            {
                "id": os.environ.get("LATEST_CI_RUN_ID", "")[:32],
                "run_number": os.environ.get("LATEST_CI_RUN_NUMBER", "")[:32],
                "completed_date": os.environ.get("LATEST_CI_DATE", "")[:32],
            }
            if pack_id == "issue-578" else None
        ),
        "changed_paths": changed_paths,
        "commands": commands,
        "status": "passed" if passed else "failed",
    }
    if not passed:
        receipt["error"] = (
            "Trusted preparation or exact-head candidate job did not pass; "
            "see this workflow run for step details."
        )
    write_receipt(Path(os.environ["RECEIPT_PATH"]), receipt)
    print(json.dumps(receipt, sort_keys=True))
    return 0


class DispatcherTests(unittest.TestCase):
    def test_every_declared_pack_has_an_accepted_explicit_surface(self) -> None:
        for pack_id, pack in catalog().items():
            with self.subTest(pack=pack_id):
                pr_number = str(pack.get("pull_request", 123))
                accepted = validate_inputs(pack_id, "a" * 40, "b" * 40, pr_number,
                                           "refs/heads/main", REPO, "main", "a" * 40, "b" * 40)
                self.assertEqual(accepted["paths"], pack["paths"])
                validate_commands(accepted["commands"])
                changed = pack["paths"] if pack.get("exact_paths") else [pack["paths"][0]]
                validate_changed_paths(pack, changed)

    def test_issue_566_allows_containment_redrive_reservation_test(self) -> None:
        path = "deploy/cloudflare/test/containment-redrive-reservation.test.ts"
        pack = catalog()["issue-566"]
        self.assertIn(path, pack["paths"])
        validate_changed_paths(pack, [path])

    def test_issue_603_binds_exact_issue_pr_surface_and_command(self) -> None:
        pack = catalog()["issue-603"]
        expected_paths = [
            "deploy/cloudflare/test/fixtures/issue-603/recovery-matrix.json",
            "deploy/cloudflare/test/historical-settlement-reconcile.mjs",
            "docs/historical-settlement-recovery.md",
        ]
        command = ["node", "--test", "deploy/cloudflare/test/historical-settlement-reconcile.mjs"]
        self.assertEqual(pack["issue"], 603)
        self.assertEqual(pack["pull_request"], 626)
        self.assertEqual(pack["paths"], expected_paths)
        self.assertEqual(pack["commands"], [command])
        accepted = validate_inputs(
            "issue-603", "a" * 40, "b" * 40, "626", "refs/heads/main", REPO,
            "main", "a" * 40, "b" * 40,
        )
        self.assertEqual(accepted, pack)
        validate_changed_paths(pack, expected_paths)
        for wrong_issue in ({**pack, "issue": 604},):
            with self.assertRaises(ValueError):
                validate_pack_definition("issue-603", wrong_issue)
        for wrong_pr in ("627", "603"):
            with self.subTest(pr=wrong_pr), self.assertRaises(ValueError):
                validate_inputs(
                    "issue-603", "a" * 40, "b" * 40, wrong_pr, "refs/heads/main", REPO,
                    "main", "a" * 40, "b" * 40,
                )
        for wrong_head, wrong_base, wrong_pr_head, wrong_pr_base in (
            ("c" * 40, "b" * 40, "a" * 40, "b" * 40),
            ("a" * 40, "c" * 40, "a" * 40, "b" * 40),
            ("a" * 40, "b" * 40, "c" * 40, "b" * 40),
            ("a" * 40, "b" * 40, "a" * 40, "c" * 40),
        ):
            with self.subTest(head=wrong_head, base=wrong_base,
                              pr_head=wrong_pr_head, pr_base=wrong_pr_base), self.assertRaises(ValueError):
                validate_inputs(
                    "issue-603", wrong_head, wrong_base, "626", "refs/heads/main", REPO,
                    "main", wrong_pr_head, wrong_pr_base,
                )
        for wrong_paths in (
            [*expected_paths, "deploy/cloudflare/src/index.ts"],
            expected_paths[:-1],
        ):
            with self.subTest(paths=wrong_paths), self.assertRaises(ValueError):
                validate_changed_paths(pack, wrong_paths)
        with self.assertRaises(ValueError):
            validate_commands([["node", "--test", "candidate-authored-test.mjs"]])
        with self.assertRaises(ValueError):
            validate_commands([["node", "--test", "deploy/cloudflare/test/historical-settlement-reconcile.mjs", "&&", "curl"]])

    def test_issue_604_binds_exact_issue_pr_surface_and_commands(self) -> None:
        pack = catalog()["issue-604"]
        expected_paths = [
            "conformance/billing-ingest-ack-v1.json",
            "conformance/manifest.sha256",
            "crates/corelink-fabric-server/src/corelink_billing.rs",
            "deploy/cloudflare/src/lib.ts",
            "deploy/cloudflare/test/billing-recovery.test.ts",
            "deploy/cloudflare/test/containment-intake.test.ts",
            "deploy/cloudflare/test/devenv-do.test.ts",
            "deploy/cloudflare/test/helpers/billing-ack.ts",
            "deploy/cloudflare/test/index.test.ts",
            "deploy/cloudflare/test/usage-event-conformance.test.ts",
            "deploy/cloudflare/test/usage-ledger-backfill.test.ts",
            "docs/contracts/billing-ingest-ack-v1.md",
            "scripts/ci/secret-scan.sh",
        ]
        expected_commands = [
            ["npm", "ci"],
            ["npm", "run", "typecheck"],
            ["npx", "vitest", "run", "test/index.test.ts", "test/containment-intake.test.ts",
             "test/devenv-do.test.ts", "test/billing-recovery.test.ts",
             "test/usage-ledger-backfill.test.ts", "test/usage-event-conformance.test.ts"],
            ["cargo", "test", "--locked", "--lib", "-p", "corelink-fabric-server",
             "corelink_billing::tests"],
            ["cargo", "test", "--locked", "--lib", "-p", "corelink-runners-contracts",
             "conformance_"],
        ]
        self.assertEqual(pack["issue"], 604)
        self.assertEqual(pack["pull_request"], 634)
        self.assertTrue(pack["exact_paths"])
        self.assertEqual(pack["paths"], expected_paths)
        self.assertEqual(pack["commands"], expected_commands)
        accepted = validate_inputs(
            "issue-604", "a" * 40, "b" * 40, "634", "refs/heads/main", REPO,
            "main", "a" * 40, "b" * 40,
        )
        self.assertEqual(accepted, pack)
        validate_changed_paths(pack, expected_paths)
        with self.assertRaises(ValueError):
            validate_pack_definition("issue-604", {**pack, "issue": 603})
        with self.assertRaises(ValueError):
            validate_pack_definition("issue-604", {**pack, "pull_request": True})
        for wrong_pr in ("633", "635"):
            with self.subTest(pr=wrong_pr), self.assertRaises(ValueError):
                validate_inputs(
                    "issue-604", "a" * 40, "b" * 40, wrong_pr, "refs/heads/main", REPO,
                    "main", "a" * 40, "b" * 40,
                )
        for wrong_paths in (
            [*expected_paths, "deploy/cloudflare/src/index.ts"],
            expected_paths[:-1],
        ):
            with self.subTest(paths=wrong_paths), self.assertRaises(ValueError):
                validate_changed_paths(pack, wrong_paths)
        for wrong_head, wrong_base, wrong_pr_head, wrong_pr_base in (
            ("c" * 40, "b" * 40, "a" * 40, "b" * 40),
            ("a" * 40, "c" * 40, "a" * 40, "b" * 40),
            ("a" * 40, "b" * 40, "c" * 40, "b" * 40),
            ("a" * 40, "b" * 40, "a" * 40, "c" * 40),
        ):
            with self.subTest(head=wrong_head, base=wrong_base,
                              pr_head=wrong_pr_head, pr_base=wrong_pr_base), self.assertRaises(ValueError):
                validate_inputs(
                    "issue-604", wrong_head, wrong_base, "634", "refs/heads/main", REPO,
                    "main", wrong_pr_head, wrong_pr_base,
                )
        with self.assertRaises(ValueError):
            validate_inputs(
                "issue-604", "a" * 40, "b" * 40, "634", "refs/heads/main",
                "attacker/corelink-runners", "main", "a" * 40, "b" * 40,
            )
        with self.assertRaises(ValueError):
            validate_inputs(
                "issue-604", "a" * 40, "b" * 40, "634", "refs/heads/feature", REPO,
                "main", "a" * 40, "b" * 40,
            )
        with self.assertRaises(ValueError):
            validate_commands([["cargo", "test", "--workspace"]])
        with self.assertRaises(ValueError):
            validate_commands([expected_commands[2] + ["&&", "curl"]])

    def test_issue_575_actionlint_uses_trusted_minimal_config(self) -> None:
        command = catalog()["issue-575"]["commands"][0]
        self.assertEqual(
            (ROOT / ".github/ci/actionlint-runner.yaml").read_text(encoding="utf-8"),
            "---\nself-hosted-runner:\n  labels:\n    - corelink\n",
        )
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory)
            (candidate / ".github/actionlint.yaml").parent.mkdir(parents=True)
            (candidate / ".github/actionlint.yaml").write_text(
                "paths:\n  '**':\n    ignore:\n      - '.*'\n", encoding="utf-8"
            )
            calls: list[tuple[list[str], Path]] = []

            def capture(invocation: list[str], cwd: Path,
                        env: dict[str, str] | None = None) -> tuple[int, str]:
                calls.append((invocation, cwd))
                return 0, "ok"

            with mock.patch(__name__ + ".bounded_process", side_effect=capture):
                code, _ = run_catalog_command(command, candidate, "", "")
        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        invocation, cwd = calls[0]
        self.assertEqual(cwd, ROOT)
        self.assertEqual(invocation[:3], [
            "actionlint", "-config-file", str(ROOT / ".github/ci/actionlint-runner.yaml")
        ])
        self.assertEqual(invocation[3], str(candidate / ".github/workflows/build-cf-container-images.yml"))

    def test_issue_578_uses_updated_at_and_rejects_incomplete_run_metadata(self) -> None:
        run = {
            "status": "completed",
            "conclusion": "success",
            "head_branch": "main",
            "updated_at": "2026-09-22T23:30:00Z",
            "completed_at": "2026-09-23T00:30:00Z",
        }
        self.assertEqual(workflow_run_date(run), "2026-09-22")
        invalid_runs = [
            {**run, "status": "in_progress"},
            {**run, "conclusion": "failure"},
            {**run, "head_branch": "release"},
            {**run, "updated_at": ""},
            {**run, "updated_at": None},
            {**run, "updated_at": "2026-09-22"},
            {**run, "updated_at": "not-a-timestamp"},
        ]
        for invalid in invalid_runs:
            with self.subTest(run=invalid), self.assertRaises(ValueError):
                workflow_run_date(invalid)

    def test_rejects_undeclared_pack_path_sha_command_and_dispatch_ref(self) -> None:
        with self.assertRaises(ValueError):
            validate_inputs("issue-999", "a" * 40, "b" * 40, "123", "refs/heads/main", REPO,
                            "main", "a" * 40, "b" * 40)
        with self.assertRaises(ValueError):
            validate_changed_paths(catalog()["issue-566"], ["deploy/cloudflare/package.json"])
        with self.assertRaises(ValueError):
            validate_inputs("issue-566", "a" * 39, "b" * 40, "123", "refs/heads/main", REPO,
                            "main", "a" * 40, "b" * 40)
        with self.assertRaises(ValueError):
            validate_commands([["bash", "candidate-authored-command.sh"]])
        with self.assertRaises(ValueError):
            validate_commands([["bash", "scripts/ci/runner-image-build-validation.sh"]])
        with self.assertRaises(ValueError):
            validate_commands([["npm", "test", "&&", "curl"]])
        with self.assertRaises(ValueError):
            validate_inputs("issue-566", "a" * 40, "b" * 40, "123", "refs/heads/feature", REPO,
                            "main", "a" * 40, "b" * 40)
        with self.assertRaises(ValueError):
            validate_inputs("issue-566", "a" * 40, "b" * 40, "123", "refs/heads/main", REPO,
                            "release", "a" * 40, "b" * 40)
        with self.assertRaises(ValueError):
            validate_inputs("issue-566", "a" * 40, "b" * 40, "123", "refs/heads/main", REPO,
                            "main", "a" * 40, "c" * 40)

    def test_candidate_environment_excludes_github_token(self) -> None:
        with mock.patch.dict(os.environ, {"GH_TOKEN": "must-not-reach-candidate"}):
            self.assertNotIn("GH_TOKEN", candidate_env())

    def test_command_output_is_streamed_into_a_bounded_tail(self) -> None:
        code, output = bounded_process(
            [sys.executable, "-c", "import sys; sys.stdout.write('x' * 50000 + 'END')"], ROOT
        )
        self.assertEqual(code, 0)
        self.assertEqual(len(output.encode("utf-8")), MAX_OUTPUT)
        self.assertTrue(output.endswith("END"))

    def test_failed_candidate_job_cannot_become_a_passing_receipt(self) -> None:
        env = {
            "PACK_ID": "issue-566",
            "CANDIDATE_SHA": "a" * 40,
            "TARGET_BASE_SHA": "b" * 40,
            "PR_NUMBER": "123",
            "PR_BASE_REF": "main",
            "PR_BINDING_OK": "true",
            "TRUSTED_SHA": "c" * 40,
            "TRUSTED_REF": "refs/heads/main",
            "RUN_URL": "https://github.com/HuGR-dev/corelink-runners/actions/runs/123",
            "PREPARE_JOB_RESULT": "success",
            "CANDIDATE_JOB_RESULT": "failure",
            "LATEST_CI_RUN_ID": "456",
            "LATEST_CI_RUN_NUMBER": "123",
            "LATEST_CI_DATE": "2026-09-25",
            "PREFLIGHT_OK": "true",
            "CANDIDATE_PATHS": '["deploy/cloudflare/src/lib.ts"]',
        }
        with tempfile.TemporaryDirectory() as directory:
            receipt_path = Path(directory) / "receipt.json"
            with mock.patch.dict(os.environ, {**env, "RECEIPT_PATH": str(receipt_path)}):
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(finalize_receipt(), 0)
            receipt = json.loads(receipt_path.read_text())
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["candidate_job_result"], "failure")
        self.assertTrue(all(row["exit_code"] is None for row in receipt["commands"]))

    def test_issue_604_passing_receipt_binds_trusted_and_candidate_metadata(self) -> None:
        pack = catalog()["issue-604"]
        env = {
            "PACK_ID": "issue-604",
            "CANDIDATE_SHA": "a" * 40,
            "TARGET_BASE_SHA": "b" * 40,
            "PR_NUMBER": "634",
            "PR_BASE_REF": "main",
            "PR_BINDING_OK": "true",
            "TRUSTED_SHA": "c" * 40,
            "TRUSTED_REF": "refs/heads/main",
            "RUN_URL": "https://github.com/HuGR-dev/corelink-runners/actions/runs/123",
            "PREPARE_JOB_RESULT": "success",
            "CANDIDATE_JOB_RESULT": "success",
            "PREFLIGHT_OK": "true",
            "CANDIDATE_PATHS": json.dumps(pack["paths"]),
        }
        with tempfile.TemporaryDirectory() as directory:
            receipt_path = Path(directory) / "receipt.json"
            with mock.patch.dict(os.environ, {**env, "RECEIPT_PATH": str(receipt_path)}):
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(finalize_receipt(), 0)
            receipt = json.loads(receipt_path.read_text())
        self.assertEqual(receipt["status"], "passed")
        self.assertEqual(receipt["issue"], 604)
        self.assertEqual(receipt["pr_number"], "634")
        self.assertEqual(receipt["candidate_sha"], "a" * 40)
        self.assertEqual(receipt["base_sha"], "b" * 40)
        self.assertEqual(receipt["pr_base_ref"], "main")
        self.assertTrue(receipt["metadata_binding"])
        self.assertEqual(receipt["trusted_controls_sha"], "c" * 40)
        self.assertEqual(receipt["trusted_ref"], "refs/heads/main")
        self.assertEqual(receipt["changed_paths"], pack["paths"])
        self.assertEqual(
            receipt["commands"],
            [{"command": command, "status": "passed", "exit_code": 0}
             for command in pack["commands"]],
        )

    def test_readme_pack_requires_the_exact_latest_successful_run_and_date(self) -> None:
        readme = (
            "CI passed on 2026-09-25: [#123](https://github.com/HuGR-dev/corelink-runners/actions/runs/456)\n"
            "CI success is not currently enforced before merge."
        )
        self.assertTrue(validate_readme_truth(readme, "123", "456", "2026-09-25"))
        self.assertFalse(validate_readme_truth(readme, "122", "456", "2026-09-25"))
        self.assertFalse(validate_readme_truth(readme, "123", "456", "2026-09-24"))


def main() -> int:
    if "--self-test" in sys.argv:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(DispatcherTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    if "--bind-pr" in sys.argv:
        return bind_pr_metadata()
    if "--finalize" in sys.argv:
        return finalize_receipt()
    if "--preflight" in sys.argv or "--candidate" in sys.argv:
        try:
            index = sys.argv.index("--candidate-dir")
            candidate_dir = (ROOT / sys.argv[index + 1]).resolve()
            if ROOT not in candidate_dir.parents:
                raise ValueError("candidate directory must be a separate child of workspace")
        except (ValueError, IndexError) as exc:
            print(f"invalid invocation: {exc}", file=sys.stderr)
            return 2
        if "--preflight" in sys.argv:
            return run_preflight(candidate_dir)
        return run_candidate(candidate_dir)
    print("use --self-test, --bind-pr, --preflight, --candidate, or --finalize", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
