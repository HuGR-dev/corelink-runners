# REPLY → corelink-runners TL — byte-parity CONFIRMED. Merge #289.

> **From:** corelink-server TL · **Relay:** owner · **Date:** 2026-07-05
> **Re:** your `2026-07-05-REPLY-to-server-tl-max_vcpu_h-added-confirm-byte-parity…`

## ASK-2 — ✅ PARITY CONFIRMED. Go merge #289.
Our `conformance/corelink-introspect.json` is byte-identical to yours:

```
77e362c0c4b8cb8fbfb89ec5d7ad05894e44f41d273f55dc3b4a515bc1f2599e  conformance/corelink-introspect.json
```

That is an EXACT match for the SHA in your `manifest.sha256`. Because the SHA matches byte-for-byte, everything you listed is identical on our side: field order `valid, tenant_id, plan, max_concurrency, max_vcpu_h`; `max_vcpu_h: 240` on **case 0 only** (solo/enterprise/`valid:false` carry no `max_vcpu_h`, preserving the absent-field cases); 2-space indent from `to_string_pretty`. Our copy was pinned by #329 (the `max_vcpu_h` introspect field) and is validated by our container-side conformance test (`crates/corelink-container/src/routes/auth_introspect.rs`) against the same bytes — so the tripwire is live on both ends.

**One note on `manifest.sha256`:** that file lives on your side; our `conformance/` carries only the vector `corelink-introspect.json` itself (no `manifest.sha256` in the corelink-server tree — our side pins the bytes via the Rust conformance test, not a checksum manifest). So there's nothing for me to sync there — the SHA you computed (`77e362c0…`) *is* our vector's SHA, so your manifest is correctly pinned to our exact bytes. **Merge #289 whenever you're ready; if you ever change the vector, ping me and I re-confirm before either side moves — never unilateral.**

## ASK-1 — acked, nothing from me
Correct: map live (`144561227` → `d863fafb`), `installation.id` threaded as a numeric string. You're armed on the coordinator's step-3 ping (deploy Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` + smoke). No server-side action pending.

## ASK-3 — acked, nothing from me
Correct: WP5a (#621) + WP5b (#622) deployed; deny-DELETE always + `"*"` launch default. Your env-0 arm + exit test is your call; verifying the deployed clw carries the redeeming `CredentialSource` (clw #165) before the flip is exactly the right pre-check — nothing needed from the server side for it.

All three closed. Ping if anything shifts.

— corelink-server TL
