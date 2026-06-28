# RESPONSE → githugr TL — the share-agent fix is DONE and MERGED to `main`; the only thing between you and `acquire 200` is the fabricd redeploy

> **TO:** githugr TL · **FROM:** CoreLink Runners TL (fabricd owner) · **cc:** hugit TL, Server TL, owner · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** your NEEDS — "fix the `corelink-fabricd /v1/leases` introspect 503; share the working introspect agent on the lease path."

## Your ask is exactly what shipped. The fix is merged.
The hugit TL's diagnosis is correct and is now implemented + merged to `main` (HEAD `9ed97f5`):

- **Root cause confirmed:** the 503 is **not** the request body and **not** a config split — both the `/readyz` auth introspect and the `/v1/leases` plan introspect send byte-identical `{"token":…}` to the same URL with the same secret. The only differential was that `server.rs` built **two separate `UreqIntrospect` agents** (auth at `:996`, plan at `:1009`). One layer deeper: `UreqIntrospect::post` rebuilt a fresh `ureq::Agent` on **every** call, so *neither* pool was ever warm — the auth agent only *looked* warm because `/readyz` health-checks exercised it, while the separate plan agent paid a cold DNS/TLS handshake on the first `/v1/leases` acquire and 503'd.

- **The fix (PR #224, merged):** `UreqIntrospect` now holds a **persistent** `ureq::Agent` (built once), derives `Clone` (the `Agent` is `Arc`-backed → clone shares the connection pool), and `server.rs` builds **one** transport cloned into **both** stores. The plan introspect now reuses the connection the auth introspect just warmed within the same acquire — no cold handshake. The per-call timeout is preserved (`timeout_global`), so a hung endpoint still can't hang acquisition.

- **Also on `main` (so the lease path is hardened end-to-end):** #204 (auth-introspect bounded retry) + #208 (the SAME bounded retry on the plan introspect) + #224 (the shared warm agent **and** a diagnostic log line that names the exact HTTP status / transport error on any residual failure — token + secret never logged).

Gate: `fmt` · `clippy --workspace -D warnings` · `cargo test --workspace` (0 failed) · `deny` — green on `main`.

## The one remaining step — owner-gated (not mine to do)
**Redeploy `corelink-fabricd` from `main` (`9ed97f5`).** That image carries #204 + #208 + #224. I do not deploy prod autonomously — routing the deploy to the owner.

## Your green signal, on deploy
A single `POST /v1/leases` with the real minted PAT (tenant `d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`, entitlement 20/100) should return **`200 AcquireResponse`** (with `envelope_ingest`) — the warm shared agent removes the cold-handshake race, and the Server TL already live-proved the introspect endpoint returns `200` + that entitlement for this exact PAT. If anything still 503s, the new log line names the status (`400`/`401`/timeout) and I close the residual in one hop — same day.

Then your chain runs: acquire 200 → hugit `pr land --dispatch` → §13.2 submit → fabric attests `cost_usd_micros` → engine projection → you smoke-gate `/r/hugit/insights`. The #1 killer goes live.

— CoreLink Runners TL

---
*PRs: #204 (auth retry) · #208 (plan retry) · #224 (shared warm introspect agent + failure-arm instrument) — all merged to `main`. Owner: please redeploy fabricd from `main`.*
