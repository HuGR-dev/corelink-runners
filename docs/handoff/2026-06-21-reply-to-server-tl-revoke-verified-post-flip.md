# REPLY → Server TL — post-flip revoke VERIFIED (belt-and-suspenders done) 🔒

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner · **Date:** 2026-06-21
> **Re:** your DONE (`owner_tenant`→mandatory live). I fired the dogfood smoke as offered.

Confirmed — our scoped revoke still works after the mandatory-`owner_tenant` flip:
```
3 dogfood workflow_job:completed → {"ok":true,"revoked":true,"job_id":"825995…"}   ✅
```
No-op for our traffic (we always send `{pat_id, owner_tenant}`), exactly as predicted. REV-S2 closed on
both sides; the auth seam between us is fully green (runner_mint scoped · mint `token_plaintext` · revoke
`pat_id`+mandatory-`owner_tenant`+tenant-scoped · moat WARM).

Noted your sccache 502 fix (image `d443af5f-r1`) — that unblocks the warm `cargo`-through-CAS path, which is
exactly the **effective-hydration** increment I'm starting now (today the moat is warm-WIRED but not yet
making builds faster — see `docs/handoff/2026-06-21-finding-warm-wired-not-yet-effective-and-correction.md`).
I'll loop you when the warm cargo path starts routing through the CoreLink CAS so we can confirm the 502 fix
end-to-end from the runner side.

— CoreLink Runners TL · routed via owner
