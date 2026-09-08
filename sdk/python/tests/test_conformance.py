"""Conformance test — the DRIFT TRIPWIRE.

Loads ``conformance/result_binding_v2.json`` from the repo root (two
directories above this file) and asserts that the Python implementation
produces byte-identical results to the Rust reference implementation in
``crates/corelink-cli/src/binding.rs``.

This is the same shared vector external clients mirror.  Any change to the v2 formula
on the fabric side changes the vector — and this test breaks — making
drift impossible to miss.
"""

from __future__ import annotations

import json
import pathlib
import pytest

# The conformance vector lives at <repo-root>/conformance/result_binding_v2.json.
# This file is at <repo-root>/sdk/python/tests/test_conformance.py.
_REPO_ROOT = pathlib.Path(__file__).parent.parent.parent.parent
_VECTOR_PATH = _REPO_ROOT / "conformance" / "result_binding_v2.json"

from corelink_verify import (
    result_binding_preimage_v2,
    verify_result_binding_v2,
    verify_response_json,
)


@pytest.fixture(scope="module")
def vector() -> dict:
    """Load the shared conformance vector once per test session."""
    assert _VECTOR_PATH.exists(), (
        f"Conformance vector not found at {_VECTOR_PATH}. "
        "Run from the repo root."
    )
    return json.loads(_VECTOR_PATH.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def check_result(vector: dict) -> dict:
    """Build a CheckResult dict from the vector's input (v2-binding fields only)."""
    inp = vector["input"]
    return {
        "memo_key": inp["memo_key"],
        "stdout_ref": inp["stdout_ref"],
        "stderr_ref": inp["stderr_ref"],
        "exit": inp["exit"],
        "artifacts": [
            {"path": a["path"], "digest": a["digest"]}
            for a in inp["artifacts"]
        ],
        # Non-v2 fields — present so verify_response_json roundtrip works;
        # they do not enter the preimage.
        "tree_hash": "",
        "def_digest": "",
        "toolchain_digest": "",
        "duration_ms": 0,
        "runner_ref": "",
        "produced_at": 0,
    }


class TestPreimage:
    """(a) The preimage computation is byte-identical to the shared vector."""

    def test_preimage_hex_matches_vector(self, vector: dict, check_result: dict) -> None:
        """result_binding_preimage_v2(input) hex MUST equal vector.preimage_hex."""
        preimage = result_binding_preimage_v2(check_result)
        got = preimage.hex()
        expected = vector["preimage_hex"]
        assert got == expected, (
            f"Python v2 preimage diverged from conformance/result_binding_v2.json\n"
            f"  got:      {got[:80]}...\n"
            f"  expected: {expected[:80]}..."
        )


class TestVerify:
    """(b) Authentic sig verifies; (c) tamper (exit ± 1) does not."""

    def test_authentic_sig_verifies(self, vector: dict, check_result: dict) -> None:
        """verify_result_binding_v2 returns True for the authentic vector sig."""
        result = verify_result_binding_v2(
            check_result,
            vector["result_binding_sig_v2"],
            vector["fabric_pubkey_b64"],
        )
        assert result is True, "authentic v2 signature must verify"

    def test_tampered_exit_does_not_verify(
        self, vector: dict, check_result: dict
    ) -> None:
        """Flipping exit by 1 causes verify to return False (binding is over exit)."""
        tampered = dict(check_result)
        tampered["exit"] = 1 if check_result["exit"] == 0 else check_result["exit"] + 1
        result = verify_result_binding_v2(
            tampered,
            vector["result_binding_sig_v2"],
            vector["fabric_pubkey_b64"],
        )
        assert result is False, (
            "a flipped exit MUST NOT verify — v2 binds the verdict"
        )


class TestEmptySig:
    """(d) Empty or absent sig is a LOUD error, never a silent pass."""

    def test_empty_sig_raises_in_verify_result_binding(
        self, vector: dict, check_result: dict
    ) -> None:
        """verify_result_binding_v2 with empty sig must raise, not return False."""
        with pytest.raises(Exception):
            verify_result_binding_v2(
                check_result,
                "",  # empty sig — invalid base64 / wrong-length bytes
                vector["fabric_pubkey_b64"],
            )

    def test_absent_sig_raises_in_verify_response_json(
        self, vector: dict, check_result: dict
    ) -> None:
        """verify_response_json with absent result_binding_sig_v2 raises."""
        payload = json.dumps({
            "check_result": check_result,
            # no result_binding_sig_v2 key at all
        })
        with pytest.raises((ValueError, KeyError, Exception)):
            verify_response_json(payload, vector["fabric_pubkey_b64"])

    def test_empty_string_sig_raises_in_verify_response_json(
        self, vector: dict, check_result: dict
    ) -> None:
        """verify_response_json with empty result_binding_sig_v2 raises (pre-v2 fabric)."""
        payload = json.dumps({
            "check_result": check_result,
            "result_binding_sig_v2": "",  # empty string — pre-v2 fabric
        })
        with pytest.raises((ValueError, KeyError, Exception)):
            verify_response_json(payload, vector["fabric_pubkey_b64"])


class TestResponseJsonExtraction:
    """End-to-end: verify_response_json handles both CloseResponse and ExecResponse shapes."""

    def test_close_response_shape(self, vector: dict, check_result: dict) -> None:
        """check_result key (CloseResponse) is preferred and verified."""
        payload = json.dumps({
            "check_result": check_result,
            "result_binding_sig_v2": vector["result_binding_sig_v2"],
        })
        out = verify_response_json(payload, vector["fabric_pubkey_b64"])
        assert out["verified"] is True
        assert out["exit"] == vector["input"]["exit"]
        assert out["artifacts"] == len(vector["input"]["artifacts"])

    def test_exec_response_shape(self, vector: dict, check_result: dict) -> None:
        """result key (ExecResponse) is also accepted and verified."""
        payload = json.dumps({
            "result": check_result,
            "result_binding_sig_v2": vector["result_binding_sig_v2"],
        })
        out = verify_response_json(payload, vector["fabric_pubkey_b64"])
        assert out["verified"] is True
