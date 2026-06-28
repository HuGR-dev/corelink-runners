# Autonomous audit loop — Round 2 (2026-06-28, ~04:30 local)

8 ROTATED/deeper prove-or-break lenses (3 Opus + 5 Sonnet) → adversarial verify. **10 confirmed** (2 high, 2 medium, 6 low), 3 refuted. Raised 13.

## Confirmed + disposition

| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **high** | Cold-path 404 **miss poisons the CAS** — `fetch_layer` returned `Ok(vec![])` (≡ empty layer); hydrate wrote it back + marked cached (false warm). | **FIXED #206** — `fetch_layer → Result<Option<…>>` (None=miss); callers skip write-back. |
| 2 | **high** | `write_layer` treats **PUT-404 as success** (Miss→mark_cached) — silent write-path fail-open. | **FIXED #206** — only 2xx is success; PUT-404 → fail-closed. |
| 3 | **med** | **Northflank provider body excerpt leaks into the 503 wire** (`leases.rs` `box provisioning failed: {e:#}`). | **FIXED (this PR)** — log detail server-side; wire body generic. +regression. |
| 4 | **med** | **Ledger `std::Mutex` held across the blocking `pg_advisory_xact_lock` wait** — cross-instance contention stalls ALL ledger ops process-wide. | **DESIGN-HANDOFF** (`2026-06-28-DESIGN-ledger-lock-across-blocking-advisory.md`) — a sync/async ledger-boundary refactor that risks cap-exactness; NOT an unsupervised 5am change. Owner-review. |
| 5 | low | C3 pool-floor `admit_permits` semaphore doesn't prevent same-instance starvation. | **DESIGN note** (folded into the #4 handoff — same ledger/admission concurrency surface). |
| 6 | low | check-mode `/v1/spawn` doesn't validate `PINNED_IMAGE_DIGEST`. | **NOT-A-BUG (on analysis).** Check-mode runs the deploy-FIXED `CHECK_HOST_CONTAINER` image — the runner's `PINNED_IMAGE_DIGEST` is the wrong pin for it; forcing a match would be semantically wrong. The `@sha256:` format is still enforced (index.ts:369) and the validated memo axis is `toolchain_digest` (required, index.ts:378). No exploitable arbitrary-exec. No change. |
| 7 | low | `claimSpawn` non-atomic (get-then-put) — concurrent webhook redeliveries race. | **ALREADY-DOCUMENTED design limitation** (lib.ts:49-52): KV has no atomic CAS; the race only collapses to the rare exactly-concurrent case (retries are seconds apart) and at worst wastes one spawn (job-level idempotency holds). The architectural fix is a Durable Object claim — out of scope, explicitly documented in-code. No silent debt. |
| 8 | low | No explicit body-size cap on `/v1/leases` + `/exec` (axum 2 MiB default). | **FIXED (this PR)** — explicit `DefaultBodyLimit::max(256 KiB)`. +regression (413). |
| 9 | low | Malformed bodies → 422, not the frozen 400. | **NOT-A-CLEAR-DEFECT (on analysis).** The frozen `ApiError::Invalid` (400) is explicitly scoped to "*structurally valid JSON* but semantically rejected" (`error.rs`) — it does NOT govern a *malformed* body. axum's `422 Unprocessable Entity` for a wrong-shape body is HTTP-defensible; the frozen vocabulary covers the fabric's SEMANTIC errors, not transport-extraction rejections. A uniform `ErrorBody` shape for extraction errors (a `ValidatedJson` extractor) is an OPTIONAL client-DX polish, not a contract violation — left as a deliberate design position, not silent debt. |
| 10 | low | `HttpBootCas` doc claimed `fetch_layer` returns `LayerUnavailable` on 404 (≠ code). | **FIXED #206** — doc realigned to Option/None. |

## Refuted (verifier killed — sound)
- attestation v2 timing-metadata omission (intentional); + 2 others proven clean.

## Status
- **#206** (2 high CAS) merged.
- **This PR** (info-leak medium + body-cap low). Gate green.
- **OPEN before 10:00:** #4 ledger-lock (design-handoff, owner-review) · #6/#7 cf-worker lows (TS batch) · #9 422→400 (custom extractor). These are SEQUENCED for later batches/rounds in this loop window, not dropped.
