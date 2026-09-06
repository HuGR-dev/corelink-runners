// corelink-fabricd on Cloudflare Containers — the proxy Worker (gap-#1 option b).
//
// The Rust control plane runs as ONE long-lived singleton container; this Worker
// routes EVERY request to it and keeps it warm via a cron ping. The control plane
// holds the lease ledger in memory, so all /v1 traffic MUST reach the same
// instance — enforced by a fixed DO id (SINGLETON) + max_instances:1 (wrangler).
//
// DEPLOY-VERIFIED (2026-07-09): live at https://corelink-fabricd.gmhelmold.workers.dev
// (/health 200), running the pinned container image; the envVars-injection +
// auto-start lifecycle mirror the proven corelink-spawn-worker pattern.

import { DurableObject } from "cloudflare:workers";
import { Container, getContainer } from "@cloudflare/containers";
import type { StopParams } from "@cloudflare/containers";
import { shardOf } from "./shard";

export interface Env {
  FABRICD: DurableObjectNamespace<FabricdContainer>;
  // Shard count (multi-instance fabricd, option 3). String in wrangler vars,
  // parsed to int; absent/invalid ⇒ 1 (inert singleton). MUST be raised in
  // lockstep with `max_instances` in wrangler.jsonc — see the comment there.
  FABRIC_NUM_SHARDS?: string;
  // Non-secret vars (wrangler.jsonc).
  CORELINK_INTROSPECT_URL: string;
  BILLING_INGEST_URL?: string;
  BILLING_REGION?: string;
  CLOUDFLARE_SPAWN_WORKER_URL?: string;
  // fabricd's OWN public base URL. REQUIRED when the moat mint is armed: the C2c
  // cred ticket delivers no CLW_TOKEN, so the box redeems its ticket at
  // {FABRIC_PUBLIC_BASE_URL}/v1/leases/{id}/cas-cred to obtain the per-job PAT.
  // The Rust boot guard (validate_mint_arm) fails closed if the mint is armed and
  // this is unset — the exact silent-when-armed gap the go-live audit caught.
  FABRIC_PUBLIC_BASE_URL?: string;
  // Secrets (`wrangler secret put`).
  FABRIC_SIGNING_KEY: string;
  FABRIC_INTROSPECT_AUTH_KEY: string;
  BILLING_INGEST_AUTH_KEY?: string;
  CLOUDFLARE_SPAWN_AUTH_TOKEN?: string;
  // Scoped control credentials for the fabric's Cloudflare engine. Each is
  // forwarded independently into the container; the Rust engine refuses a
  // partial or shared-token configuration before it can send requests.
  CLOUDFLARE_EXEC_AUTH_TOKEN?: string;
  CLOUDFLARE_LIFECYCLE_AUTH_TOKEN?: string;
  // A GitHub PAT with repo Administration:write — lets fabricd's PAT broker mint
  // JIT runner configs WITHOUT a GitHub App (the mechanism the autoscaler uses).
  // Absent ⇒ fabricd falls back to the FABRIC_GITHUB_APP_* App path. When set it
  // is PREFERRED. Worker secret (`wrangler secret put`).
  FABRIC_GITHUB_MINT_TOKEN?: string;
  // Runner-broker GitHub App (ADR-0007 Stage A) — the FALLBACK to the PAT above
  // when FABRIC_GITHUB_MINT_TOKEN is absent. `runner_broker.rs` reads this trio
  // (env::{APP_ID, INSTALLATION_ID, PRIVATE_KEY_B64}). Forwarded into the
  // container below; all-absent ⇒ App path not wired (a `runner:` acquire without
  // either mechanism is rejected, byte-identical to today). Secrets.
  FABRIC_GITHUB_APP_ID?: string;
  FABRIC_GITHUB_APP_INSTALLATION_ID?: string;
  // Base64 of the App's PEM private key (the *_B64 form survives env-var UIs that
  // mangle multi-line PEM — runner_broker.rs prefers it over the raw PEM form).
  FABRIC_GITHUB_APP_PRIVATE_KEY_B64?: string;
  // Durable ledger (R1 — arms the vCPU ceiling). When DATABASE_URL is present the
  // container runs the PgLedger (durable, survives DO restart) instead of in-memory,
  // and the vCPU-hour ceiling (FABRIC_RUNNER_VCPU) can arm — the #265 boot guard
  // fail-closes an armed ceiling on a non-pg backend, so the two are wired together.
  // Absent ⇒ in-memory ledger, no ceiling (unchanged dogfood behaviour). Secret.
  DATABASE_URL?: string;
  // Fail-closed arm for the durable ledger, WITHOUT deleting the secret. Only the
  // exact string "0" permits DATABASE_URL to reach the container; unset, blank,
  // whitespace, "1", and every malformed value behave as if DATABASE_URL were
  // absent, so the container boots on the in-memory ledger and never dials PG.
  //
  // It exists because `PgLedger::connect` fail-closes BEFORE `TcpListener::bind`:
  // when the database refuses connections the control plane cannot start, the
  // keep-warm cron retries every minute, and every retry is another connection
  // attempt against a database that is already refusing. On a scale-to-zero
  // provider that is worse than useless — a database woken every 60 s never
  // autosuspends, so the crash loop itself consumes the compute allowance whose
  // exhaustion caused the refusal in the first place.
  //
  // Deleting the secret would also work, but that is a credential this session
  // cannot restore. A var is reversible by anyone, from the config, in one line.
  FABRIC_PG_DISABLED?: string;
  // Opt-in pg TLS: `disable` (default) | `require`. A public-internet managed PG
  // should set `require`; when DATABASE_URL is set we default it to `require`.
  FABRIC_PG_TLS?: string;
  // ── Moat mint (env-0 C2c) — arms the per-job CAS PAT mint. The fabricd's
  // `cas_pat_mint_from_env` + `validate_mint_arm` require the mint URL+key, the
  // cred-ticket secret, and the CLW endpoint to be armed TOGETHER (else boot
  // fails loud); all-absent ⇒ moat OFF (cold path). These MUST be forwarded into
  // the container's envVars below — the wrangler `vars`/secrets are visible to the
  // Worker as `env.*` but the CONTAINER only sees what `this.envVars` sets. ──
  CLW_ENDPOINT?: string; // var — CoreLink API base for CLW cache hydration
  CORELINK_RUNNER_MINT_URL?: string; // var — mint base ({url}/internal/v1/runner/{mint,revoke})
  CORELINK_RUNNER_MINT_AUTH_KEY?: string; // secret — x-corelink-internal-auth for the mint
  // Inc-3 (2026-08-19): CF Access service token for /internal/v1/* (mint/revoke/
  // introspect/billing) now that corelink-server gates them behind Cloudflare
  // Access. Both must be forwarded into the container envVars below (the crate's
  // `cf_access` reads them). No-op until both are set.
  CORELINK_CF_ACCESS_CLIENT_ID?: string; // secret — CF Access service-token client id
  CORELINK_CF_ACCESS_CLIENT_SECRET?: string; // secret — CF Access service-token client secret
  FABRIC_CRED_TICKET_SECRET?: string; // secret — env-0 cred-ticket HMAC (PAT never in untrusted env)
  // Attested-cost emission (FLIP-B): "true"/"1" ⇒ `intent_metrics_sig` on CloseResponse. Var.
  FABRIC_EMIT_INTENT_METRICS_SIG?: string;
  // Resilience knobs the Rust binary already reads but that were UNSET/unforwarded
  // (so crash-surfacing + durable billing export were OFF). Forwarded into the
  // container below. Container reads env at boot ⇒ a rollout is needed to apply.
  FABRIC_CRASH_PROBE_INTERVAL_SECS?: string;
  FABRIC_BILLING_EXPORT_INTERVAL_SECS?: string;
  // ── ACQUIRE-PATH RESILIENCE (W0/W1) — inbound + introspect backpressure ──────
  // Global inbound in-flight cap: right-sized for the 2-vCPU singleton so a herd
  // sheds 503 cleanly instead of browning out. Forwarded into the container (the
  // set-but-unforwarded trap — a wrangler var alone never reaches the container).
  FABRIC_MAX_INFLIGHT_REQUESTS?: string;
  // Introspect admission cap (W1): bounds concurrent auth+plan introspect
  // round-trips so an acquire burst sheds before starving the blocking pool. The
  // Rust default (32) is box-appropriate; passthrough here so it stays tunable.
  FABRIC_INTROSPECT_MAX_INFLIGHT?: string;
  // Enforcement / observability / safety (optional passthroughs; inert until set)
  FABRIC_ADMIN_KEY?: string;
  FABRIC_OBSERVABILITY_KEY?: string;
  // DEV/TEST-ONLY out-of-band cred-ticket mint (POST /v1/test/mint-cred-ticket).
  // OFF by default: absent ⇒ the route 404s (inert). Arms a SENSITIVE mint surface,
  // so set it manually (`wrangler secret put`) only for a dev/test flip, never in a
  // steady prod deploy. FABRIC_TEST_MINT_TENANTS is an optional comma-separated
  // allowlist (default f0005). Both forwarded into the container below.
  FABRIC_TEST_MINT_KEY?: string;
  FABRIC_TEST_MINT_TENANTS?: string;
  FABRIC_RUNNER_REPO_ALLOWLIST?: string;
  // ADR-0007 Stage-B autoscaler (default-off; the route only mounts when
  // FABRIC_AUTOSCALER_WEBHOOK_SECRET is set — inert until then)
  FABRIC_AUTOSCALER_WEBHOOK_SECRET?: string;
  FABRIC_AUTOSCALER_PAT?: string;
  FABRIC_AUTOSCALER_RUNNER_IMAGE?: string;
  FABRIC_AUTOSCALER_LABELS?: string;
  FABRIC_AUTOSCALER_TMP_ROOT?: string;
  FABRIC_AUTOSCALER_EXPIRY_MS?: string;
  FABRIC_AUTOSCALER_REPO_ALLOWLIST?: string;
  FABRIC_AUTOSCALER_MAX_TRACKED_JOBS?: string;
  // Boot-guard override honoured by the fabricd binary: `warn` downgrades the
  // introspect boot self-check from FATAL to log-only. Forwarded into the
  // container (see the constructor) — it was previously documented but
  // unreachable, which made a rejected introspect key unrecoverable.
  FABRIC_INTROSPECT_BOOTCHECK?: string;
}

/**
 * Container env for the durable PG ledger.
 *
 * The operational containment binding is an explicit arm, despite its historical
 * `*_DISABLED` name: PG is reachable only when it is byte-for-byte `"0"`.
 * Keeping this decision pure makes the complete fail-closed matrix testable.
 */
export function pgLedgerEnvVars(
  env: Pick<
    Env,
    | "DATABASE_URL"
    | "FABRIC_PG_DISABLED"
    | "FABRIC_PG_TLS"
    | "FABRIC_BILLING_EXPORT_INTERVAL_SECS"
  >,
): Record<string, string> {
  if (!env.DATABASE_URL || env.FABRIC_PG_DISABLED !== "0") return {};

  return {
    FABRIC_LEDGER_BACKEND: "pg",
    DATABASE_URL: env.DATABASE_URL,
    FABRIC_PG_TLS: env.FABRIC_PG_TLS ?? "require",
    FABRIC_RUNNER_VCPU: "4",
    // Durable billing EXPORT (WP-A) is pg-only and therefore shares this arm.
    ...(env.FABRIC_BILLING_EXPORT_INTERVAL_SECS
      ? { FABRIC_BILLING_EXPORT_INTERVAL_SECS: env.FABRIC_BILLING_EXPORT_INTERVAL_SECS }
      : {}),
  };
}

/** The singleton control-plane container. fabricd binds 0.0.0.0:8080. */
export class FabricdContainer extends Container<Env> {
  defaultPort = 8080;
  // Zero-idle-cost: sleep 5m after the last REAL request. The scheduled() cron no
  // longer force-keeps it warm — it reads a container-free activity marker first
  // (see fetch() override + the idle gate in scheduled()) and skips the health
  // probe once a shard is idle, so an idle fabricd actually sleeps and stops
  // billing memory. Lease state is pg-durable (DATABASE_URL), so sleeping loses
  // nothing; a new acquire wakes the container (~2-3s, hidden behind a minutes-long
  // CI job). While leases are active the box keeps calling in, so it stays warm.
  sleepAfter = "5m";
  // fabricd dials OUT to CoreLink introspect + billing ingest (+ the spawn-Worker
  // once boxes are wired); it needs egress.
  enableInternet = true;

  constructor(ctx: DurableObject<Env>["ctx"], env: Env) {
    super(ctx, env);
    // Inject the fabricd env at container start. corelink auth backend +
    // loopback-free bind; secrets flow from Worker secrets → the container
    // process. Optional keys are omitted when unset (billing/boxes added later).
    this.envVars = {
      FABRIC_AUTH_BACKEND: "corelink",
      FABRIC_BIND_ADDR: "0.0.0.0:8080",
      // REQUIRED at boot (no default) — the bootstrap tenant's cap. In corelink
      // backend mode the live per-tenant cap comes from introspect; this is the
      // static fallback, set deliberately high so it never masks the real cap.
      FABRIC_TENANT_MAX_CONCURRENCY: "100",
      // Multi-instance shard COUNT — forwarded so the container learns it at boot
      // (the cap-safety guard is then authoritative before the first shard header,
      // closing the header-less-acquire over-admit window at N>1). The proxy above
      // routes by this SAME value. Absent ⇒ container defaults to 1 (inert).
      ...(env.FABRIC_NUM_SHARDS ? { FABRIC_NUM_SHARDS: env.FABRIC_NUM_SHARDS } : {}),
      CORELINK_INTROSPECT_URL: env.CORELINK_INTROSPECT_URL,
      FABRIC_INTROSPECT_AUTH_KEY: env.FABRIC_INTROSPECT_AUTH_KEY,
      FABRIC_SIGNING_KEY: env.FABRIC_SIGNING_KEY,
      // Inc-3: CF Access service-token for /internal/v1/* (both must be present
      // for the crate's cf_access to emit the headers; no-op until bound).
      // Inc-3 CF Access service token for outbound /internal/v1/* (mint, revoke,
      // introspect, billing). Restored 2026-08-31 after the outage bisect cleared
      // it: withholding these two changed nothing, and the real cause was the
      // base64 App PEM below.
      ...(env.CORELINK_CF_ACCESS_CLIENT_ID
        ? { CORELINK_CF_ACCESS_CLIENT_ID: env.CORELINK_CF_ACCESS_CLIENT_ID }
        : {}),
      ...(env.CORELINK_CF_ACCESS_CLIENT_SECRET
        ? { CORELINK_CF_ACCESS_CLIENT_SECRET: env.CORELINK_CF_ACCESS_CLIENT_SECRET }
        : {}),
      // ...(env.CORELINK_CF_ACCESS_CLIENT_SECRET
      //   ? { CORELINK_CF_ACCESS_CLIENT_SECRET: env.CORELINK_CF_ACCESS_CLIENT_SECRET }
      //   : {}),
      FABRIC_BILLING_PUSH_INTERVAL_SECS: "30",
      ...(env.BILLING_INGEST_URL ? { BILLING_INGEST_URL: env.BILLING_INGEST_URL } : {}),
      ...(env.BILLING_INGEST_AUTH_KEY ? { BILLING_INGEST_AUTH_KEY: env.BILLING_INGEST_AUTH_KEY } : {}),
      ...(env.BILLING_REGION ? { BILLING_REGION: env.BILLING_REGION } : {}),
      ...(env.CLOUDFLARE_SPAWN_WORKER_URL
        ? { CLOUDFLARE_SPAWN_WORKER_URL: env.CLOUDFLARE_SPAWN_WORKER_URL }
        : {}),
      ...(env.CLOUDFLARE_SPAWN_AUTH_TOKEN
        ? { CLOUDFLARE_SPAWN_AUTH_TOKEN: env.CLOUDFLARE_SPAWN_AUTH_TOKEN }
        : {}),
      ...(env.CLOUDFLARE_EXEC_AUTH_TOKEN
        ? { CLOUDFLARE_EXEC_AUTH_TOKEN: env.CLOUDFLARE_EXEC_AUTH_TOKEN }
        : {}),
      ...(env.CLOUDFLARE_LIFECYCLE_AUTH_TOKEN
        ? { CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: env.CLOUDFLARE_LIFECYCLE_AUTH_TOKEN }
        : {}),
      ...(env.FABRIC_GITHUB_MINT_TOKEN
        ? { FABRIC_GITHUB_MINT_TOKEN: env.FABRIC_GITHUB_MINT_TOKEN }
        : {}),
      // Runner-broker App path (fallback to the PAT above). Forwarded only when
      // present so an unarmed deploy stays byte-identical; runner_broker.rs treats
      // the PAT as PREFERRED when both are set.
      ...(env.FABRIC_GITHUB_APP_ID ? { FABRIC_GITHUB_APP_ID: env.FABRIC_GITHUB_APP_ID } : {}),
      ...(env.FABRIC_GITHUB_APP_INSTALLATION_ID
        ? { FABRIC_GITHUB_APP_INSTALLATION_ID: env.FABRIC_GITHUB_APP_INSTALLATION_ID }
        : {}),
      // ⚠️ 2026-08-30 ENV BISECT step 2 — the minimal env BOOTED (/health 200),
      // so the fault is the env. This is the one large value in it (a base64 PEM).
      // Withheld to test the env-SIZE hypothesis. RESTORE AFTER READING.
      // ...(env.FABRIC_GITHUB_APP_PRIVATE_KEY_B64
      //   ? { FABRIC_GITHUB_APP_PRIVATE_KEY_B64: env.FABRIC_GITHUB_APP_PRIVATE_KEY_B64 }
      //   : {}),
      // R1 — durable ledger + vCPU ceiling, gated on DATABASE_URL. Present ⇒ pg
      // backend + FABRIC_RUNNER_VCPU=4 (standard-4 sizing) arm together; the #265
      // guard requires pg for an armed ceiling, so we never set one without the
      // other. Absent ⇒ neither key is injected → in-memory, unchanged behaviour.
      // Only exact `FABRIC_PG_DISABLED=0` permits this block — see the Env field.
      // This deliberately makes the operational kill-switch fail closed: a lost,
      // blank, whitespace-padded, or malformed binding cannot silently re-arm PG.
      // Every key here arms together (the pg backend, the vCPU ceiling the #265
      // guard ties to it, and the pg-only export), so suppressing them together is
      // the same coherent state as never having set the secret. Nothing half-arms.
      ...pgLedgerEnvVars(env),
      // ── Moat mint + env-0 + attested-cost — forward the wrangler vars/secrets
      // INTO the container (the fabricd binary reads these from its own env). The
      // mint trio (URL+key, cred-ticket secret, CLW endpoint) arm together or the
      // #-guard fails boot; absent ⇒ moat OFF. Each forwarded only when present so
      // an unarmed deploy stays byte-identical to the cold path. ──
      ...(env.CLW_ENDPOINT ? { CLW_ENDPOINT: env.CLW_ENDPOINT } : {}),
      // fabricd's own public base — the box's cred-ticket redemption target
      // ({base}/v1/leases/{id}/cas-cred). REQUIRED when the mint is armed (the
      // Rust boot guard fails closed without it); forwarded here so the container
      // process actually sees it (env.* alone never reaches the container).
      ...(env.FABRIC_PUBLIC_BASE_URL
        ? { FABRIC_PUBLIC_BASE_URL: env.FABRIC_PUBLIC_BASE_URL }
        : {}),
      ...(env.CORELINK_RUNNER_MINT_URL
        ? { CORELINK_RUNNER_MINT_URL: env.CORELINK_RUNNER_MINT_URL }
        : {}),
      ...(env.CORELINK_RUNNER_MINT_AUTH_KEY
        ? { CORELINK_RUNNER_MINT_AUTH_KEY: env.CORELINK_RUNNER_MINT_AUTH_KEY }
        : {}),
      ...(env.FABRIC_CRED_TICKET_SECRET
        ? { FABRIC_CRED_TICKET_SECRET: env.FABRIC_CRED_TICKET_SECRET }
        : {}),
      ...(env.FABRIC_EMIT_INTENT_METRICS_SIG
        ? { FABRIC_EMIT_INTENT_METRICS_SIG: env.FABRIC_EMIT_INTENT_METRICS_SIG }
        : {}),
      // Crash-surfacing sweep (WP-CRASH-SWEEP) — OPT-IN, no pg dependency. Present
      // valid u32≥1 ⇒ the sweep spawns at that cadence and surfaces crashed leases
      // (the always-on deadline reaper is the backstop when absent). Forwarded
      // unconditionally-when-set so the wrangler var actually reaches the container.
      ...(env.FABRIC_CRASH_PROBE_INTERVAL_SECS
        ? { FABRIC_CRASH_PROBE_INTERVAL_SECS: env.FABRIC_CRASH_PROBE_INTERVAL_SECS }
        : {}),
      // Acquire-path resilience (W0/W1): forward the global inbound cap + the
      // introspect admission cap so they actually reach the container (the
      // set-but-unforwarded trap — the Rust binary reads these at boot, but a
      // wrangler var/secret alone is only visible to the Worker as env.*).
      ...(env.FABRIC_MAX_INFLIGHT_REQUESTS
        ? { FABRIC_MAX_INFLIGHT_REQUESTS: env.FABRIC_MAX_INFLIGHT_REQUESTS }
        : {}),
      ...(env.FABRIC_INTROSPECT_MAX_INFLIGHT
        ? { FABRIC_INTROSPECT_MAX_INFLIGHT: env.FABRIC_INTROSPECT_MAX_INFLIGHT }
        : {}),
      // Unreachable-in-deploy fix: forward the enforcement/observability/safety
      // + Stage-B autoscaler passthroughs so a future `wrangler secret put` /
      // var actually reaches the container (all inert until set).
      ...(env.FABRIC_ADMIN_KEY ? { FABRIC_ADMIN_KEY: env.FABRIC_ADMIN_KEY } : {}),
      ...(env.FABRIC_OBSERVABILITY_KEY ? { FABRIC_OBSERVABILITY_KEY: env.FABRIC_OBSERVABILITY_KEY } : {}),
      // DEV/TEST-ONLY out-of-band cred-ticket mint — forwarded so a dev/test
      // `wrangler secret put FABRIC_TEST_MINT_KEY` actually reaches the container
      // (the set-but-unforwarded trap). Absent ⇒ the route 404s (inert). Never set
      // in a steady prod deploy — it arms a sensitive mint surface.
      ...(env.FABRIC_TEST_MINT_KEY ? { FABRIC_TEST_MINT_KEY: env.FABRIC_TEST_MINT_KEY } : {}),
      ...(env.FABRIC_TEST_MINT_TENANTS
        ? { FABRIC_TEST_MINT_TENANTS: env.FABRIC_TEST_MINT_TENANTS }
        : {}),
      ...(env.FABRIC_RUNNER_REPO_ALLOWLIST
        ? { FABRIC_RUNNER_REPO_ALLOWLIST: env.FABRIC_RUNNER_REPO_ALLOWLIST }
        : {}),
      ...(env.FABRIC_AUTOSCALER_WEBHOOK_SECRET
        ? { FABRIC_AUTOSCALER_WEBHOOK_SECRET: env.FABRIC_AUTOSCALER_WEBHOOK_SECRET }
        : {}),
      ...(env.FABRIC_AUTOSCALER_PAT ? { FABRIC_AUTOSCALER_PAT: env.FABRIC_AUTOSCALER_PAT } : {}),
      ...(env.FABRIC_AUTOSCALER_RUNNER_IMAGE
        ? { FABRIC_AUTOSCALER_RUNNER_IMAGE: env.FABRIC_AUTOSCALER_RUNNER_IMAGE }
        : {}),
      ...(env.FABRIC_AUTOSCALER_LABELS ? { FABRIC_AUTOSCALER_LABELS: env.FABRIC_AUTOSCALER_LABELS } : {}),
      ...(env.FABRIC_AUTOSCALER_TMP_ROOT
        ? { FABRIC_AUTOSCALER_TMP_ROOT: env.FABRIC_AUTOSCALER_TMP_ROOT }
        : {}),
      ...(env.FABRIC_AUTOSCALER_EXPIRY_MS
        ? { FABRIC_AUTOSCALER_EXPIRY_MS: env.FABRIC_AUTOSCALER_EXPIRY_MS }
        : {}),
      ...(env.FABRIC_AUTOSCALER_REPO_ALLOWLIST
        ? { FABRIC_AUTOSCALER_REPO_ALLOWLIST: env.FABRIC_AUTOSCALER_REPO_ALLOWLIST }
        : {}),
      ...(env.FABRIC_AUTOSCALER_MAX_TRACKED_JOBS
        ? { FABRIC_AUTOSCALER_MAX_TRACKED_JOBS: env.FABRIC_AUTOSCALER_MAX_TRACKED_JOBS }
        : {}),
      // ── BOOT-GUARD OVERRIDE — the escape hatch the FATAL message itself names.
      // `boot_introspect_selfcheck` aborts boot on a rejected introspect key and
      // prints "…or set FABRIC_INTROSPECT_BOOTCHECK=warn to override". That var was
      // documented in this file's comments but NEVER forwarded, so the override did
      // nothing and a rejected key was an UNRECOVERABLE outage: the container
      // crashes before binding, and nothing an operator can set reaches it.
      // (2026-08-30 outage; the guard is honoured by the deployed image — verified
      // at `e5f07f8:crates/corelink-fabric-server/src/server.rs:1106`.)
      ...(env.FABRIC_INTROSPECT_BOOTCHECK
        ? { FABRIC_INTROSPECT_BOOTCHECK: env.FABRIC_INTROSPECT_BOOTCHECK }
        : {}),
    };
  }

  // ── CONTAINER LIFECYCLE OBSERVABILITY ───────────────────────────────────────
  // Before this, a container that crashed at boot produced exactly one opaque
  // line at the Worker edge ("Failed to start container") and NOTHING about why:
  // the fabricd binary prints a precise `[boot] …` diagnostic for each of the
  // nine fallible steps before `TcpListener::bind`, and none of it was reachable.
  //
  // SCOPE, honestly stated: `@cloudflare/containers@0.3.7` exposes NO container
  // stdout/stderr — `monitor` is private and no type in the SDK carries process
  // output (checked in `dist/lib/container.d.ts` + `dist/types/index.d.ts`). So
  // this does NOT surface the `[boot]` lines. What it does surface is the exit
  // signal — `{ exitCode, reason }` from `StopParams` — which separates a clean
  // `anyhow` abort from a signal/OOM kill, plus any error the runtime reports.
  // Full boot-log observability needs a different mechanism and stays open.
  override onError(error: unknown): unknown {
    console.error(
      JSON.stringify({
        event: "fabricd_container_error",
        error: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
      }),
    );
    return super.onError(error);
  }

  // Did the container ever reach "started"? Separates "never came up" from
  // "came up and was stopped" — the two have completely different causes and the
  // exit signal alone cannot tell them apart.
  override onStart(): void | Promise<void> {
    console.error(JSON.stringify({ event: "fabricd_container_started" }));
    return super.onStart();
  }

  // The SDK's ONLY graceful-stop path (container.js:748 → this.stop()). A clean
  // exitCode 0 with no `fabricd_container_activity_expired` line preceding it
  // means the process exited on its OWN, not because we stopped it.
  override async onActivityExpired(): Promise<void> {
    console.error(JSON.stringify({ event: "fabricd_container_activity_expired" }));
    return super.onActivityExpired();
  }

  override onStop(params: StopParams): void | Promise<void> {
    console.error(
      JSON.stringify({
        event: "fabricd_container_stopped",
        exitCode: params.exitCode,
        reason: params.reason,
      }),
    );
    return super.onStop(params);
  }

  // Zero-idle-cost activity marker. All fabricd traffic funnels through this DO
  // (the Worker default fetch proxies every route via getContainer(FABRICD).fetch),
  // so the DO is the authoritative choke point for "is there real activity" —
  // no pg query, no cross-worker coupling.
  //
  // `/__do/idle-status` is answered PURELY from DO storage and MUST NOT delegate to
  // `super.fetch()` (which would `containerFetch` → start/renew the container and
  // defeat the whole purpose). The scheduled() idle gate reads it to decide whether
  // to skip the health probe for a sleeping shard. It is internal-only — the Worker
  // default fetch 404s any external `/__do/*` (see below).
  override async fetch(request: Request): Promise<Response> {
    const { pathname } = new URL(request.url);

    if (pathname === "/__do/idle-status") {
      const v = await this.ctx.storage.get<number>("lastActivityMs");
      return new Response(JSON.stringify({ lastActivityMs: v ?? 0 }), {
        headers: { "content-type": "application/json" },
      });
    }

    // Record real activity (health probes are NOT activity). Fire-and-forget via
    // waitUntil so a durable-storage write never adds latency to — or fails — the
    // lease hot path; a lost write only means a slightly-stale marker (⇒ stays warm
    // a touch longer ⇒ fail-SAFE). Only the container path renews sleepAfter, so
    // the marker write itself does not keep the container awake.
    if (pathname !== "/v1/health") {
      this.ctx.waitUntil(
        this.ctx.storage.put("lastActivityMs", Date.now()).catch(() => {}),
      );
    }

    return super.fetch(request);
  }
}

// A FIXED id ⇒ exactly one container instance serves all traffic (the in-memory
// ledger's single-instance requirement). At N=1 shardDoId returns THIS exact id
// for every shard, so the multi-instance routing below is byte-identical to the
// old singleton proxy (the inert-at-N=1 property).
// 2026-08-30 OUTAGE: the id is what pins PLACEMENT. Every instance since
// 2026-08-19 — across three different images (the Inc-3 build, a fresh rebuild
// from main, and the last verified-live 9191661 binary) and across container
// rollouts that genuinely recreated the instance — has been placed in the SAME
// colo (`bog04`) and has never reached `started`. Image-independent,
// config-independent, instance-independent, colo-constant. Renaming the DO
// forces a new placement; if the container then boots, the cause was placement,
// not this codebase. Lease state is pg-durable (DATABASE_URL), and this DO's own
// storage holds only the `lastActivityMs` marker, so a rename loses nothing.
export const SINGLETON = "fabricd-singleton-enam";

/** Shard count N from the wrangler var, parsed to int; default 1. */
function numShards(env: Env): number {
  const n = parseInt(env.FABRIC_NUM_SHARDS ?? "", 10);
  return Number.isFinite(n) && n >= 1 ? n : 1;
}

/**
 * The DO id for shard `k` under `n` shards. At n===1 this is the SINGLETON id
 * for ALL k — byte-identical to today, NO container identity change (the inert
 * property). At n>1 each shard gets its own stable container id.
 */
// ── PLACEMENT (2026-08-30 outage fix) ───────────────────────────────────────
// A container Durable Object runs where the DO lives, and a DO is placed near
// whoever first touched it. Every fabricd instance since 2026-08-19 landed in
// `bog04` (traffic originates in South America) and NONE of them ever served —
// across three images, several genuine container rollouts, a DO rename and a
// machine-shape change. Meanwhile every HEALTHY container app on this account
// runs in US colos (`ord02`, `ewr16`) and none in bog04.
//
// So the placement is not incidental to the outage; it is the one variable that
// never changed. `locationHint` is honoured only when the DO is FIRST created,
// which is why SINGLETON also carries a fresh suffix — an existing DO cannot be
// relocated. `enam` puts the control plane next to corelink-api (BILLING_REGION
// is already `iad`), which also shortens every introspect/mint round-trip.
//
// The hint is applied by wrapping the namespace rather than by replacing
// `getContainer`, deliberately: `getContainer` stays the single seam the tests
// mock, so this changes production placement without touching the test surface.
const FABRICD_LOCATION_HINT: DurableObjectLocationHint = "enam";

function placed(ns: DurableObjectNamespace<FabricdContainer>): DurableObjectNamespace<FabricdContainer> {
  return {
    ...ns,
    idFromName: (name: string) => ns.idFromName(name),
    get: (id: DurableObjectId) => ns.get(id, { locationHint: FABRICD_LOCATION_HINT }),
  } as DurableObjectNamespace<FabricdContainer>;
}

function shardDoId(k: number, n: number): string {
  return n === 1 ? SINGLETON : `fabricd-shard-${k}`;
}

// Round-robin cursor for ACQUIRE placement. A lease's id is minted (Rust side)
// to hash back to the shard that acquired it, so subsequent lease-ops route via
// shardOf — only the initial acquire is placed round-robin.
let acquireCursor = 0;

// Round-robin cursor for GitHub-webhook autoscaler placement. The webhook drives
// out-of-band acquires (workflow_job.queued → provision_runner); a fixed shard-0
// route would concentrate every autoscaler-driven runner on one shard. A second
// cursor spreads them like ACQUIRE does (the acquire path reads the same
// X-Fabricd-* headers, so the minted lease-id hashes back to the chosen shard).
let webhookCursor = 0;

/**
 * Extract the `{lease_id}` from a `/v1/leases/{lease_id}/...` path, or null for
 * the collection endpoint (`/v1/leases`) and any non-lease path.
 */
function leaseIdOf(pathname: string): string | null {
  const m = pathname.match(/^\/v1\/leases\/([^/]+)(?:\/.*)?$/);
  return m ? decodeURIComponent(m[1]) : null;
}

/**
 * Wire shape of `GET /v1/leases` (matches the Rust `lease_list::LeaseListResponse`
 * — `{ tenant, leases: [...] }`, NOT a bare array). Each shard is tenant-scoped by
 * the same Bearer PAT, so every shard returns the SAME `tenant` and its OWN slice
 * of that tenant's leases.
 */
interface LeaseListBody {
  tenant?: unknown;
  leases?: Array<{ lease_id: string; [k: string]: unknown }>;
}

/**
 * `GET /v1/leases` scatter-gather. Under N>1 each shard holds only its OWN leases
 * in memory, so shard 0 alone gives an INCOMPLETE list. Fan the (cloned) request
 * out to all N shards concurrently, merge the per-shard `leases` arrays into one,
 * dedup by `lease_id` (a lease lives on exactly one shard — dedup is defensive),
 * and re-key on the shared `tenant`. Best-effort: a down/erroring shard is skipped
 * so it can't blank the whole list; only if EVERY shard fails do we propagate an
 * error. Result is ordered by `lease_id` to preserve the single-shard contract's
 * deterministic ordering.
 *
 * N===1 SHORT-CIRCUITS to a plain passthrough (no parse/re-serialize) so the bytes
 * are byte-identical to the old singleton proxy.
 */
async function listLeasesScatterGather(
  request: Request,
  env: Env,
  N: number,
  applyTimeout: boolean,
): Promise<Response> {
  // N=1: byte-identical passthrough — no parse, no re-serialize. Still bounded by
  // the per-request timeout (a wedged singleton fails fast with a 503).
  if (N === 1) {
    return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(0, 1)), request, applyTimeout);
  }

  const settled = await Promise.allSettled(
    Array.from({ length: N }, (_unused, k) =>
      // Bound each shard fetch so ONE wedged shard can't hang the whole gather:
      // a timed-out shard rejects → allSettled marks it "rejected" → skipped
      // (best-effort), exactly like a down shard below.
      getContainer(placed(env.FABRICD), shardDoId(k, N)).fetch(shardFanRequest(request, applyTimeout)),
    ),
  );

  const byId = new Map<string, { lease_id: string; [k: string]: unknown }>();
  let tenant: unknown;
  let contentType = "application/json";
  let anyOk = false;
  let firstErrorResponse: Response | null = null;

  for (const outcome of settled) {
    if (outcome.status !== "fulfilled") continue;
    const resp = outcome.value;
    if (resp.status !== 200) {
      if (firstErrorResponse === null) firstErrorResponse = resp;
      continue;
    }
    anyOk = true;
    const ct = resp.headers.get("content-type");
    if (ct) contentType = ct;
    let body: LeaseListBody;
    try {
      body = (await resp.json()) as LeaseListBody;
    } catch {
      continue; // malformed body from a shard — skip it (best-effort)
    }
    if (tenant === undefined && body.tenant !== undefined) tenant = body.tenant;
    for (const lease of body.leases ?? []) {
      if (lease && typeof lease.lease_id === "string") byId.set(lease.lease_id, lease);
    }
  }

  // Every shard failed → propagate a shard's error (or a 502 if all rejected).
  if (!anyOk) {
    if (firstErrorResponse !== null) return firstErrorResponse;
    return new Response(JSON.stringify({ error: "all fabricd shards unreachable" }), {
      status: 502,
      headers: { "content-type": "application/json" },
    });
  }

  const leases = Array.from(byId.values()).sort((a, b) =>
    a.lease_id < b.lease_id ? -1 : a.lease_id > b.lease_id ? 1 : 0,
  );
  return new Response(JSON.stringify({ tenant, leases }), {
    status: 200,
    headers: { "content-type": contentType },
  });
}

/**
 * Wire shape of `GET /v1/metrics/tenant` (matches the Rust
 * `metrics::TenantMetricsResponse`): the caller's own wait stats — a bounded
 * 6-bucket histogram plus nearest-rank p50/p95. Each shard computes these over
 * only ITS OWN in-memory samples, so shard-0 alone is a partial view at N>1.
 */
interface TenantMetricsBody {
  tenant?: unknown;
  p50_ms?: unknown;
  p95_ms?: unknown;
  histogram?: unknown;
  count?: unknown;
}

// Representative value (ms) reported for a percentile that falls in each bucket
// when merging across shards. The buckets are `[<10 · <50 · <250 · <1s · <5s ·
// >=5s]` (exclusive upper bounds `WAIT_BUCKET_BOUNDS_MS = [10,50,250,1000,5000]`
// in the Rust `interference` core). From merged bucket COUNTS the raw sample
// values are gone, so the exact nearest-rank value is unrecoverable; we report
// each bucket's upper bound (the catch-all `>=5s` bucket has no upper bound → its
// 5000ms floor). This is an approximation ONLY at N>1; N=1 passes the container's
// exact percentiles through byte-identically (it never reaches this merge).
const WAIT_BUCKET_REPR_MS: [number, number, number, number, number, number] = [
  10, 50, 250, 1_000, 5_000, 5_000,
];

/**
 * Nearest-rank percentile over MERGED histogram bucket counts — the same rank
 * formula as the Rust `interference::nearest_rank`
 * (`rank = ceil(count * pct / 100)`, clamped to >=1), walked cumulatively across
 * the buckets to find which one the rank lands in. Returns that bucket's
 * representative value. `count == 0` ⇒ 0 (matches `WaitSnapshot::default`).
 *
 * NOT an average of per-shard percentiles — that would be wrong; percentiles do
 * not compose. Merging the underlying bucket counts and re-ranking is the correct
 * (approximate, bucket-resolution) merge given only histograms are exposed.
 */
function percentileFromBuckets(buckets: number[], count: number, percentile: number): number {
  if (count === 0) return 0;
  // ceil(count * percentile / 100), integer form == Rust's `div_ceil(100)`.
  const rank = Math.max(1, Math.floor((count * percentile + 99) / 100));
  let cumulative = 0;
  for (let i = 0; i < buckets.length; i++) {
    cumulative += buckets[i];
    if (cumulative >= rank) return WAIT_BUCKET_REPR_MS[i];
  }
  return WAIT_BUCKET_REPR_MS[WAIT_BUCKET_REPR_MS.length - 1];
}

/**
 * `GET /v1/metrics/tenant` scatter-gather. Under N>1 each shard reports wait
 * stats over only its own in-memory samples, so shard-0 alone under-counts. Fan
 * the (cloned) request out to all N shards, SUM the 6 histogram buckets across
 * shards, recompute `count` as the total, and recompute p50/p95 by nearest-rank
 * over the MERGED buckets (never by averaging per-shard percentiles). Best-effort
 * like the lease-list merge: a down/erroring shard is skipped; only if EVERY shard
 * fails do we propagate an error. `tenant` is the same on every shard (Bearer-PAT
 * scoped) — take the first defined.
 *
 * N===1 SHORT-CIRCUITS via the caller (this helper is only invoked at N>1); the
 * N=1 path passes the container's exact snapshot through byte-identically.
 */
async function tenantMetricsScatterGather(
  request: Request,
  env: Env,
  N: number,
  applyTimeout: boolean,
): Promise<Response> {
  const settled = await Promise.allSettled(
    Array.from({ length: N }, (_unused, k) =>
      // Bound each shard fetch (see listLeasesScatterGather) — a wedged shard is
      // skipped best-effort rather than hanging the whole gather.
      getContainer(placed(env.FABRICD), shardDoId(k, N)).fetch(shardFanRequest(request, applyTimeout)),
    ),
  );

  const merged = [0, 0, 0, 0, 0, 0];
  let tenant: unknown;
  let contentType = "application/json";
  let anyOk = false;
  let firstErrorResponse: Response | null = null;

  for (const outcome of settled) {
    if (outcome.status !== "fulfilled") continue;
    const resp = outcome.value;
    if (resp.status !== 200) {
      if (firstErrorResponse === null) firstErrorResponse = resp;
      continue;
    }
    anyOk = true;
    const ct = resp.headers.get("content-type");
    if (ct) contentType = ct;
    let body: TenantMetricsBody;
    try {
      body = (await resp.json()) as TenantMetricsBody;
    } catch {
      continue; // malformed body from a shard — skip it (best-effort)
    }
    if (tenant === undefined && body.tenant !== undefined) tenant = body.tenant;
    if (Array.isArray(body.histogram)) {
      for (let i = 0; i < 6; i++) {
        const v = body.histogram[i];
        if (typeof v === "number" && Number.isFinite(v)) merged[i] += v;
      }
    }
  }

  // Every shard failed → propagate a shard's error (or a 502 if all rejected).
  if (!anyOk) {
    if (firstErrorResponse !== null) return firstErrorResponse;
    return new Response(JSON.stringify({ error: "all fabricd shards unreachable" }), {
      status: 502,
      headers: { "content-type": "application/json" },
    });
  }

  // count == sum of buckets (the per-shard invariant count == sum(histogram)).
  const count = merged.reduce((a, b) => a + b, 0);
  return new Response(
    JSON.stringify({
      tenant,
      p50_ms: percentileFromBuckets(merged, count, 50),
      p95_ms: percentileFromBuckets(merged, count, 95),
      histogram: merged,
      count,
    }),
    { status: 200, headers: { "content-type": contentType } },
  );
}

// ── Per-request timeout on the proxied container fetch (W1b) ────────────────
// Every route below forwards to the singleton (or a shard) container. With NO
// bound, a WEDGED container makes the client's request hang until the platform's
// own (much longer, opaque) limit — and concurrent clients pile up on the
// stalled singleton. We wrap each proxied fetch in an AbortSignal.timeout so a
// hung upstream fails the client FAST with a clean, structured 503 instead.
//
// 30s is chosen deliberately: it sits ABOVE the slowest server-BOUNDED route's
// legitimate latency (a cold box provision on acquire; the scatter-gather read
// merges answer in ms) yet still surfaces a genuine hang quickly. The UNBOUNDED
// routes that run customer/build code or block on the §13 ack window — POST
// /v1/leases/{id}/exec, POST /v1/leases/{id}/close (up to a 30s ack), and POST
// /v1/queue/trigger (reuses the same exec engine) — are EXEMPT (isLongLivedRoute):
// no finite timeout is correct for them, so they forward unbounded as today.
const PROXY_FETCH_TIMEOUT_MS = 30_000;

/** Minimal shape of a `getContainer(...)` handle — only the `fetch` we call. */
interface ContainerLike {
  fetch(request: Request): Promise<Response>;
}

/** A fired AbortSignal.timeout rejects fetch with a Timeout/Abort-named error. */
function isAbortLikeError(e: unknown): boolean {
  return e instanceof Error && (e.name === "TimeoutError" || e.name === "AbortError");
}

/** Structured 503 for a wedged upstream — a generic reason, NO internals leaked. */
function upstreamTimeout503(): Response {
  return new Response(JSON.stringify({ error: "fabricd upstream timeout" }), {
    status: 503,
    headers: { "content-type": "application/json", "retry-after": "1" },
  });
}

/**
 * True for the routes that legitimately BLOCK for an unbounded / long duration:
 * they run customer/build code or wait on the §13 ack window, so no finite
 * per-request timeout is correct — they MUST forward unbounded.
 *   • POST /v1/queue/trigger      — reuses the exec engine (run_check)
 *   • POST /v1/leases/{id}/exec   — synchronous box command execution
 *   • POST /v1/leases/{id}/close  — blocks up to the 30s §13 JobClose ack window
 * (agent-exec is async: POST returns 202 + step_id immediately and the poll GET
 *  is non-blocking — both are bounded, so they are NOT exempt.)
 */
export function isLongLivedRoute(method: string, pathname: string): boolean {
  if (method !== "POST") return false;
  if (pathname === "/v1/queue/trigger") return true;
  return /^\/v1\/leases\/[^/]+\/(exec|close)$/.test(pathname);
}

/**
 * Forward `request` to `container`. When `applyTimeout` (a non-long-lived route),
 * bound the wait with AbortSignal.timeout and translate a hung upstream into a
 * structured 503. A long-lived route forwards unbounded + byte-identically. Any
 * NON-abort error propagates unchanged (best-effort merge helpers rely on this).
 */
async function proxyFetch(
  container: ContainerLike,
  request: Request,
  applyTimeout: boolean,
): Promise<Response> {
  if (!applyTimeout) return container.fetch(request);
  const bounded = new Request(request, {
    signal: AbortSignal.timeout(PROXY_FETCH_TIMEOUT_MS),
  });
  try {
    return await container.fetch(bounded);
  } catch (e) {
    if (isAbortLikeError(e)) {
      console.log(
        `proxy: upstream fetch to ${new URL(request.url).pathname} exceeded ${PROXY_FETCH_TIMEOUT_MS}ms — failing fast 503`,
      );
      return upstreamTimeout503();
    }
    throw e;
  }
}

/**
 * A per-shard clone of a scatter-gather request, optionally bounded by the
 * per-request timeout. The clone is required because one Request body can be
 * consumed only once, so each of the N concurrent shard fetches needs its own;
 * the reads are always GETs (leases-list / tenant-metrics) so the clone is cheap.
 */
function shardFanRequest(request: Request, applyTimeout: boolean): Request {
  return new Request(
    request.clone(),
    applyTimeout ? { signal: AbortSignal.timeout(PROXY_FETCH_TIMEOUT_MS) } : {},
  );
}

// ── Watchdog boot-grace (W1b) ───────────────────────────────────────────────
// The scheduled() watchdog destroys a container that fails 3 consecutive health
// probes (~30s of sustained unresponsiveness). But a container that has NEVER
// yet answered is either (a) genuinely broken OR (b) still COLD-BOOTING — a fresh
// Rust+pg boot under load can exceed one cron tick's probe budget. Destroying a
// still-booting container restarts the boot → a boot LOOP. So:
//   • BOOT-GRACE: a never-yet-healthy container is exempt from destroy until it
//     has been watched for at least BOOT_GRACE_MS — long enough to finish a cold
//     boot. A previously-healthy container that goes dark is NOT booting, so it
//     is destroyed on the first sustained failure (preserves the #316 recovery).
//   • REBOOT-BACKOFF: after a destroy, do not destroy the SAME container again
//     within REBOOT_BACKOFF_MS — the replacement needs its own cold-boot budget;
//     destroying again inside that window would thrash.
// Both windows cover a cold Rust+pg boot under load (image start + pg pool warm).
const BOOT_GRACE_MS = 180_000; // 3 min — a never-healthy container is "booting" below this
const REBOOT_BACKOFF_MS = 180_000; // 3 min — don't re-destroy a just-rebooted container

export interface WatchdogEntry {
  firstSeenAt: number; // first cron tick this container id was observed
  firstHealthyAt: number | null; // first successful health probe, ever (null = never up)
  lastDestroyAt: number | null; // last destroy() issued for this id (null = never)
}

export type WatchdogAction = "healthy" | "skip-booting" | "skip-backoff" | "destroy";

export type IdleGateDecision =
  | { action: "probe"; lastActivityMs: number }
  | { action: "skip"; reason: "idle" | "unset" | "malformed_marker" };

/**
 * Decide whether a container-waking health probe is permitted by the
 * container-free activity marker. Only a structurally valid, positive,
 * non-future, recent timestamp is affirmative. Everything uncertain refuses
 * the probe; zero is the DO's explicit "never active" value.
 */
export function idleGateDecision(
  body: unknown,
  now: number,
  idleMs: number,
): IdleGateDecision {
  if (body === null || typeof body !== "object" || Array.isArray(body)) {
    return { action: "skip", reason: "malformed_marker" };
  }

  const lastActivityMs = (body as { lastActivityMs?: unknown }).lastActivityMs;
  if (lastActivityMs === 0) return { action: "skip", reason: "unset" };
  if (
    typeof lastActivityMs !== "number" ||
    !Number.isSafeInteger(lastActivityMs) ||
    lastActivityMs < 0 ||
    lastActivityMs > now
  ) {
    return { action: "skip", reason: "malformed_marker" };
  }
  if (now - lastActivityMs > idleMs) return { action: "skip", reason: "idle" };
  return { action: "probe", lastActivityMs };
}

/**
 * Pure boot-grace decision for one container tick. `entry` is mutated to record
 * firstHealthyAt on a healthy probe (the caller persists it across ticks). Given
 * whether this tick's probes succeeded and `now`, decide:
 *   • healthy      — answered; nothing to do (firstHealthyAt recorded)
 *   • skip-backoff — unhealthy, but a destroy was issued < REBOOT_BACKOFF_MS ago
 *   • skip-booting — unhealthy, NEVER yet healthy, still within BOOT_GRACE_MS
 *   • destroy      — sustained unresponsiveness that is NOT a cold boot → reboot
 */
export function watchdogAction(
  entry: WatchdogEntry,
  healthy: boolean,
  now: number,
  cfg: { bootGraceMs: number; rebootBackoffMs: number } = {
    bootGraceMs: BOOT_GRACE_MS,
    rebootBackoffMs: REBOOT_BACKOFF_MS,
  },
): WatchdogAction {
  if (healthy) {
    if (entry.firstHealthyAt === null) {
      entry.firstHealthyAt = now;
      entry.lastDestroyAt = null; // a successful (re)boot clears the reboot backoff
    }
    return "healthy";
  }
  if (entry.lastDestroyAt !== null && now - entry.lastDestroyAt < cfg.rebootBackoffMs) {
    return "skip-backoff";
  }
  if (entry.firstHealthyAt === null && now - entry.firstSeenAt < cfg.bootGraceMs) {
    return "skip-booting";
  }
  return "destroy";
}

// Per-container watchdog state, keyed by DO id. Module scope persists across
// scheduled() ticks within an isolate (best-effort, same as the round-robin
// cursors above); a recycled isolate simply re-learns firstSeenAt on the next
// tick, which only restarts the (safe) boot-grace clock.
const watchdogState = new Map<string, WatchdogEntry>();

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const N = numShards(env);
    const { pathname } = new URL(request.url);
    // `/__do/*` is the DO-internal, container-free activity surface (idle-status)
    // reached ONLY by scheduled() via getContainer(...).fetch(); it must never be
    // exposed to external callers, so 404 it on the public Worker entry.
    if (pathname.startsWith("/__do/")) {
      return new Response("not found", { status: 404 });
    }
    // Non-long-lived routes get the per-request timeout → clean 503 on a wedged
    // upstream; exec/close/queue-trigger forward unbounded (isLongLivedRoute).
    const applyTimeout = !isLongLivedRoute(request.method, pathname);

    // ACQUIRE — POST to the collection exactly. Place on a round-robin shard and
    // stamp the chosen N + shard so the container can mint a lease-id that hashes
    // back to this shard (X-Fabricd-Shard) under this fan-out (X-Fabricd-Num-Shards).
    if (request.method === "POST" && pathname === "/v1/leases") {
      const k = ((acquireCursor++ % N) + N) % N;
      const modified = new Request(request);
      modified.headers.set("X-Fabricd-Num-Shards", String(N));
      modified.headers.set("X-Fabricd-Shard", String(k));
      return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(k, N)), modified, applyTimeout);
    }

    // LEASE-LIST — GET the collection exactly (NOT /v1/leases/{id}). Each shard
    // holds only its own leases in memory, so scatter-gather across all N and
    // merge; N===1 short-circuits to a byte-identical passthrough.
    if (request.method === "GET" && pathname === "/v1/leases") {
      return listLeasesScatterGather(request, env, N, applyTimeout);
    }

    // §9 TRIGGER — POST /v1/queue/trigger carries the lease id in the BODY
    // (`lease_id`), not the URL, so leaseIdOf can't see it and it would fall to
    // "everything else → shard 0" — exec fails-closed for any lease NOT on shard
    // 0 (§13 hook-not-found). At N>1 read+parse the body, route by shardOf(the
    // body's lease_id), and re-attach the consumed body verbatim to the forwarded
    // request. Missing/unparseable/no lease_id → shard 0 (the old behaviour). At
    // N=1 this branch is inert; the request falls through to shard 0 == singleton
    // untouched (byte-identical — no body read).
    if (N > 1 && request.method === "POST" && pathname === "/v1/queue/trigger") {
      const raw = await request.text();
      let leaseId: string | null = null;
      try {
        const parsed = JSON.parse(raw) as { lease_id?: unknown };
        if (parsed && typeof parsed.lease_id === "string") leaseId = parsed.lease_id;
      } catch {
        // Unparseable body → fall back to shard 0 (leaseId stays null).
      }
      const k = leaseId !== null ? shardOf(leaseId, N) : 0;
      // Re-attach the buffered body to a fresh request (same method/headers/bytes).
      const forwarded = new Request(request.url, {
        method: request.method,
        headers: request.headers,
        body: raw,
      });
      // §9 trigger is long-lived (reuses the exec engine) → applyTimeout is false
      // here, so proxyFetch forwards unbounded + byte-identically.
      return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(k, N)), forwarded, applyTimeout);
    }

    // GITHUB WEBHOOK — POST /webhooks/github drives the autoscaler's out-of-band
    // acquires. Routed to shard 0 it would pile every autoscaler runner on one
    // shard, so at N>1 pick a round-robin shard and stamp the frozen X-Fabricd-*
    // headers (the acquire path honours them so the minted lease-id hashes back to
    // this shard). The body is HMAC-signed — do NOT read/alter it; `new
    // Request(request)` carries the raw bytes verbatim while headers stay mutable
    // (same pattern as ACQUIRE). At N=1 this is inert → falls through to shard 0.
    if (N > 1 && request.method === "POST" && pathname === "/webhooks/github") {
      const k = ((webhookCursor++ % N) + N) % N;
      const modified = new Request(request);
      modified.headers.set("X-Fabricd-Num-Shards", String(N));
      modified.headers.set("X-Fabricd-Shard", String(k));
      return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(k, N)), modified, applyTimeout);
    }

    // TENANT METRICS — GET /v1/metrics/tenant reads per-instance in-memory
    // wait_stats, so shard-0 alone is a partial view at N>1. Scatter-gather across
    // all N shards and merge the histograms (sum buckets, recompute count +
    // nearest-rank percentiles). At N=1 this is inert → falls through to shard 0
    // == singleton, a byte-identical passthrough of the exact snapshot.
    if (N > 1 && request.method === "GET" && pathname === "/v1/metrics/tenant") {
      return tenantMetricsScatterGather(request, env, N, applyTimeout);
    }

    // LEASE-OP — a request that names a lease id. Route to the shard that owns
    // the lease (deterministic: same hash the Rust side used to mint the id).
    const leaseId = leaseIdOf(pathname);
    if (leaseId !== null) {
      const k = shardOf(leaseId, N);
      return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(k, N)), request, applyTimeout);
    }

    // Everything else (/v1/health, /v1/attestation/key, …) → shard 0.
    return proxyFetch(getContainer(placed(env.FABRICD), shardDoId(0, N)), request, applyTimeout);
  },

  async scheduled(_event: ScheduledController, env: Env): Promise<void> {
    // Keep-alive ping so the singleton never sleeps (the 24/7 knob) — AND a
    // liveness watchdog + self-heal (2026-07-08 recurring-hang incident: the
    // singleton went dark on its own, `/v1/health` timing out, needing a MANUAL
    // delete+redeploy each time).
    //
    // ⚠️ CONSECUTIVE-FAILURE GATE (2026-07-08 go-live hardening): the watchdog
    // MUST NOT destroy a container that is merely BUSY. A legitimate §13 close
    // holds its request up to the 30s JobClose ack window; two concurrent such
    // closes can transiently saturate the standard-2 workers and make a SINGLE
    // 10s /v1/health probe miss — which the old single-probe watchdog mistook for
    // a hang and destroyed, aborting in-flight work and forcing the exact
    // "Failed to start container" cold-start we hit at go-live. A GENUINE hang
    // (the #316 dark-container case) stays unresponsive for minutes; a busy blip
    // recovers within seconds. So we probe up to 3× with a gap and destroy ONLY
    // when ALL probes fail (~30s of SUSTAINED unresponsiveness) — this still
    // catches a real hang (it never recovers) while tolerating a busy singleton.
    const N = numShards(env);

    const PROBES = 3; // consecutive failures required to declare a real hang
    const PROBE_TIMEOUT_MS = 8_000;
    const GAP_MS = 5_000; // between probes — lets a busy worker free up
    // Idle threshold, kept just UNDER sleepAfter (5m) so the cron stops probing a
    // shard before its container would sleep — a probe must never re-wake an
    // about-to-sleep idle container (that would defeat zero-idle-cost).
    const IDLE_MS = 4 * 60_000;

    // Probe every shard independently — the same 3-consecutive-failure watchdog
    // per container. At N=1 this is a single iteration = today's behaviour.
    for (let k = 0; k < N; k++) {
      const id = shardDoId(k, N);
      const container = getContainer(placed(env.FABRICD), id);

      // ── Zero-idle-cost gate ────────────────────────────────────────────────
      // Read the container-free activity marker (handled by FabricdContainer.fetch
      // WITHOUT touching the container). If this shard has had no real request for
      // > IDLE_MS, it is idle → SKIP the /v1/health probe so the container can
      // sleep (a probe would wake it). A sleeping container is not "dark": the next
      // real acquire wakes it, and a genuine hang while a job is active is caught
      // because the box's own call fails. This gate is affirmative-only: only a
      // valid, recent marker permits the legacy watchdog. Read errors, non-200s,
      // malformed JSON/markers, future timestamps, and unset markers all refuse
      // `/v1/health`, so uncertainty can never wake a container or re-arm PG.
      let gateDecision: IdleGateDecision;
      try {
        const idleRes = await container.fetch(
          new Request("http://fabricd/__do/idle-status"),
        );
        if (idleRes.status !== 200) {
          console.warn(
            JSON.stringify({
              event: "fabricd_watchdog_probe_refused",
              reason: "idle_status_non_200",
              status: idleRes.status,
              shard: k,
              numShards: N,
            }),
          );
          watchdogState.delete(id);
          continue;
        }

        let body: unknown;
        try {
          body = await idleRes.json();
        } catch {
          console.warn(
            JSON.stringify({
              event: "fabricd_watchdog_probe_refused",
              reason: "idle_status_malformed_json",
              shard: k,
              numShards: N,
            }),
          );
          watchdogState.delete(id);
          continue;
        }
        gateDecision = idleGateDecision(body, Date.now(), IDLE_MS);
      } catch {
        console.warn(
          JSON.stringify({
            event: "fabricd_watchdog_probe_refused",
            reason: "idle_status_unreachable",
            shard: k,
            numShards: N,
          }),
        );
        watchdogState.delete(id);
        continue;
      }
      if (gateDecision.action === "skip") {
        // Clear watchdog lifecycle so the next active period treats a possibly-slept
        // container as a fresh boot, not a previously-healthy one that "went dark".
        watchdogState.delete(id);
        if (gateDecision.reason === "malformed_marker") {
          console.warn(
            JSON.stringify({
              event: "fabricd_watchdog_probe_refused",
              reason: gateDecision.reason,
              shard: k,
              numShards: N,
            }),
          );
        }
        continue;
      }

      // Persisted per-container lifecycle (module scope; see watchdogState). A
      // never-before-seen id starts its boot-grace clock now.
      let entry = watchdogState.get(id);
      if (entry === undefined) {
        entry = { firstSeenAt: Date.now(), firstHealthyAt: null, lastDestroyAt: null };
        watchdogState.set(id, entry);
      }

      let healthy = false;

      for (let attempt = 1; attempt <= PROBES; attempt++) {
        const t0 = Date.now();
        try {
          const resp = await container.fetch(
            new Request("http://fabricd/v1/health", {
              signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
              // Stamp the shard so a future shard-aware /v1/health can use it.
              headers: { "X-Fabricd-Shard": String(k) },
            }),
          );
          if (resp.status === 200) {
            console.log(
              `keep-warm[shard ${k}/${N}]: health 200 in ${Date.now() - t0}ms (probe ${attempt}/${PROBES})`,
            );
            healthy = true;
            break; // this shard healthy (possibly recovered from a busy blip)
          }
          console.log(
            `keep-warm[shard ${k}/${N}]: health ${resp.status} in ${Date.now() - t0}ms (probe ${attempt}/${PROBES})`,
          );
        } catch (e) {
          console.log(
            `keep-warm[shard ${k}/${N}]: health UNREACHABLE in ${Date.now() - t0}ms (probe ${attempt}/${PROBES}: ${e})`,
          );
        }
        // Probe failed. If more probes remain, wait a beat and retry — a container
        // busy with a long close will free a worker and answer the next probe.
        if (attempt < PROBES) {
          await new Promise((r) => setTimeout(r, GAP_MS));
        }
      }

      // BOOT-GRACE decision (see watchdogAction): a never-yet-healthy container
      // within the boot window is COLD-BOOTING, not hung — destroying it would
      // boot-loop. A previously-healthy container that went dark IS a real hang
      // and is destroyed on the first sustained failure (#316 recovery). A
      // just-rebooted container is spared for one backoff window (no thrash).
      const now = Date.now();
      const action = watchdogAction(entry, healthy, now);

      if (action === "healthy") continue;

      if (action === "skip-backoff") {
        console.log(
          `keep-warm[shard ${k}/${N}]: ${PROBES} health failures but destroyed ${now - (entry.lastDestroyAt ?? now)}ms ago (< ${REBOOT_BACKOFF_MS}ms reboot backoff) — letting the fresh instance boot, NOT re-destroying`,
        );
        continue;
      }

      if (action === "skip-booting") {
        console.log(
          `keep-warm[shard ${k}/${N}]: ${PROBES} health failures but never-yet-healthy for only ${now - entry.firstSeenAt}ms (< ${BOOT_GRACE_MS}ms boot-grace) — cold boot in progress, NOT destroying`,
        );
        continue;
      }

      // action === "destroy": sustained unresponsiveness that is NOT a cold boot.
      // Destroy so a fresh instance boots on the next fetch (self-heal).
      console.log(
        `keep-warm[shard ${k}/${N}]: ${PROBES} consecutive health failures (~30s) — destroying (${
          entry.firstHealthyAt === null ? "never healthy past boot-grace" : "was healthy, went dark"
        })`,
      );
      try {
        await container.destroy();
        // The replacement is a fresh cold boot: reset the lifecycle so it earns
        // its OWN boot-grace, and arm the reboot backoff so we don't thrash.
        entry.firstSeenAt = now;
        entry.firstHealthyAt = null;
        entry.lastDestroyAt = now;
        console.log(
          `keep-warm[shard ${k}/${N}]: destroyed hung shard — fresh instance will boot on next request`,
        );
      } catch (e) {
        console.log(`keep-warm[shard ${k}/${N}]: destroy() failed: ${e}`);
      }
    }
  },
};
