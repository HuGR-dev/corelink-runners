# ADR-0013 — Runner tenant ownership on combined credentials

- Status: implementation decision under delegated owner authority; live proof pending
- Date: 2026-09-06
- Decision author: Codex root, repository tech lead
- Authorization: repository owner instructed, “voce tem autonomia pra decidir”.
- Scope: D13 / T4-W1, runner authorization, mint and immutable billing attribution

When a GitHub installation will be supplied, its server-side installation-to-tenant
mapping will be the owner of record. A configured acquiring PAT will be an additional
credential; it cannot replace or hide that installation. If both identities will be
present, the server must derive both and require the same tenant before minting.
The Worker sends the installation in both authorize and mint requests even when
it also sends an acquiring PAT.

| Installation | Acquiring PAT | Outcome |
| --- | --- | --- |
| Valid mapped tenant | Absent, no configured repository PAT | Authorize that mapped tenant through all existing entitlement and repository gates. |
| Valid mapped tenant | Valid PAT for the same tenant | Authorize the same tenant through all existing gates. |
| Valid mapped tenant | Invalid PAT or a different tenant | HTTP 403, code `FORBIDDEN`, message `runner mint unauthorized`; no mint. |
| Missing or invalid mapping for a supplied installation | Any | The same generic HTTP 403; no PAT fallback or mint. |
| No installation (native caller) | Valid acquiring PAT | Existing PAT-derived tenant authorization; all existing gates apply. |
| No installation | No acquiring PAT | Existing HTTP 401; no mint. |
| Repository explicitly maps a PAT secret, but that secret will be missing/blank | Unavailable | Worker preparation refuses before authorization, slot, mint, claim or provider effects; no installation fallback. |

The server never accepts a tenant identifier from the request body. The tenant
returned by authorization must equal mint's tenant and immutable attribution;
capacity and compute metadata must also match. A changed authorization refuses
the attempt and cleans up its exact issued credential.

This decision tightens conflicting identity handling. It does not waive any
acceptance gate, authorize a live rollout, or claim a human cryptographic
signature. Deployment requires compatible server and Worker versions, explicit
installation mappings for repository PAT configurations, and the sprint gates.
