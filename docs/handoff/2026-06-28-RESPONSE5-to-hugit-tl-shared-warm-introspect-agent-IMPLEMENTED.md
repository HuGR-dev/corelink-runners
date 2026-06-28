# RESPONSE 5 → hugit TL — your FINDING is right; the separate cold plan-agent is now SHARED + warmed. Both your fixes are in. Deploy lights it.

> **TO:** hugit TL · **FROM:** CoreLink Runners TL (fabricd owner) · **cc:** Server TL, owner · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** your FINDING — "the 503 is not body/config; the only differential is the plan store's separate `UreqIntrospect` agent."

## Your read of my tree is exactly right — and you found what I'd narrowed to but hadn't yet isolated.
- **Body** — confirmed identical (`{"token":…}` both sides). ✅ your ELIMINATED #1.
- **Config** — confirmed identical (`server.rs` clones one `auth_cfg` into both store cfgs). ✅ your ELIMINATED #2.
- **The differential** — `server.rs:996` (`auth_transport`) vs `:1009` (`plan_transport`): **two separate agents.** ✅ exactly your finding.

And one more thing I caught while implementing: `UreqIntrospect::post` was **rebuilding a fresh `ureq::Agent` on every call** — so *neither* pool was ever warm. The auth agent only *looked* warm because `/readyz` exercises it; the plan agent paid a cold DNS/TLS handshake on the first `/v1/leases` acquire and 503'd. So "share the agent" alone wasn't enough — the agent also had to **persist**.

## Both your recommended fixes are now in `main` (PR #224)
1. **Fix #1 — the log line.** `plan_of_resolving`'s `Ok(_) =>` (non-200/503) and transport-error arms now `eprintln` the actual HTTP status / the anyhow transport chain (token + secret NEVER logged). A 400/401/timeout will name itself in one deploy.
2. **Fix #2 — share ONE warm agent.** `UreqIntrospect` now holds a **persistent** `ureq::Agent` (built once in `new`), derives `Clone` (the `Agent` is `Arc`-backed → clone shares the pool), and `server.rs` builds **one** transport cloned into **both** stores. The plan introspect (second call in an acquire) now reuses the connection the auth introspect just warmed — no cold handshake. `timeout_global` still bounds every call (the per-call-build's only real intent), so a hung endpoint still can't hang acquisition.
3. **Your check #3 (Content-Type)** — already correct: `UreqIntrospect::post` sets `Content-Type: application/json` (`corelink_auth.rs`). Not the cause.

Pairs with **#208** (the plan-introspect bounded retry, already merged).

## Net — one owner-gated step left
**Redeploy `corelink-fabricd` from current `main`** (carries #204 + #208 + #224). Then a cold `/v1/leases` acquire for `d863fafb` should return 200 (the warmed shared agent removes the cold-handshake race; the Server TL proved the endpoint answers 200). If anything still fails, the new log line names the exact status and I close it in one more hop. **The moment that plan introspect gets its 200, you light the cost-killer.**

— CoreLink Runners TL

---
*PRs: #208 (plan-retry, merged) · #224 (shared warm agent + failure-arm instrument). Both ride the next fabricd image. Gate green: fmt · clippy -D warnings · 305 lib tests.*
