"""corelink_verify — in-language v2 result-binding verifier.

A customer trusts a CoreLink fabric verdict because they can VERIFY the
``result_binding_sig_v2`` against the fabric's published ed25519 key.
This module is that check, client-side, byte-identical to the Rust
``crates/corelink-cli/src/binding.rs`` reference implementation.

Contract V2VERIFY (FROZEN — same formula as the fabric signer, the Rust
CLI, and the external verifier):

    LP(s)    = u32_be(len(utf8(s))) || utf8(s)
    preimage = LP(memo_key) || LP(stdout_ref) || LP(stderr_ref)
               || i32_be(exit)          # 4-byte big-endian two's-complement
               || u32_be(len(artifacts))
               || for each artifact IN ORDER: LP(path) || LP(digest)

Locked to ``conformance/result_binding_v2.json`` — the same shared
conformance vector external clients mirror — so this module can never drift from
the fabric signer or the external verifier without the golden test breaking.
"""

from __future__ import annotations

import json
import struct
from typing import Any

__all__ = [
    "result_binding_preimage_v2",
    "verify_result_binding_v2",
    "verify_response_json",
]


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _lp(s: str) -> bytes:
    """``LP(s) = u32_be(byte_len(s)) || utf8_bytes(s)`` — shared framing."""
    encoded = s.encode("utf-8")
    length = len(encoded)
    if length > 0xFFFF_FFFF:
        raise ValueError(f"binding field exceeds u32::MAX bytes ({length})")
    return struct.pack(">I", length) + encoded


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def result_binding_preimage_v2(result: dict[str, Any]) -> bytes:
    """Compute the v2 result-binding pre-image from a CheckResult dict.

    The formula is byte-identical to the Rust reference in
    ``crates/corelink-cli/src/binding.rs``::result_binding_preimage_v2:

        LP(memo_key) || LP(stdout_ref) || LP(stderr_ref)
        || i32_be(exit)
        || u32_be(len(artifacts))
        || for each artifact: LP(path) || LP(digest)

    Args:
        result: A dict with at minimum the v2-binding fields:
            ``memo_key`` (str), ``stdout_ref`` (str), ``stderr_ref`` (str),
            ``exit`` (int, i32), ``artifacts`` (list of {path, digest}).

    Returns:
        The raw pre-image bytes.

    Raises:
        KeyError: A required field is missing from *result*.
        ValueError: A field value is out of range (e.g. exit not i32,
            artifact count or string exceeds u32::MAX).
        struct.error: Packing failure (should not occur for valid i32).
    """
    out = bytearray()

    out += _lp(result["memo_key"])
    out += _lp(result["stdout_ref"])
    out += _lp(result["stderr_ref"])

    # 4-byte big-endian two's-complement signed integer — exit may be negative.
    exit_code = int(result["exit"])
    if not (-2**31 <= exit_code <= 2**31 - 1):
        raise ValueError(f"exit code {exit_code} is out of i32 range")
    out += struct.pack(">i", exit_code)

    artifacts = result["artifacts"]
    artifact_count = len(artifacts)
    if artifact_count > 0xFFFF_FFFF:
        raise ValueError(f"artifact count {artifact_count} exceeds u32::MAX")
    out += struct.pack(">I", artifact_count)

    for artifact in artifacts:
        out += _lp(artifact["path"])
        out += _lp(artifact["digest"])

    return bytes(out)


def verify_result_binding_v2(
    result: dict[str, Any],
    sig_b64: str,
    pubkey_b64: str,
) -> bool:
    """Verify a detached std-base64 ed25519 ``result_binding_sig_v2``.

    Args:
        result: CheckResult dict (see :func:`result_binding_preimage_v2`).
        sig_b64: Standard (not URL-safe) base64-encoded detached ed25519
            signature from the fabric response.
        pubkey_b64: Standard base64-encoded 32-byte ed25519 public key
            from ``GET /v1/attestation/key``.

    Returns:
        ``True`` if the verdict + outputs are authentic; ``False`` if the
        signature does not verify (forged / wrong key / tampered result).

    Raises:
        ValueError: Malformed key or signature (wrong length, bad base64).
        Exception: Any underlying cryptography error propagates; never
            returns ``True`` on a malformed input.
    """
    import base64
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    from cryptography.exceptions import InvalidSignature

    pk_bytes = base64.b64decode(pubkey_b64.strip())
    if len(pk_bytes) != 32:
        raise ValueError(
            f"ed25519 pubkey must be 32 bytes, got {len(pk_bytes)}"
        )

    sig_bytes = base64.b64decode(sig_b64.strip())
    if len(sig_bytes) != 64:
        raise ValueError(
            f"ed25519 signature must be 64 bytes, got {len(sig_bytes)}"
        )

    vk = Ed25519PublicKey.from_public_bytes(pk_bytes)
    preimage = result_binding_preimage_v2(result)

    try:
        vk.verify(sig_bytes, preimage)
        return True
    except InvalidSignature:
        return False


def verify_response_json(raw: str, pubkey_b64: str) -> dict[str, Any]:
    """Extract and verify a fabric response JSON payload end-to-end.

    Extracts the ``CheckResult`` (``check_result`` for a CloseResponse,
    or ``result`` for an ExecResponse) and ``result_binding_sig_v2``,
    then verifies the binding against *pubkey_b64*.

    An empty or absent ``result_binding_sig_v2`` is a LOUD error — it
    indicates a pre-v2 fabric and must never silently pass.

    Args:
        raw: The raw JSON string of the fabric response.
        pubkey_b64: Standard base64-encoded 32-byte ed25519 public key.

    Returns:
        A dict with keys:
            ``verified`` (bool) — whether the signature verified.
            ``exit`` (int) — the process exit code the signature covers.
            ``artifacts`` (int) — the number of artifacts the sig covers.

    Raises:
        ValueError: JSON parse error, missing fields, empty/absent sig,
            malformed key/sig, or any cryptographic failure.
    """
    try:
        v = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError(f"input is not valid JSON: {exc}") from exc

    # Prefer check_result (CloseResponse) over result (ExecResponse).
    cr_val = None
    check_result_candidate = v.get("check_result")
    if check_result_candidate is not None and check_result_candidate is not None:
        # Filter null explicitly (json null → None in Python).
        cr_val = check_result_candidate if check_result_candidate is not None else None
    if cr_val is None:
        cr_val = v.get("result")
    if cr_val is None:
        raise ValueError(
            "input JSON has neither a `check_result` nor a `result` object"
        )

    sig = v.get("result_binding_sig_v2")
    if not sig:  # absent, null, or empty string
        raise ValueError(
            "input JSON has no non-empty `result_binding_sig_v2` "
            "(was the fabric pre-v2? — this is never a silent pass)"
        )

    verified = verify_result_binding_v2(cr_val, sig, pubkey_b64)
    return {
        "verified": verified,
        "exit": int(cr_val["exit"]),
        "artifacts": len(cr_val.get("artifacts", [])),
    }
