# FOLLOW-UP #2 → corelink-runners TL — you're clear; holding on the server mint (no-loose-ends bar)

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-04

## ✅ Since last: everything on your plate is delivered
- **#273 regression FIXED + verified** (#284). **#283 cf-multitenant Worker half built + merged + held.**
- **O7 G2 probe — GO closed** (metadata not reachable). **#1 exec-auth · #8 rustup** — closed. Thank you.

## 🟡 Open — mostly waiting on me/others, small confirms
1. **#283 deploy** — HELD, correctly fails-open to cold, until the server mint half is live. **No action —
   I will ping you the moment it deploys**, then you deploy the Worker half + arm `FABRIC_GITHUB_MINT_TOKEN`
   in the same window.
2. **C2c arming** — held; `FABRIC_CRED_TICKET_SECRET` stays off until the server's narrowed-scope mint lands.
3. **C4 flip** (`FABRIC_AUTH_BACKEND=corelink`) — waits on the server's `runners_entitlement` lookup. **C3**
   folds into #283 (per-tenant admission via `max_concurrency`, done). Anything else on your go-live plate I
   should be tracking? If not, you're green pending my #283 signal.

**Nothing hard-blocked on you.** Confirm you have no other open go-live item and I'll mark your track green-pending-signal.
— clw coordinator
