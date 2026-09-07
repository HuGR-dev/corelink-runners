export type ComputeReceipt = {
  reservation_id: string;
  state: "prepared" | "active" | "cancelled" | "settled";
  materialized?: boolean;
  actual_vcpu_ms?: string;
  evidence_digest?: string;
  future_materialization_fence?: string;
  terminal_authority?: string;
  authority_signature?: string;
};

export type AuthenticatedTerminalReceipt = {
  reservation_id: string;
  state: "cancelled" | "settled";
  materialized: boolean;
  actual_vcpu_ms: string;
  evidence_digest: string;
  future_materialization_fence: string;
  terminal_authority: string;
  authority_signature: string;
};

export type ComputeBudgetErrorCode =
  | "over_compute"
  | "baseline_or_unavailable"
  | "conflict"
  | "unauthorized"
  | "invalid"
  | "ambiguous";

export class ComputeBudgetClientError extends Error {
  readonly name = "ComputeBudgetClientError";
  constructor(readonly code: ComputeBudgetErrorCode, message: string) {
    super(message);
  }
}

const MAX_TOKEN_BYTES = 8 * 1024;
const MAX_RESPONSE_BYTES = 8 * 1024;
const TIMEOUT_MS = 5_000;
const I64_MAX = 9_223_372_036_854_775_807n;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const DECIMAL = /^(0|[1-9][0-9]*)$/;
const DIGEST = /^[0-9a-f]{64}$/i;
const SIGNATURE = /^[A-Za-z0-9_-]{43,}$/;
const AUTHORITY = /^[A-Za-z0-9._:-]{1,128}$/;

function invalid(message: string): ComputeBudgetClientError {
  return new ComputeBudgetClientError("invalid", message);
}

function validateToken(token: string): void {
  if (typeof token !== "string" || token.length === 0 || new TextEncoder().encode(token).length > MAX_TOKEN_BYTES || /[\r\n]/.test(token)) {
    throw invalid("invalid compute grant");
  }
}

function validateReservationId(id: string): void {
  if (typeof id !== "string" || !UUID.test(id)) throw invalid("invalid reservation id");
}

function validateDecimal(value: string, field: string): void {
  if (typeof value !== "string" || value.length > 19 || !DECIMAL.test(value)) throw invalid(`invalid ${field}`);
  try {
    if (BigInt(value) > I64_MAX) throw invalid(`invalid ${field}`);
  } catch (error) {
    if (error instanceof ComputeBudgetClientError) throw error;
    throw invalid(`invalid ${field}`);
  }
}

async function readBoundedBody(response: Response, signal: AbortSignal): Promise<string> {
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  let onAbort: (() => void) | undefined;
  const aborted = new Promise<never>((_, reject) => {
    onAbort = () => reject(new ComputeBudgetClientError("baseline_or_unavailable", "compute request timed out"));
    if (signal.aborted) onAbort();
    else signal.addEventListener("abort", onAbort, { once: true });
  });
  try {
    for (;;) {
      const part = await Promise.race([reader.read(), aborted]);
      if (part.done) break;
      total += part.value.byteLength;
      if (total > MAX_RESPONSE_BYTES) {
        void reader.cancel("response too large").catch(() => {});
        throw invalid("compute response too large");
      }
      chunks.push(part.value);
    }
  } finally {
    if (onAbort) signal.removeEventListener("abort", onAbort);
    if (signal.aborted) void reader.cancel().catch(() => {});
    reader.releaseLock();
  }
  if (signal.aborted) throw new ComputeBudgetClientError("baseline_or_unavailable", "compute request timed out");
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return new TextDecoder().decode(bytes);
}

type Operation = "reserve" | "activate" | "cancel" | "settle";

function expectedState(operation: Operation): ComputeReceipt["state"][] {
  return operation === "reserve" ? ["prepared", "active"] : [operation === "activate" ? "active" : operation === "cancel" ? "cancelled" : "settled"];
}

export function validateAuthenticatedTerminalReceipt(
  value: unknown,
  reservationId: string,
  expectedState?: AuthenticatedTerminalReceipt["state"],
): AuthenticatedTerminalReceipt {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
  const row = value as Record<string, unknown>;
  const fields = ["reservation_id", "state", "materialized", "actual_vcpu_ms", "evidence_digest", "future_materialization_fence", "terminal_authority", "authority_signature"];
  if (Object.keys(row).length !== fields.length || fields.some(field => !(field in row))) throw new ComputeBudgetClientError("ambiguous", "provider terminal receipt is incomplete");
  if (row.reservation_id !== reservationId || (row.state !== "cancelled" && row.state !== "settled") || (expectedState && row.state !== expectedState)) throw new ComputeBudgetClientError("ambiguous", "provider terminal receipt identity mismatch");
  if (typeof row.materialized !== "boolean" || typeof row.actual_vcpu_ms !== "string" || typeof row.evidence_digest !== "string" || typeof row.future_materialization_fence !== "string" || typeof row.terminal_authority !== "string" || typeof row.authority_signature !== "string") throw new ComputeBudgetClientError("ambiguous", "provider terminal receipt is malformed");
  if (!DIGEST.test(row.evidence_digest) || !DIGEST.test(row.future_materialization_fence) || !AUTHORITY.test(row.terminal_authority) || !SIGNATURE.test(row.authority_signature)) throw new ComputeBudgetClientError("ambiguous", "provider terminal receipt is unauthenticated");
  validateDecimal(row.actual_vcpu_ms, "actual_vcpu_ms");
  if (row.state === "cancelled" && (row.materialized !== false || row.actual_vcpu_ms !== "0")) throw new ComputeBudgetClientError("ambiguous", "cancel receipt permits materialization");
  if (row.state === "settled" && row.materialized !== true) throw new ComputeBudgetClientError("ambiguous", "settlement receipt is not materialized");
  return row as AuthenticatedTerminalReceipt;
}

export class ComputeBudgetClient {
  private readonly origin: string;
  private readonly fetcher: typeof fetch;

  constructor(endpoint: string, fetcher: typeof fetch = fetch) {
    let parsed: URL;
    try { parsed = new URL(endpoint); } catch { throw invalid("compute endpoint must be an HTTPS origin"); }
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash || (parsed.pathname !== "/" && parsed.pathname !== "")) {
      throw invalid("compute endpoint must be an HTTPS origin");
    }
    this.origin = parsed.origin;
    this.fetcher = fetcher;
  }

  reserve(token: string, reservationId: string): Promise<ComputeReceipt> { return this.call("reserve", token, reservationId); }
  activate(token: string, reservationId: string): Promise<ComputeReceipt> { return this.call("activate", token, reservationId); }
  cancel(token: string, reservationId: string): Promise<AuthenticatedTerminalReceipt> { return this.call("cancel", token, reservationId); }
  async settle(token: string, reservationId: string, actualVcpuMs: string, terminalEvidenceDigest: string): Promise<AuthenticatedTerminalReceipt> {
    validateDecimal(actualVcpuMs, "actual_vcpu_ms");
    if (typeof terminalEvidenceDigest !== "string" || !DIGEST.test(terminalEvidenceDigest)) throw invalid("invalid terminal evidence digest");
    return this.call("settle", token, reservationId, { actual_vcpu_ms: actualVcpuMs, terminal_evidence_digest: terminalEvidenceDigest });
  }

  private call(operation: "reserve" | "activate", token: string, reservationId: string, body?: Record<string, string>): Promise<ComputeReceipt>;
  private call(operation: "cancel" | "settle", token: string, reservationId: string, body?: Record<string, string>): Promise<AuthenticatedTerminalReceipt>;
  private async call(operation: Operation, token: string, reservationId: string, body: Record<string, string> = {}): Promise<ComputeReceipt | AuthenticatedTerminalReceipt> {
    validateToken(token); validateReservationId(reservationId);
    const controller = new AbortController();
    let timeoutReject!: (error: ComputeBudgetClientError) => void;
    const timeout = new Promise<never>((_, reject) => { timeoutReject = reject; });
    const timer = setTimeout(() => {
      controller.abort();
      timeoutReject(new ComputeBudgetClientError("ambiguous", "compute result is ambiguous"));
    }, TIMEOUT_MS);
    try {
      let response: Response;
      try {
        response = await Promise.race([this.fetcher(new URL(`/internal/v1/compute/${operation}`, this.origin).toString(), {
          method: "POST", redirect: "error", signal: controller.signal,
          headers: { Authorization: `ComputeGrant ${token}`, "content-type": "application/json" },
          body: JSON.stringify(body),
        }), timeout]);
      } catch {
        throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
      }
      let text: string;
      try { text = await readBoundedBody(response, controller.signal); }
      catch (error) {
        throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
      }
      if (response.status !== 200) {
        const code: ComputeBudgetErrorCode = response.status === 429 ? "over_compute" : response.status === 401 ? "unauthorized" : response.status === 409 ? "conflict" : response.status === 503 ? "baseline_or_unavailable" : response.status >= 500 ? "baseline_or_unavailable" : "invalid";
        throw new ComputeBudgetClientError(code, "compute request rejected");
      }
      let parsed: unknown;
      try { parsed = JSON.parse(text); } catch { throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous"); }
      if (!parsed || typeof parsed !== "object") throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
      const value = parsed as Record<string, unknown>;
      if (operation === "cancel" || operation === "settle") return validateAuthenticatedTerminalReceipt(value, reservationId, operation === "cancel" ? "cancelled" : "settled");
      if (Object.keys(value).length !== 2 || value.reservation_id !== reservationId || typeof value.state !== "string" || !expectedState(operation).includes(value.state as ComputeReceipt["state"])) throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
      return { reservation_id: reservationId, state: value.state as ComputeReceipt["state"] };
    } catch (error) {
      if (error instanceof ComputeBudgetClientError) throw error;
      throw new ComputeBudgetClientError("ambiguous", "compute result is ambiguous");
    } finally { clearTimeout(timer); }
  }
}
