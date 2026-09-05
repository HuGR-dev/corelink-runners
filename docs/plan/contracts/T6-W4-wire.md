# T6-W4 producer wire decisions

Offline frozen implementation contract: signed ACK and recovery tokens use compact UTF-8 JSON arrays in the canonical DAG field order, Ed25519 base64url signatures, raw 32-byte base64url public keys, and lowercase SHA-256 hexadecimal digests. Production has no trust capability until the independently verified role/manifest/revocation adapter is bound; absence refuses ACK acceptance. This copies the orchestrator's 2026-09-05 T6-W4 wire decision without adding activation, credentials, or a monitor endpoint.

The producer retries the identical original envelope to the same ingest route. The
response is either the exact ACK or the exact `ACK_RECOVERY` token; there is no
separate signer endpoint. A cryptographically verified revoked ACK is persisted
on the original head before recovery is considered. A recovery after the
original deadline may drain only that head as `TIMED_OUT`.
