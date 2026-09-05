import type { KvLike } from "./lib";

export type ContainmentEffectState =
  | "PREPARED" | "CLAIM_ACQUIRED" | "PERMIT_ISSUED" | "BOUND"
  | "DRIVING" | "COMMITTED" | "ABORTED_PRE_EFFECT";
export interface ContainmentEffectIdentity { repo: string; job_id: string; effect_id: string }
export interface ContainmentEffectBinding {
  schema_version: 1;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  binding_sha256: string;
}
export interface ContainmentEffectPermit {
  schema_version: 1;
  repo: string;
  job_id: string;
  effect_id: string;
  nonce: string;
  owner: string;
  owner_token: string;
  lease_epoch: number;
  permit_id: string;
}
export interface ContainmentEffectReceipt {
  schema_version: 1;
  trusted: true;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  nonce: string;
  permit_id: string;
  binding_sha256: string;
  receipt_id: string;
  receipt_sha256: string;
  provider_signature: string;
}
export interface ContainmentEffectAttempt {
  schema_version: 1;
  repo: string;
  job_id: string;
  effect_id: string;
  nonce: string;
  owner: string;
  owner_token: string;
  lease_epoch: number;
  state: ContainmentEffectState;
  created_at_ms: number;
  updated_at_ms: number;
  permit: ContainmentEffectPermit | null;
  binding: ContainmentEffectBinding | null;
  provider_receipt: ContainmentEffectReceipt | null;
}
export interface ContainmentEffectMirror {
  schema_version: 1;
  repo: string;
  job_id: string;
  effect_id: string;
  nonce: string;
  owner: string;
  owner_token: string;
  lease_epoch: number;
  state: Exclude<ContainmentEffectState, "ABORTED_PRE_EFFECT">;
  permit_id: string | null;
  binding_sha256: string | null;
}
interface ContainmentEffectPointer {
  schema_version: 1;
  repo: string;
  job_id: string;
  effect_id: string;
  active_nonce: string | null;
  terminal: "COMMITTED" | null;
}
export interface ContainmentEffectPrepareInput { identity: ContainmentEffectIdentity; owner: string; lease_epoch: number; now?: number }
export interface ContainmentEffectTransition {
  identity: ContainmentEffectIdentity;
  nonce: string;
  owner: string;
  owner_token: string;
  lease_epoch: number;
  now?: number;
}
export interface ContainmentEffectReapInput {
  identity: ContainmentEffectIdentity;
  nonce: string;
  authority: "containment-reaper-v1";
  lease_epoch: number;
  stale_after_ms: number;
  now?: number;
}
export type ContainmentEffectResult =
  | { status: "prepared" | "active" | "committed"; attempt: ContainmentEffectAttempt }
  | { status: "transitioned"; attempt: ContainmentEffectAttempt }
  | { status: "aborted" | "reaped"; attempt: ContainmentEffectAttempt }
  | { status: "stale" | "busy" | "invalid" | "unknown_terminal" | "mirror_unavailable" | "mirror_mismatch" };

const POINTER_PREFIX = "containment:v1:effect-owner:";
const ATTEMPT_PREFIX = "containment:v1:effect-attempt:";
const SHA256_HEX = /^[0-9a-f]{64}$/;
const REPO = /^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?\/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$/;

interface EffectStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
  transaction<T>(fn: (storage: EffectStorage) => Promise<T>): Promise<T>;
}
function trimAscii(value: string): string { return value.replace(/^[\u0009-\u000d\u0020]+|[\u0009-\u000d\u0020]+$/g, ""); }
function normalizeIdentity(input: ContainmentEffectIdentity): ContainmentEffectIdentity | null {
  if (!input || typeof input.repo !== "string" || typeof input.job_id !== "string" || typeof input.effect_id !== "string") return null;
  const repo = trimAscii(input.repo).toLowerCase(); const job = trimAscii(input.job_id);
  if (!REPO.test(repo) || !/^[1-9][0-9]*$/.test(job)) return null;
  try { if (BigInt(job) > BigInt(Number.MAX_SAFE_INTEGER)) return null; } catch { return null; }
  if (input.effect_id.length === 0 || input.effect_id.length > 256 || /[\u0000\n\r]/.test(input.effect_id)) return null;
  return { repo, job_id: String(BigInt(job)), effect_id: input.effect_id };
}
export function containmentEffectPointerKey(identityInput: ContainmentEffectIdentity): string {
  const identity = normalizeIdentity(identityInput); if (!identity) return "";
  return `${POINTER_PREFIX}${encodeURIComponent(identity.repo)}/${encodeURIComponent(identity.job_id)}/${encodeURIComponent(identity.effect_id)}`;
}
function attemptKey(identity: ContainmentEffectIdentity, nonce: string): string {
  return `${ATTEMPT_PREFIX}${encodeURIComponent(identity.repo)}/${encodeURIComponent(identity.job_id)}/${encodeURIComponent(identity.effect_id)}/${encodeURIComponent(nonce)}`;
}
function state(value: unknown): value is ContainmentEffectState {
  return typeof value === "string" && ["PREPARED", "CLAIM_ACQUIRED", "PERMIT_ISSUED", "BOUND", "DRIVING", "COMMITTED", "ABORTED_PRE_EFFECT"].includes(value);
}
function validBinding(value: unknown): value is ContainmentEffectBinding {
  if (!value || typeof value !== "object") return false; const b = value as Partial<ContainmentEffectBinding>;
  return b.schema_version === 1 && typeof b.provider === "string" && b.provider.length > 0 && typeof b.resource_id === "string" && b.resource_id.length > 0
    && typeof b.idempotency_key === "string" && b.idempotency_key.length > 0 && typeof b.binding_sha256 === "string" && SHA256_HEX.test(b.binding_sha256);
}
function validPermit(value: unknown, attempt: ContainmentEffectAttempt): value is ContainmentEffectPermit {
  if (!value || typeof value !== "object") return false; const p = value as Partial<ContainmentEffectPermit>;
  return p.schema_version === 1 && p.repo === attempt.repo && p.job_id === attempt.job_id && p.effect_id === attempt.effect_id && p.nonce === attempt.nonce
    && p.owner === attempt.owner && p.owner_token === attempt.owner_token && p.lease_epoch === attempt.lease_epoch && typeof p.permit_id === "string" && p.permit_id.length > 0;
}
function validReceipt(value: unknown, attempt: ContainmentEffectAttempt): value is ContainmentEffectReceipt {
  if (!value || typeof value !== "object" || !attempt.binding || !attempt.permit) return false; const r = value as Partial<ContainmentEffectReceipt>; const b = attempt.binding;
  return r.schema_version === 1 && r.trusted === true && r.provider === b.provider && r.resource_id === b.resource_id && r.idempotency_key === b.idempotency_key
    && r.nonce === attempt.nonce && r.permit_id === attempt.permit.permit_id && r.binding_sha256 === b.binding_sha256
    && typeof r.receipt_id === "string" && r.receipt_id.length > 0 && typeof r.receipt_sha256 === "string" && SHA256_HEX.test(r.receipt_sha256)
    && typeof r.provider_signature === "string" && r.provider_signature.length > 0;
}
function validAttempt(value: unknown, identity: ContainmentEffectIdentity, nonce: string): value is ContainmentEffectAttempt {
  if (!value || typeof value !== "object") return false; const a = value as Partial<ContainmentEffectAttempt>; const candidate = a as ContainmentEffectAttempt;
  if (a.schema_version !== 1 || a.repo !== identity.repo || a.job_id !== identity.job_id || a.effect_id !== identity.effect_id || a.nonce !== nonce
    || typeof a.owner !== "string" || a.owner.length === 0 || typeof a.owner_token !== "string" || a.owner_token.length === 0
    || !Number.isSafeInteger(a.lease_epoch) || (a.lease_epoch as number) < 1 || !state(a.state) || !Number.isFinite(a.created_at_ms) || !Number.isFinite(a.updated_at_ms)) return false;
  const permit = a.permit; const binding = a.binding; const receipt = a.provider_receipt;
  if (a.state === "PREPARED" || a.state === "CLAIM_ACQUIRED") return permit === null && binding === null && receipt === null;
  if (a.state === "PERMIT_ISSUED") return permit !== null && validPermit(permit, candidate) && binding === null && receipt === null;
  if (a.state === "BOUND" || a.state === "DRIVING") return permit !== null && validPermit(permit, candidate) && binding !== null && validBinding(binding) && receipt === null;
  if (a.state === "COMMITTED") return permit !== null && validPermit(permit, candidate) && binding !== null && validBinding(binding) && receipt !== null && validReceipt(receipt, candidate);
  return receipt === null && (permit === null || validPermit(permit, candidate)) && (binding === null || validBinding(binding));
}
function validPointer(value: unknown, identity: ContainmentEffectIdentity): value is ContainmentEffectPointer {
  if (!value || typeof value !== "object") return false; const p = value as Partial<ContainmentEffectPointer>;
  return p.schema_version === 1 && p.repo === identity.repo && p.job_id === identity.job_id && p.effect_id === identity.effect_id
    && (p.active_nonce === null || (typeof p.active_nonce === "string" && p.active_nonce.length > 0))
    && (p.terminal === null || (p.terminal === "COMMITTED" && p.active_nonce !== null));
}
function mirrorFor(attempt: ContainmentEffectAttempt): ContainmentEffectMirror | null {
  if (attempt.state === "ABORTED_PRE_EFFECT") return null;
  return { schema_version: 1, repo: attempt.repo, job_id: attempt.job_id, effect_id: attempt.effect_id, nonce: attempt.nonce, owner: attempt.owner, owner_token: attempt.owner_token,
    lease_epoch: attempt.lease_epoch, state: attempt.state, permit_id: attempt.permit?.permit_id ?? null, binding_sha256: attempt.binding?.binding_sha256 ?? null };
}
function mirrorMatches(attempt: ContainmentEffectAttempt, mirror: ContainmentEffectMirror | null): boolean { const expected = mirrorFor(attempt); return !!expected && !!mirror && JSON.stringify(expected) === JSON.stringify(mirror); }

export function containmentEffectMirrorKey(identity: ContainmentEffectIdentity): string { return containmentEffectPointerKey(identity); }
export function containmentEffectMirrorFromAttempt(attempt: ContainmentEffectAttempt): ContainmentEffectMirror | null { return mirrorFor(attempt); }
export async function readContainmentEffectMirror(kv: KvLike | undefined, identityInput: ContainmentEffectIdentity): Promise<ContainmentEffectMirror | null> {
  const identity = normalizeIdentity(identityInput); if (!identity || !kv) return null;
  const raw = await kv.get(containmentEffectMirrorKey(identity)); if (!raw) return null;
  try {
    const value = JSON.parse(raw) as ContainmentEffectMirror;
    return value.schema_version === 1 && value.repo === identity.repo && value.job_id === identity.job_id && value.effect_id === identity.effect_id
      && typeof value.nonce === "string" && typeof value.owner === "string" && typeof value.owner_token === "string" && Number.isSafeInteger(value.lease_epoch)
      && state(value.state) && value.state !== "ABORTED_PRE_EFFECT" && (value.permit_id === null || typeof value.permit_id === "string")
      && (value.binding_sha256 === null || SHA256_HEX.test(value.binding_sha256)) ? value : null;
  } catch { return null; }
}

export class ContainmentEffectLedger {
  constructor(private readonly storage: EffectStorage, private readonly kv?: KvLike) {}
  async getEffectAttempt(identityInput: ContainmentEffectIdentity, nonce: string): Promise<ContainmentEffectAttempt | null> {
    const identity = normalizeIdentity(identityInput); if (!identity || !nonce) return null;
    const value = await this.storage.get<ContainmentEffectAttempt>(attemptKey(identity, nonce)); return value && validAttempt(value, identity, nonce) ? value : null;
  }
  async prepareEffect(input: ContainmentEffectPrepareInput): Promise<ContainmentEffectResult> {
    const identity = normalizeIdentity(input.identity); if (!identity || !input.owner || !Number.isSafeInteger(input.lease_epoch) || input.lease_epoch < 1) return { status: "invalid" }; const now = input.now ?? Date.now();
    return this.storage.transaction(async (s) => {
      const raw = await s.get<ContainmentEffectPointer>(containmentEffectPointerKey(identity));
      if (raw !== undefined && !validPointer(raw, identity)) return { status: "busy" as const };
      if (raw?.active_nonce) {
        const current = await s.get<ContainmentEffectAttempt>(attemptKey(identity, raw.active_nonce));
        if (!current || !validAttempt(current, identity, raw.active_nonce)) return { status: "busy" as const };
        if ((raw.terminal === "COMMITTED") !== (current.state === "COMMITTED")) return { status: "busy" as const };
        if (current.state === "COMMITTED") return { status: "committed" as const, attempt: current };
        if (current.state === "DRIVING") return { status: "unknown_terminal" as const };
        return { status: "active" as const, attempt: current };
      }
      const attempt: ContainmentEffectAttempt = { schema_version: 1, ...identity, nonce: crypto.randomUUID(), owner: input.owner, owner_token: crypto.randomUUID(), lease_epoch: input.lease_epoch,
        state: "PREPARED", created_at_ms: now, updated_at_ms: now, permit: null, binding: null, provider_receipt: null };
      await s.put(attemptKey(identity, attempt.nonce), attempt); await s.put(containmentEffectPointerKey(identity), { schema_version: 1, ...identity, active_nonce: attempt.nonce, terminal: null } satisfies ContainmentEffectPointer);
      return { status: "prepared" as const, attempt };
    });
  }
  private status(attempt: ContainmentEffectAttempt): ContainmentEffectResult {
    if (attempt.state === "COMMITTED") return { status: "committed", attempt }; if (attempt.state === "DRIVING") return { status: "unknown_terminal" }; return { status: "busy" };
  }
  private async owned(s: EffectStorage, input: ContainmentEffectTransition): Promise<{ identity: ContainmentEffectIdentity; pointer: ContainmentEffectPointer; attempt: ContainmentEffectAttempt } | null> {
    const identity = normalizeIdentity(input.identity); if (!identity || !input.nonce || !input.owner || !input.owner_token || !Number.isSafeInteger(input.lease_epoch)) return null;
    const pointer = await s.get<ContainmentEffectPointer>(containmentEffectPointerKey(identity)); if (!pointer || !validPointer(pointer, identity) || pointer.active_nonce !== input.nonce) return null;
    const attempt = await s.get<ContainmentEffectAttempt>(attemptKey(identity, input.nonce));
    if (!attempt || !validAttempt(attempt, identity, input.nonce) || attempt.owner !== input.owner || attempt.owner_token !== input.owner_token || attempt.lease_epoch !== input.lease_epoch) return null;
    if ((pointer.terminal === "COMMITTED") !== (attempt.state === "COMMITTED")) return null;
    return { identity, pointer, attempt };
  }
  async acquireEffectClaim(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> {
    const now = input.now ?? Date.now(); return this.storage.transaction(async (s) => { const owned = await this.owned(s, input); if (!owned) return { status: "stale" as const }; if (owned.attempt.state !== "PREPARED") return this.status(owned.attempt);
      const attempt = { ...owned.attempt, state: "CLAIM_ACQUIRED" as const, updated_at_ms: now }; await s.put(attemptKey(owned.identity, input.nonce), attempt); return { status: "transitioned" as const, attempt }; });
  }
  async issueEffectPermit(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> {
    const now = input.now ?? Date.now(); return this.storage.transaction(async (s) => { const owned = await this.owned(s, input); if (!owned) return { status: "stale" as const }; if (owned.attempt.state === "PERMIT_ISSUED" || owned.attempt.state === "BOUND") return { status: "transitioned" as const, attempt: owned.attempt }; if (owned.attempt.state !== "CLAIM_ACQUIRED") return this.status(owned.attempt);
      const permit: ContainmentEffectPermit = { schema_version: 1, ...owned.identity, nonce: input.nonce, owner: input.owner, owner_token: input.owner_token, lease_epoch: input.lease_epoch, permit_id: crypto.randomUUID() }; const attempt = { ...owned.attempt, state: "PERMIT_ISSUED" as const, permit, updated_at_ms: now }; await s.put(attemptKey(owned.identity, input.nonce), attempt); return { status: "transitioned" as const, attempt }; });
  }
  async bindEffect(input: ContainmentEffectTransition & { permit_id: string; binding: ContainmentEffectBinding }): Promise<ContainmentEffectResult> {
    const now = input.now ?? Date.now(); if (!validBinding(input.binding) || !input.permit_id) return { status: "invalid" }; return this.storage.transaction(async (s) => { const owned = await this.owned(s, input); if (!owned || !owned.attempt.permit || !validPermit(owned.attempt.permit, owned.attempt) || owned.attempt.permit.permit_id !== input.permit_id) return { status: "stale" as const }; if (owned.attempt.state === "BOUND" && JSON.stringify(owned.attempt.binding) === JSON.stringify(input.binding)) return { status: "transitioned" as const, attempt: owned.attempt }; if (owned.attempt.state !== "PERMIT_ISSUED") return this.status(owned.attempt);
      const attempt = { ...owned.attempt, state: "BOUND" as const, binding: input.binding, updated_at_ms: now }; await s.put(attemptKey(owned.identity, input.nonce), attempt); return { status: "transitioned" as const, attempt }; });
  }
  async beginEffectDrive(input: ContainmentEffectTransition & { permit_id: string }): Promise<ContainmentEffectResult> {
    const identity = normalizeIdentity(input.identity); if (!identity || !this.kv) return { status: "mirror_unavailable" }; let mirror: ContainmentEffectMirror | null; try { mirror = await readContainmentEffectMirror(this.kv, identity); } catch { return { status: "mirror_unavailable" }; } if (!mirror) return { status: "mirror_mismatch" };
    const now = input.now ?? Date.now(); return this.storage.transaction(async (s) => { const owned = await this.owned(s, input); if (!owned) return { status: "stale" as const }; if (owned.attempt.state === "DRIVING") return { status: "unknown_terminal" as const }; if (owned.attempt.state === "COMMITTED") return { status: "committed" as const, attempt: owned.attempt }; if (owned.attempt.state !== "BOUND" || !owned.attempt.permit || owned.attempt.permit.permit_id !== input.permit_id || !owned.attempt.binding) return this.status(owned.attempt); if (!mirrorMatches(owned.attempt, mirror)) return { status: "mirror_mismatch" as const };
      const attempt = { ...owned.attempt, state: "DRIVING" as const, updated_at_ms: now }; await s.put(attemptKey(owned.identity, input.nonce), attempt); return { status: "transitioned" as const, attempt }; });
  }
  async commitEffect(input: ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }): Promise<ContainmentEffectResult> {
    const identity = normalizeIdentity(input.identity); if (!identity || !input.receipt) return { status: "invalid" }; if (!this.kv) return { status: "mirror_unavailable" }; let mirror: ContainmentEffectMirror | null; try { mirror = await readContainmentEffectMirror(this.kv, identity); } catch { return { status: "mirror_unavailable" }; } if (!mirror) return { status: "mirror_mismatch" }; const now = input.now ?? Date.now();
    return this.storage.transaction(async (s) => { const owned = await this.owned(s, input); if (!owned) return { status: "stale" as const }; if (owned.attempt.state === "COMMITTED") return { status: "committed" as const, attempt: owned.attempt }; if (owned.attempt.state !== "DRIVING" || !owned.attempt.permit || !owned.attempt.binding || owned.attempt.permit.permit_id !== input.permit_id) return this.status(owned.attempt); if (!mirrorMatches(owned.attempt, mirror) || !validReceipt(input.receipt, owned.attempt)) return { status: "mirror_mismatch" as const };
      const attempt = { ...owned.attempt, state: "COMMITTED" as const, provider_receipt: input.receipt, updated_at_ms: now }; await s.put(attemptKey(owned.identity, input.nonce), attempt); await s.put(containmentEffectPointerKey(owned.identity), { ...owned.pointer, active_nonce: input.nonce, terminal: "COMMITTED" } satisfies ContainmentEffectPointer); return { status: "committed" as const, attempt }; });
  }
  async abortEffect(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> { return this.abortOrReap(input, "aborted"); }
  async reapEffect(input: ContainmentEffectReapInput): Promise<ContainmentEffectResult> {
    if (input.authority !== "containment-reaper-v1" || !Number.isSafeInteger(input.lease_epoch) || !Number.isSafeInteger(input.stale_after_ms) || input.stale_after_ms < 1) return { status: "invalid" };
    return this.abortOrReap({ identity: input.identity, nonce: input.nonce, owner: "__reaper__", owner_token: "__reaper__", lease_epoch: input.lease_epoch, now: input.now }, "reaped", input.stale_after_ms, true);
  }
  private async abortOrReap(input: ContainmentEffectTransition, result: "aborted" | "reaped", staleAfterMs = 0, reaper = false): Promise<ContainmentEffectResult> {
    const now = input.now ?? Date.now(); return this.storage.transaction(async (s) => { const identity = normalizeIdentity(input.identity); if (!identity) return { status: "invalid" as const }; const pointer = await s.get<ContainmentEffectPointer>(containmentEffectPointerKey(identity)); if (!pointer || !validPointer(pointer, identity) || pointer.active_nonce !== input.nonce) return { status: "stale" as const }; const attempt = await s.get<ContainmentEffectAttempt>(attemptKey(identity, input.nonce)); if (!attempt || !validAttempt(attempt, identity, input.nonce) || attempt.lease_epoch !== input.lease_epoch) return { status: "stale" as const };
      if (!reaper && (attempt.owner !== input.owner || attempt.owner_token !== input.owner_token)) return { status: "stale" as const }; if (!["PREPARED", "CLAIM_ACQUIRED", "PERMIT_ISSUED", "BOUND"].includes(attempt.state)) return this.status(attempt); if (!reaper && attempt.state === "BOUND") return { status: "busy" as const }; if (reaper && now < attempt.updated_at_ms + staleAfterMs) return { status: "busy" as const };
      const tombstone = { ...attempt, state: "ABORTED_PRE_EFFECT" as const, updated_at_ms: now }; await s.put(attemptKey(identity, input.nonce), tombstone); await s.put(containmentEffectPointerKey(identity), { ...pointer, active_nonce: null, terminal: null } satisfies ContainmentEffectPointer); return { status: result, attempt: tombstone }; });
  }
}
