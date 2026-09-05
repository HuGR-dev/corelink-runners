# Sprint 4 action preparation — 2026-09-05

Status: reviewed and focused-tested T5-W3 preparation, **not a completed WP or
an independently mergeable sprint**. T5-W2/D7/D3 integration predecessors remain
open. Full CI belongs to the later composed sprint tip, not to each author PR.
The validated Sprint1/#558 source remains unchanged at `bb165bad`.

## Implemented

- AU5.11: all action inputs cross step environment mappings, never script
  interpolation; quoted argv, no PAT printing, child control-file environment
  stripping, unique temporary files and bounded always-cleanup.
- AU5.12: the Python runtime dependency is replaced by the existing immutable
  github-script pin `3a2844b7e9c422d3c10d287c895573f7108da1b3` (Node24).
  Strict JSON/object/string/i32/boolean validation, generic errors, preserved
  public outputs and distinction between raw CLI exit1 and check exit137.
- Independent review approved author source
  `840ff9cd40bfff2cfd13ca011663d17e8cb5b1f7`, tree
  `32fb19756a7dba27f44de3e654bf87ba863be62e`. Root composition `d362742d`
  preserves its exact action and validator blobs atop the validated Sprint1 tip.

## Focused evidence

Eleven validator groups pass, including14 malicious-input cells, actual YAML
environment/output routing, execution markers, failure/verification matrices,
cleanup sentinels, invalid JSON and the combined raw1/check137 case. Local core
API shims are explicitly unit preparation evidence, not the runtime proof below.

The complete composite action additionally passed in local `act`0.2.89, using
the **real pinned github-script distribution**, not a core API shim. Its same-job
preflight requires Bash and Node24.20.0 and rejects an available `python3`.
The final job verifies exactly `exit=0`, `verified=true`,
`lease-id=lease-au5-12`, using a deterministic, network-free stub CLI.

- Node base: `node@sha256:ba849c60be29959425b8734d57b8b4b7d56f98edd9504c9af091d5281095a71e`.
- Execution image ID: `sha256:a2b53118ac298a872bf9b1643100eb0e0a981365c0ab16c8c7f20939a180736c`.
- Source/executed action SHA256: `2ce6bcfc6d330049a572deb7a6ea926b8048c2dcb60374c099f8f81a75d93026`.
- Persisted final execution log SHA256: `501522aa8f3299c03629c658d0cf19cefab7afa8843a5a019dd581773e6eb10b`.
- Executed workflow SHA256: `54bf818387c34cba978cb29f9b594b147832d7bc37fb50d64b85bd2544f2badc`.

Host artifacts: `/private/tmp/corelink-act-proof.x0IfKC/`. Root independently
read and hashed the raw log, workflow, action copy and image provenance before
the dedicated proof VM was stopped. An earlier transient log was lost on VM
shutdown and is **not** used as auditable evidence; the persisted rerun above is.

This is a real local container/composite execution, not a hosted GitHub job,
published-release validation, customer onboarding, production deployment or a
full Sprint4 CI certificate. No real PAT/GitHub token, live fabric call or runner
registration was used. User PostgreSQL and other VMs were untouched.
