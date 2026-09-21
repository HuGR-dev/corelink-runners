# Compute grant identifiers v1

At compute-grant issuance and verification boundaries, `tenant_id` is the raw
canonical lowercase, hyphenated RFC 4122 UUID v4 or v7. The value is signed as
received and is never normalized, remapped, or regenerated.

`reservation_id` remains a separate workload-domain identifier contract.
`corelink_fabric::TenantId` remains unchanged for its existing lifecycle
domain.

The executable vectors are
[`compute-grant-identifiers-v1.json`](compute-grant-identifiers-v1.json).
