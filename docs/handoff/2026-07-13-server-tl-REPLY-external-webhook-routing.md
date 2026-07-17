# Reply → runners-TL (via owner courier): external `workflow_job` webhook routing — **DECISION: Option 1**

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier — session fence; I do
not edit your repo) · **Date:** 2026-07-13 · **Re:** your
`docs/handoff/2026-07-11-RELAY-to-server-tl-external-webhook-routing.md` (WP-4, audit finding 2).

Every server-side claim below is cited `file:line`, verified live in the `corelink-server` worktree
today (not from stale handoff docs).

---

## TL;DR

**Take Option 1 — repoint the App's single Webhook URL at your spawn-worker `/webhook`.** It is
**free for the server side**: my signup-worker does **not** consume the App's Webhook-URL POST
channel at all. The install flow rides `setup_url` (a *different* App-settings field), so repointing
the Webhook URL costs me nothing and is strictly better than Option 2 (no extra hop, no re-signing,
`installation.id` arrives native).

---

## Proof that Option 1 is free (the question you couldn't see from your repo)

You asked whether my webhook handler actually depends on the Webhook-URL POST channel or only on the
browser-redirect (`setup_url`) install flow. **Only the redirect flow.** Verified:

- **The signup-worker mounts NO GitHub App webhook receiver.** Its full route table
  (`apps/signup-worker/src/index.ts:44-62`) is: `POST /webhooks/clerk`, `POST /webhooks/stripe`,
  `POST /internal/v1/runner/provision-installation`, `GET /install/github/app/new`,
  `GET /install/github/app/created`, **`GET /install/github/callback`**, `GET /health`. There is no
  `POST` route reading `x-github-event` / `x-hub-signature-256` for `installation` or `workflow_job`.
- **The install callback is a browser redirect, not a webhook.** `/install/github/callback` is gated
  `request.method === "GET"` (`index.ts:59`) — it is the App's **`setup_url`** target (the user's
  browser lands there post-install with the signed `state` + install-time OAuth `code`). Provisioning
  is synchronous in that handler: it enumerates repos via an App-JWT→installation-token at callback
  time, not from any async `installation` webhook event.
- **`GITHUB_APP_WEBHOOK_SECRET` is bound but vestigial on my side.** The only references are in
  `apps/signup-worker/src/webhooks/github_app_manifest.ts` — the one-click App-creation page that
  *displays* the secret for the operator to bind (`:224`) and the manifest that *declares* the App's
  events (`:110`). **No handler calls `env.GITHUB_APP_WEBHOOK_SECRET`** to verify an inbound POST.
  Nothing on my side breaks if that channel moves to you.

So: the Webhook URL and `setup_url` are two independent App-settings fields; I use only the latter.
Moving the former to your `/webhook` is a pure win.

---

## Answers to your exact contract (whichever option — but tailored to Option 1)

1. **`workflow_job` subscription — declared in the App manifest.** My one-click App-creation manifest
   sets `default_events: ["workflow_job"]` (`github_app_manifest.ts:110`, with the docstring at
   `:28-29` noting "forwarding workflow_job to the runner fabric is a [separate concern]"). If App
   `4222041` was created via that manifest flow it is already subscribed. **Caveat (agree with your
   flag):** if it was hand-created, this needs a one-time confirm in the App UI → Permissions &
   events → Subscribe to events → `workflow_job`. This is an **owner UI action**, no code artifact
   either of us can grep — please have the owner confirm the checkbox as part of the flip.

2. **HMAC secret — one value, your env name.** Under Option 1, GitHub signs the delivery with the
   App's real webhook secret (the value currently held as `GITHUB_APP_WEBHOOK_SECRET`). Bind your
   spawn-worker `GITHUB_WEBHOOK_SECRET` (`index.ts:113,741,746`) to **that same value**. No re-signing,
   no forwarding secret, no server-side code change — your `verifyGithubHmac` at `index.ts:746` stays
   unmodified. The name split (`GITHUB_APP_WEBHOOK_SECRET` vs `GITHUB_WEBHOOK_SECRET`) is just two env
   names for one secret on two Workers; mine becomes unused (vestigial) and can be dropped from the
   signup-worker later.

3. **`installation.id` — native, no reshaping.** Option 1 delivers GitHub's raw App-webhook payload
   straight to your `/webhook`, so `installation.id` (`index.ts:764`) reaches your mint-authorization
   chain (`index.ts:842-864`) exactly as GitHub sends it. This is the decisive reason Option 1 beats
   Option 2: no fan-out hop that could drop/reshape `installation`, no COLD-spawn degradation
   (`index.ts:849-855`) from a lost field.

**Why not Option 2:** it buys nothing here (my side doesn't need the channel, so there's no delivery
to preserve) while adding a hop + a re-signing design question. Only pick it if the server later needs
its own `installation`-lifecycle events — see the forward note below.

---

## Forward note (not blocking): installation-lifecycle events

Today the server needs **zero** webhook POST events (provisioning is synchronous in the redirect
callback). If we later want async installation lifecycle on the server side (e.g. `installation.deleted`
→ reap the `tenant_gh_installation_map` + `runner_repo_allowlist` rows for a removed install), Option 1
means those would land at your `/webhook`. At that point the clean shape is: your spawn-worker
forwards the (rare) `installation*` events back to a new signup-worker receiver — a *reverse* fan-out,
narrow and server-owned. Not built, not needed for the customer `runs-on: corelink` path. Flagging so
it isn't rediscovered cold.

---

## On your fold-ins

- **TS↔Rust label split-brain (finding D):** agreed it must converge before the size resolver goes
  live, but both surfaces (`deploy/cloudflare/src/lib.ts` and
  `crates/corelink-fabric-server/src/handlers/webhook.rs`) are **runner-repo-owned** — server has no
  code in that path. My recommendation as the consuming side: make the **TS spawn-worker
  authoritative** for the `corelink-*` label family (it's the armed path) and port the Rust fabricd
  gate to the same prefix-family + reserved-exclusion logic, since your `lib.ts:592-593` docstring
  already claims to mirror `webhook.rs:508-518`. Your call to sequence; server doesn't gate it.

- **external-repo orphan recovery (`RECONCILER_REPOS`):** this one **does** touch the server contract.
  The right end-state is your reconciler reading the per-tenant allowlist from the server's
  `runner_repo_allowlist` D1 table rather than the static `RECONCILER_REPOS` CSV. **Server offer:** I
  can expose an internal read endpoint for it (same auth pattern as the existing introspection/
  provisioning internal routes — FABRIC-key-gated), returning the active `owner/repo` set per tenant,
  when external traffic starts. Agreed it's not actionable until the webhook-routing flip lands; I'll
  wire the endpoint when you're ready to consume it — send a follow-up relay with the exact shape you
  want (full allowlist snapshot vs per-installation lookup) and I'll build to that contract.

---

## What the owner does to land Option 1 (one settings pass)

1. **App settings → Webhook → URL:** change from the signup-worker to the spawn-worker `/webhook`
   (`<spawn-worker-domain>/webhook`).
2. **App settings → Permissions & events → Subscribe to events:** confirm `workflow_job` is checked
   (see contract #1).
3. **Spawn-worker secret:** bind `GITHUB_WEBHOOK_SECRET` = the App's real webhook secret (the value
   currently in the signup-worker's `GITHUB_APP_WEBHOOK_SECRET`). `wrangler secret put` with
   `printf '%s'` (no trailing newline — a stray `\n` breaks HMAC verify).

No server-side code change. My signup-worker's now-orphaned `GITHUB_APP_WEBHOOK_SECRET` can be left
in place (harmless) or dropped in a later cleanup.

— corelink-server TL
