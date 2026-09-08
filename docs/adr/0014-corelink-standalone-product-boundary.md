# ADR-0014 — CoreLink operates independently of Hugit

**Status:** accepted  
**Date:** 2026-09-08  
**Owner:** CoreLink product and techlead

## Decision

CoreLink Runners is operated and qualified as a standalone product. Hugit and
githugr are separate projects and are not CoreLink consumers, dependencies,
authorities, deployment environments, acceptance oracles, or go-live gates.

The direct CoreLink surfaces are the public Runner API, the `corelink` CLI,
the CoreLink SDK/conformance clients, the GitHub App path, and the Workspaces
integration where explicitly included by its own contract. Memoized checks,
attestation, usage, and agent execution are CoreLink-owned capabilities. A
future external customer or integration may consume them through those public
contracts, but no named external project is required to promote CoreLink.

## Consequences

- Active plans and acceptance criteria must name CoreLink API/CLI/SDK or a
  generic external customer, never Hugit/githugr ownership or dispatch.
- A missing Hugit manifest, replica inventory, PAT, verifier, dispatch, or
  approval cannot block a CoreLink release or secret rotation.
- Historical handoffs, evidence, changelogs, and frozen wire provenance retain
  their original names and dates. They are historical records and do not create
  current obligations.
- Any legacy Hugit-specific environment or local path in an active operator
  workflow is replaced by the CoreLink equivalent before that workflow is
  integrated.

## Promotion rule

The go-live reviewer checks the CoreLink deployment manifest, active Cloudflare
instances, direct API/CLI/SDK evidence, and applicable customer-facing
contracts. No external-project evidence is accepted as a substitute for those
artifacts.
