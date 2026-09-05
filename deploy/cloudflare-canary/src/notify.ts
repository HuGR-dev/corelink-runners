// Alert transport, behind an interface. Default-OFF and fail-soft:
//  - if RESEND_API_KEY / ALERT_EMAIL_TO / ALERT_EMAIL_FROM are unset ⇒ NO-OP
//    that logs a line (so the Worker deploys + runs before the owner arms it).
//  - it NEVER throws (a transport failure must not crash a scheduled run) and
//    NEVER logs key material.

import type { Alert, Severity } from "./rules";

export interface NotifyEnv {
  /** Resend API key (`wrangler secret put`). Absent ⇒ email is a logged no-op. */
  RESEND_API_KEY?: string;
  /** Destination address (owner). `wrangler var`/secret. */
  ALERT_EMAIL_TO?: string;
  /** From address — MUST be on a Resend-verified humangr.com domain. */
  ALERT_EMAIL_FROM?: string;
}

export interface SendResult {
  sent: boolean;
  /** Why it did not send (config missing / transport error), for the log only. */
  reason?: string;
}

const SEVERITY_RANK: Record<Severity, number> = { critical: 3, warn: 2, info: 1 };

/** Highest severity present, for the subject prefix. */
function topSeverity(alerts: Alert[]): Severity {
  let top: Severity = "info";
  for (const a of alerts) {
    if (SEVERITY_RANK[a.severity] > SEVERITY_RANK[top]) top = a.severity;
  }
  return top;
}

/** Build the plain-text email (subject + body) for a set of alerts. PURE. */
export function formatAlertEmail(alerts: Alert[], now = Date.now()): { subject: string; text: string } {
  const sev = topSeverity(alerts);
  const tag = sev === "critical" ? "CRITICAL" : sev === "warn" ? "WARN" : "INFO";
  const subject = `[CoreLink canary] ${tag}: ${alerts.length} alert${alerts.length === 1 ? "" : "s"}`;
  const lines: string[] = [];
  lines.push("CoreLink golden-counter canary detected the following:");
  lines.push("");
  for (const a of alerts) {
    lines.push(`  [${a.severity.toUpperCase()}] ${a.title}`);
    lines.push(`      ${a.detail}`);
    lines.push(`      (rule: ${a.key})`);
    lines.push("");
  }
  lines.push(`Detected at ${new Date(now).toISOString()}.`);
  lines.push("Surfaces watched: fabricd /internal/v1/status + /v1/health, spawn-worker /internal/v1/metrics.");
  return { subject, text: lines.join("\n") };
}

/**
 * Send an alert email via Resend. Default-off + fail-soft: no-ops (logging a
 * line) when unconfigured, swallows transport errors, never logs secrets.
 */
export async function sendAlert(env: NotifyEnv, alerts: Alert[]): Promise<SendResult> {
  if (alerts.length === 0) return { sent: false, reason: "no-alerts" };

  const key = env.RESEND_API_KEY;
  const to = env.ALERT_EMAIL_TO;
  const from = env.ALERT_EMAIL_FROM;
  if (!key || !to || !from) {
    // Default-off: log WHAT would have been sent (never the key), then no-op.
    const missing = [
      !key ? "RESEND_API_KEY" : null,
      !to ? "ALERT_EMAIL_TO" : null,
      !from ? "ALERT_EMAIL_FROM" : null,
    ].filter(Boolean);
    const { subject } = formatAlertEmail(alerts);
    console.log(
      `[canary] email transport not armed (missing: ${missing.join(", ")}); would send: ${subject}`,
    );
    return { sent: false, reason: "not-configured" };
  }

  const { subject, text } = formatAlertEmail(alerts);
  try {
    const resp = await fetch("https://api.resend.com/emails", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${key}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({ from, to, subject, text }),
      signal: AbortSignal.timeout(8000),
    });
    // Always consume the response, including failures, so the Worker can
    // release the connection. The body is intentionally discarded: provider
    // error payloads are untrusted and may contain sensitive request data.
    await resp.text().catch(() => "");
    if (!resp.ok) {
      // Do NOT echo the response body (avoid leaking anything); status only.
      console.log(`[canary] Resend send failed: HTTP ${resp.status}`);
      return { sent: false, reason: `resend-${resp.status}` };
    }
    console.log(`[canary] alert email sent (${alerts.length} alert(s)): ${subject}`);
    return { sent: true };
  } catch (err) {
    // Never throw into the scheduled run.
    console.log(`[canary] Resend send threw: ${err instanceof Error ? err.name : "error"}`);
    return { sent: false, reason: "transport-error" };
  }
}
