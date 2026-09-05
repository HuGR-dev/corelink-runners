export interface ContainmentMetaShape {
  schema_version: 1; next_pause_seq: number; drain_cursor: number; backlog_count: number; lease_epoch: number;
  lease: { owner: string; epoch: number; expires_ms: number } | null; drain_requested: boolean;
}
export interface ContainmentJobIndex { schema_version: 1; repo: string; job_id: string; event_ids: string[] }
export interface ContainmentJobIndexMeta { schema_version: 1; initialized: true }
export interface ContainmentOutboxShape { schema_version: 1; signal_id: string; state: "PENDING" | "DELIVERED"; attempts: number }
export interface InvalidConfigShape { schema_version: 1; signal_id: string; switch_name: string; raw_value_sha256: string }
export interface RedriveReservationShape {
  schema_version: 1; repo: string; job_id: string; owner: string; token: string; epoch: number; path: "redrive";
  state: "HELD" | "EFFECT_ELIGIBLE" | "COMPLETED"; expires_ms: number; event_id: string | null; effect_id: string; completion_observed: boolean;
}
export interface RedrivePermitShape { schema_version: 1; repo: string; job_id: string; owner: string; token: string; epoch: number; path: "redrive"; effect_id: string }

const EVENT_PREFIX = "containment:v1:event:";
const PAUSE_PREFIX = "containment:v1:pause:";
const INVALID_PREFIX = "containment:v1:invalid:";
const OUTBOX_PREFIX = "containment:v1:outbox:";
const RESERVATION_PREFIX = "containment:v1:reservation:";
const INDEX_PREFIX = "containment:v1:job-index:";
const MAX_INDEX_EVENTS = 32;
const SHA256_HEX = /^[0-9a-f]{64}$/;
const SWITCHES = new Set(["AUTOSCALER_INTAKE_PAUSED", "AUTOSCALER_REDRIVE_PAUSED"]);
function trimAscii(value: string): string { return value.replace(/^[\u0009-\u000d\u0020]+|[\u0009-\u000d\u0020]+$/g, ""); }
export function emptyContainmentMeta(): ContainmentMetaShape { return { schema_version: 1, next_pause_seq: 1, drain_cursor: 0, backlog_count: 0, lease_epoch: 0, lease: null, drain_requested: false }; }
export function containmentEventKey(id: string): string { return `${EVENT_PREFIX}${id}`; }
export function containmentPauseKey(seq: number): string { return `${PAUSE_PREFIX}${String(seq).padStart(20, "0")}`; }
export function containmentInvalidKey(name: string, digest: string): string { return `${INVALID_PREFIX}${name}:${digest}`; }
export function containmentOutboxKey(signalId: string): string { return `${OUTBOX_PREFIX}${signalId}`; }
export function containmentJobIndexKey(repo: string, jobId: string): string { return `${INDEX_PREFIX}${repo}/${jobId}`; }
export function containmentReservationKey(repo: string, jobId: string): string { return `${RESERVATION_PREFIX}${repo}/${jobId}`; }
export function redriveEffectId(repo: string, jobId: string): string { return `containment:v1:redrive:${repo}/${jobId}`; }
export function isValidJobIndex(value: unknown, repo: string, jobId: string): value is ContainmentJobIndex {
  if (!value || typeof value !== "object") return false; const r = value as Partial<ContainmentJobIndex>;
  return r.schema_version === 1 && r.repo === repo && r.job_id === jobId && Array.isArray(r.event_ids) && r.event_ids.length <= MAX_INDEX_EVENTS
    && new Set(r.event_ids).size === r.event_ids.length && r.event_ids.every((id) => typeof id === "string" && id.length > 0);
}
export function isValidJobIndexMeta(value: unknown): value is ContainmentJobIndexMeta {
  return !!value && typeof value === "object" && (value as Partial<ContainmentJobIndexMeta>).schema_version === 1 && (value as Partial<ContainmentJobIndexMeta>).initialized === true;
}
export function isValidOutboxRecord(value: unknown): value is ContainmentOutboxShape {
  if (!value || typeof value !== "object") return false; const r = value as Partial<ContainmentOutboxShape>;
  return r.schema_version === 1 && typeof r.signal_id === "string" && SHA256_HEX.test(r.signal_id) && (r.state === "PENDING" || r.state === "DELIVERED") && Number.isSafeInteger(r.attempts) && (r.attempts as number) >= 0;
}
export function isValidInvalidConfigRecord(value: unknown, key: string): value is InvalidConfigShape {
  if (!value || typeof value !== "object") return false; const r = value as Partial<InvalidConfigShape>;
  return r.schema_version === 1 && typeof r.switch_name === "string" && SWITCHES.has(r.switch_name) && typeof r.raw_value_sha256 === "string" && SHA256_HEX.test(r.raw_value_sha256)
    && typeof r.signal_id === "string" && SHA256_HEX.test(r.signal_id) && key === containmentInvalidKey(r.switch_name, r.raw_value_sha256);
}
export function canonicalSafeJobId(jobId: unknown): string | null {
  if (typeof jobId !== "string") return null; const normalized = trimAscii(jobId); if (!/^[0-9]+$/.test(normalized)) return null;
  try { const numeric = BigInt(normalized); return numeric > BigInt(Number.MAX_SAFE_INTEGER) ? null : numeric.toString(10); } catch { return null; }
}
export function normalizeRedriveIdentity(repo: unknown, jobId: unknown): { repo: string; job_id: string } | null {
  if (typeof repo !== "string") return null; const normalizedRepo = trimAscii(repo).toLowerCase();
  if (!/^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?\/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$/.test(normalizedRepo)) return null;
  const normalizedJobId = canonicalSafeJobId(jobId); return normalizedJobId === null ? null : { repo: normalizedRepo, job_id: normalizedJobId };
}
export function reservationPermit(reservation: RedriveReservationShape): RedrivePermitShape {
  return { schema_version: 1, repo: reservation.repo, job_id: reservation.job_id, owner: reservation.owner, token: reservation.token, epoch: reservation.epoch, path: "redrive", effect_id: reservation.effect_id };
}
export function reservationTupleMatches(reservation: RedriveReservationShape, repo: string, jobId: string, owner: string, token: string, epoch: number, path: "redrive", effectId: string): boolean {
  return reservation.schema_version === 1 && reservation.repo === repo && reservation.job_id === jobId && reservation.owner === owner && reservation.token === token && reservation.epoch === epoch
    && reservation.path === path && reservation.effect_id === effectId && typeof reservation.completion_observed === "boolean";
}
