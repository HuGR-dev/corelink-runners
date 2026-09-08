# corelink-verify

The Python reference verifier for CoreLink fabric `result_binding_sig_v2`
attestations — the customer-trust primitive in-language.

A customer trusts a CoreLink verdict because they can **verify** it themselves.
This package does exactly that: given a raw fabric response JSON and the
published ed25519 key, it reconstructs the v2 pre-image byte-for-byte (the
same formula as the fabric signer, the Rust CLI, and the external verifier) and
checks the detached signature.  No trust in the transport, no trust in the
intermediary — the math is the proof.

## Install

```bash
pip install corelink-verify
```

Requires Python 3.9+. The only runtime dependency is
[`cryptography`](https://pypi.org/project/cryptography/) (ed25519 verification).

## Usage

```python
from corelink_verify import verify_response_json

# pubkey from GET /v1/attestation/key
FABRIC_PUBKEY = "+X0vGNFOSY5t9jo7OTlJNZsoLZOxE172jw/QURNEYw4="

raw_json = """{"check_result": {...}, "result_binding_sig_v2": "..."}"""

outcome = verify_response_json(raw_json, FABRIC_PUBKEY)
# {"verified": True, "exit": 0, "artifacts": 2}

if not outcome["verified"]:
    raise RuntimeError("fabric response did not verify — do not trust this result")
```

An empty or absent `result_binding_sig_v2` raises immediately (loud error,
never a silent pass) — this indicates a pre-v2 fabric endpoint.

## Why verify?

CoreLink runners execute untrusted customer and AI-agent workloads.  The
`result_binding_sig_v2` covers the full verdict: memo key, stdout/stderr refs,
exit code, and every output artifact — in order.  The fabric's ed25519 key is
published at `GET /v1/attestation/key`.  Verifying the signature before acting
on a result closes the loop: the customer is not trusting a transport or an
intermediary, they are trusting a math proof that this exact verdict was
produced by the fabric that holds the private key.

## Drift safety

This package is byte-locked to
[`conformance/result_binding_v2.json`](../../conformance/result_binding_v2.json)
— the same shared vector mirrored by the external consumer.  The conformance test
(`tests/test_conformance.py`) asserts that `result_binding_preimage_v2(input)`
produces the exact `preimage_hex` in that file, and that the committed
signature verifies.  Any change to the v2 formula on the fabric side updates
the vector — and this test breaks — making silent drift impossible.
