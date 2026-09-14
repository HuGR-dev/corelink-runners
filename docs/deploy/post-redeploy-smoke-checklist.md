# Post-redeploy smoke checklist

> Run after a Northflank **NEW BUILD** of `main` (NOT a restart — see
> `northflank-postgres-runbook.md §1`). Confirms the build is the new code, the
> P0 attestation fix + hardening are live, and nothing regressed. ~5 minutes.
> `H=https://<service-host>` · `PAT=<bootstrap tenant PAT>` (the live `FABRIC_PAT`).

## 0. The build actually shipped (not a restart)
- [ ] Boot log shows the expected ledger line: `ledger backend: Postgres
      (persistent, multi-instance cap-safe; pool=8; tls=…)`.
- [ ] Boot log: reaper started, the §13.5 sweeps, and — if `FABRIC_ADMISSION_MODE`
      is set — the admission mode. A restart cannot show new boot lines added since
      the last *build*; if these are absent, you hit Restart, not NEW BUILD.

## 1. Liveness
- [ ] `curl $H/v1/health` → `ok`.

## 2. Acquire → attest → the P0 fix is live (the headline)
- [ ] Acquire a lease (`POST /v1/leases` with the bootstrap PAT, a sha256-pinned
      image, a short ttl). Expect `200` + a `lease-<uuid>` id (UUID minting, #39 —
      NOT `lease-<n>`; a sequential id means the OLD binary is running).
- [ ] Close it with a `CheckResult` (`POST /v1/leases/{id}/close`). In the
      `CloseResponse`, confirm **both** `result_binding_sig` (v1) **and**
      `result_binding_sig_v2` are present. **`result_binding_sig_v2` present = the
      P0 fix is live.** (Absent v2 → old binary; redeploy did not take.)
- [ ] Close with a `CheckResult` whose `memo_key` does NOT match
      `SHA-256(LP(tree)‖LP(def)‖LP(toolchain))` of its own axes → expect a
      `400`/fail-closed (the memo_key-validation gate, W2-C). An honest result → `200`.

## 3. Fail-closed posture (spot-checks)
- [ ] Acquire with an UNpinned image → `400` before any box contact (API2).
- [ ] A request with a bad/absent PAT → `401`/`403`, never admitted.
- [ ] (If a tenant cap is set) acquire past the cap → over-cap reject (or, under
      `FABRIC_ADMISSION_MODE=queue`, queued — confirm which mode you deployed).

## 4. Persistence + multi-instance (if instances ≥ 2)
- [ ] A held lease survives a single-instance restart (the Postgres ledger, not
      memory).
- [ ] Fire more concurrent acquires than the cap across both instances → total
      admitted == the cap exactly (advisory-lock serialized; per-instance caps
      would over-admit). The `instances=2` cap-safety proof.

## 5. No secret leakage
- [ ] Grep the boot + request logs for the Postgres password / PAT / signing key /
      Northflank token / `FABRIC_INTROSPECT_AUTH_KEY` → none present (the redaction
      hardening: `ServerConfig`/`HttpRequest`/`BearerPat` Debug all redacted;
      `DATABASE_URL` never printed).

## 6. Housekeeping after the smoke test
- [ ] Any test leases you acquired will expire on their ttl (the durable deadline
      reaper now reclaims them cross-instance) — or `TRUNCATE leases;` for a clean
      state on a non-serving deployment.
- [ ] 🔒 Rotate any secret that appeared in a chat/log/screenshot during the deploy
      (Northflank token, `FABRIC_SIGNING_KEY`, `FABRIC_PAT`, the Postgres password).

---

**If §2's `result_binding_sig_v2` is absent**, the redeploy did not ship the P0
fix — re-trigger a **NEW BUILD** (Builds → New build → main), not a restart.
