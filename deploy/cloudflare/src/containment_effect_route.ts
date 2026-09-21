import {
  containmentSpawnActiveKey,
  containmentSpawnAttemptKey,
  containmentSpawnMirrorKey,
  type ContainmentEffectBinding,
  type ContainmentEffectReceipt,
  type ContainmentEffectPermit,
  type OwnerResult,
  type OwnerTuple,
  type SpawnOwnerRequest,
  type SpawnMirrorObservation,
  type ContainmentEffectLedger,
} from "./containment_effect_ledger";

export interface ProviderDriveReceipt {
  resource_id: string;
  receipt_id: string;
  provider_signature: string;
}

export interface ProviderNoEffectRefusal {
  status: "refused";
  no_effect: true;
  reason?: string;
}
export interface LegacyPermit {
  permit_id: string;
  issued_to_owner: string;
  issued_to_epoch: number;
}

export type ProviderDriveResult = ProviderDriveReceipt | ProviderNoEffectRefusal | void;

export interface CanonicalEffectRouteDeps<TOpts extends object> {
  ledger: {
    ownerPrepare: (request: SpawnOwnerRequest) => Promise<OwnerResult>;
    ownerAcquire: (request: SpawnOwnerRequest) => Promise<OwnerResult>;
    ownerMirror: (request: SpawnOwnerRequest, result?: "acquired" | "owned") => Promise<SpawnMirrorObservation>;
    ownerConfirm: (request: SpawnOwnerRequest, mirrorDigest: string, readbackDigest: string, permitId?: string) => Promise<OwnerResult>;
    ownerBegin: (request: SpawnOwnerRequest, permitId: string) => Promise<OwnerResult>;
    ownerBind: (request: SpawnOwnerRequest, permitId: string, proofId: string, binding: ContainmentEffectBinding) => Promise<OwnerResult>;
    ownerMarkDriving: (request: SpawnOwnerRequest, permitId: string, proofId: string) => Promise<OwnerResult>;
    ownerCommit: (request: SpawnOwnerRequest, permitId: string, proofId: string, receipt: ContainmentEffectReceipt) => Promise<OwnerResult>;
    ownerObserve: (pointerKey: string, attemptKey: string) => Promise<OwnerResult>;
    ownerAbort: (request: SpawnOwnerRequest) => Promise<OwnerResult>;
    ownerFreeze: (request: SpawnOwnerRequest) => Promise<OwnerResult>;
  };
  tuple: OwnerTuple;
  opts: TOpts;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  claim: () => Promise<boolean>;
  release?: () => Promise<void>;
  /** Authority-only admission fence; runs before every mutable external seam. */
  admit?: () => Promise<boolean>;
  /** Only the drain path may replace a validated predecessor after strict admission. */
  allowFencedDrainPredecessor?: boolean;
  beforeClaim?: () => Promise<void>;
  /**
   * Undo credentials prepared by this invocation when the provider was never
   * authorized to start. This deliberately has no claim or slot authority:
   * another invocation may now own either resource for the same job.
   */
  abandonPreparation?: () => Promise<void>;
  beforeDrive?: () => Promise<boolean>;
  /** Legacy drain persists its recovery-only permit before owner PERMIT_ISSUED. */
  beforeConfirm?: (permitId: string) => Promise<LegacyPermit | null | undefined>;
  beforeBegin?: (permit: ContainmentEffectPermit) => Promise<boolean>;
  drive: (opts: TOpts & {
    containment_event_id: string;
    effect_id: string;
    effect_permit_id: string;
    effect_proof_id: string;
    effect_binding: ContainmentEffectBinding;
  }) => Promise<ProviderDriveResult>;
  finalize?: (receipt: ContainmentEffectReceipt) => Promise<boolean>;
}

export type CanonicalEffectRouteResult =
  | { status: "committed"; receipt: ContainmentEffectReceipt; finalized: boolean }
  | { status: "busy" | "unauthorized" | "mirror_tampered" | "claim_refused" | "before_drive_refused" | "provider_refused" | "unavailable"; reason?: string }
  | { status: "unknown_terminal"; retryable?: true; reason?: string };

const hex = /^[0-9a-f]{64}$/;
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0;

async function sha256(value: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function legacyPermitCandidate(tuple: OwnerTuple): Promise<string> {
  const digest = await sha256(JSON.stringify({ domain: "corelink:containment-legacy-permit:v1", tuple, token: tuple.token, caller_nonce: tuple.caller_nonce }));
  return `containment:v1:legacy-permit:${digest}`;
}

function request(tuple: OwnerTuple): SpawnOwnerRequest {
  return { schema_version: 1, tuple, caller_nonce: tuple.caller_nonce };
}

function receiptFrom(record: OwnerResult["record"]): ContainmentEffectReceipt | null {
  const value = record && (record as unknown as { effect_observation?: ContainmentEffectReceipt }).effect_observation;
  return value && value.trusted === true ? value : null;
}

function retryableUnknownTerminal(reason?: string): CanonicalEffectRouteResult {
  return { status: "unknown_terminal", retryable: true, ...(reason ? { reason } : {}) };
}

function terminal(result: OwnerResult): CanonicalEffectRouteResult | null {
  if (result.kind === "missing") return null;
  if (result.kind === "reaped_predecessor") return { status: "unknown_terminal" };
  if (result.state === "UNKNOWN" || result.state === "ABORTED_PRE_EFFECT") return { status: "unknown_terminal" };
  if (result.kind === "committed" && result.record) {
    const receipt = receiptFrom(result.record);
    if (receipt) return { status: "committed", receipt, finalized: false };
  }
  if (result.kind === "unknown" || result.kind === "legacy_unknown") return { status: "unknown_terminal" };
  if (result.state === "DRIVING" && result.record?.state === "DRIVING"
    && (result.kind === "owned" || result.kind === "busy" || result.kind === "already_started" || result.kind === "driving")) {
    return Date.now() <= result.record.expires_ms
      ? retryableUnknownTerminal()
      : { status: "unknown_terminal" };
  }
  if (result.kind === "busy" || result.kind === "already_started" || result.kind === "driving") return { status: "unknown_terminal" };
  return null;
}

function observationDigest(mirror: SpawnMirrorObservation): string | null {
  return mirror.kind === "exact" && mirror.payload_digest && hex.test(mirror.payload_digest) ? mirror.payload_digest : null;
}

/**
 * Execute one provider effect through the canonical owner ledger. Every
 * pre-DRIVING state is resumable from the durable owner record; once DRIVING
 * is observed the provider is never called again by this route.
 */
export async function runCanonicalEffect<TOpts extends object>(
  deps: CanonicalEffectRouteDeps<TOpts>,
): Promise<CanonicalEffectRouteResult> {
  const tuple = deps.tuple;
  if (!tuple || !text(tuple.repo) || !text(tuple.job_id) || !text(tuple.event_id) || !text(tuple.effect_id)
    || !text(tuple.owner) || !text(tuple.token) || !text(tuple.caller_nonce)) return { status: "unavailable" };
  const req = request(tuple);
  let claimAdmitted = false;
  let effectStarted = false;
  const releaseClaim = async () => {
    if (claimAdmitted && deps.release) { await deps.release().catch(() => undefined); claimAdmitted = false; }
  };

  try {
    const admitted = deps.admit ? await deps.admit() : false;
    if (deps.admit && !admitted) return { status: "busy" };
    // Finalization retries must observe the exact owner tuple before claim
    // admission. A committed pointer is already an idempotency record; asking
    // the provider or competing for the external claim again is forbidden.
    const existing = await deps.ledger.ownerObserve(containmentSpawnActiveKey(tuple), containmentSpawnAttemptKey(tuple));
    const fencedDrainPredecessor = existing.kind === "reaped_predecessor"
      && tuple.path === "drain" && deps.allowFencedDrainPredecessor === true && admitted;
    if (existing.kind === "reaped_predecessor" && !fencedDrainPredecessor) return terminal(existing)!;
    // An empty unknown response cannot identify an owner tuple and is treated
    // as a missing pair; partial or malformed ledger evidence carries both keys.
    const recovered = fencedDrainPredecessor ? null
      : existing.kind === "unknown" && !existing.attempt_key && !existing.active_pointer_key
      ? null : terminal(existing);
    if (recovered) {
      if (recovered.status === "committed" && deps.finalize) recovered.finalized = await deps.finalize(recovered.receipt);
      return recovered;
    }
    const resumable = existing.kind === "owned" && (existing.state === "PREPARED" || existing.state === "CLAIM_ACQUIRED" || existing.state === "PERMIT_ISSUED" || existing.state === "BOUND");
    let state: "PREPARED" | "CLAIM_ACQUIRED" | "PERMIT_ISSUED" | "BOUND" = (resumable ? existing.state : "PREPARED") as "PREPARED" | "CLAIM_ACQUIRED" | "PERMIT_ISSUED" | "BOUND";
    let ownerRecord: any = resumable ? existing.record : undefined;
    const bindingBase = {
      schema_version: 1 as const,
      provider: deps.provider,
      resource_id: deps.resource_id,
      idempotency_key: deps.idempotency_key,
    };
    const binding: ContainmentEffectBinding = {
      ...bindingBase,
      binding_sha256: await sha256(JSON.stringify(bindingBase)),
    };
    // A persisted BOUND owner already has the provider binding. Validate or
    // repair that durable binding before preparation callbacks, which may
    // authorize and mint external credentials. The normal post-claim checks
    // below still revalidate it immediately before BOUND -> DRIVING.
    if (state === "BOUND") {
      const persisted = ownerRecord as { permit?: ContainmentEffectPermit; permit_id?: string | null; effect_start_proof_id?: string | null } | undefined;
      if (!persisted?.permit || persisted.permit.permit_id !== persisted.permit_id
        || !text(persisted.effect_start_proof_id)) {
        return { status: "unknown_terminal", reason: "missing persisted BOUND permit or start proof" };
      }
      const started = await deps.ledger.ownerBegin(req, persisted.permit.permit_id);
      if (!started.proof || started.proof.proof_id !== persisted.effect_start_proof_id
        || (started.kind !== "already_started" && started.kind !== "owned")) {
        return { status: "unknown_terminal", reason: "corrupt persisted BOUND start proof" };
      }
      const validated = await deps.ledger.ownerBind(req, persisted.permit.permit_id, started.proof.proof_id, binding);
      if (validated.kind !== "bound") {
        return terminal(validated) ?? (validated.kind === "unavailable" ? { status: "unavailable" } : { status: "unauthorized" });
      }
      ownerRecord = validated.record ?? ownerRecord;
    }
    if (deps.beforeClaim) await deps.beforeClaim();
    claimAdmitted = await deps.claim();
    // A persisted tuple does not transfer or replace the external claim. Every
    // invocation must win the claim in this invocation; only that claim may be
    // released on an abortable failure.
    if (!claimAdmitted) return { status: "claim_refused" };
    if (deps.beforeDrive && !(await deps.beforeDrive())) { await releaseClaim(); return { status: "before_drive_refused" }; }
    if (!resumable) {
      const prepared = await deps.ledger.ownerPrepare(req);
      const previous = prepared.kind === "committed" || (prepared.kind === "owned" && prepared.state === "COMMITTED") ? terminal(prepared) : null;
      if (previous) {
        if (previous.status === "committed" && deps.finalize) previous.finalized = await deps.finalize(previous.receipt);
        return previous;
      }
      if (prepared.kind !== "prepared") { await releaseClaim(); return prepared.kind === "busy" ? { status: "busy" } : (terminal(prepared) ?? { status: "busy" }); }
      ownerRecord = prepared.record;
    }
    if (state === "PREPARED") {
      const acquired = await deps.ledger.ownerAcquire(req);
      if (acquired.kind !== "acquired" && !(acquired.kind === "owned" && acquired.state === "CLAIM_ACQUIRED")) {
        await releaseClaim(); return acquired.kind === "busy" ? { status: "busy" } : (terminal(acquired) ?? { status: "busy" });
      }
      state = "CLAIM_ACQUIRED"; ownerRecord = acquired.record ?? ownerRecord;
    }
    let permit: ContainmentEffectPermit;
    let proof: NonNullable<OwnerResult["proof"]>;
    if (state === "CLAIM_ACQUIRED") {
      const mirrored = await deps.ledger.ownerMirror(req, "acquired");
      const mirrorDigest = observationDigest(mirrored);
      if (mirrored.kind !== "exact" || !mirrorDigest) {
        await deps.ledger.ownerAbort(req).catch(() => undefined);
        await releaseClaim();
        return { status: mirrored.kind === "mismatch" ? "mirror_tampered" : "unauthorized" };
      }
      let externalPermitId: string | undefined;
      if (deps.beforeConfirm) {
        const candidate = await legacyPermitCandidate(tuple);
        let legacy: LegacyPermit | null | undefined;
        let responseLost = false;
        try {
          legacy = await deps.beforeConfirm(candidate);
        } catch {
          responseLost = true;
          try { legacy = await deps.beforeConfirm(candidate); } catch { legacy = undefined; }
        }
        const validLegacy = !!legacy && text(legacy.permit_id) && legacy.permit_id === candidate
          && legacy.issued_to_owner === tuple.owner && legacy.issued_to_epoch === tuple.lease_epoch;
        if (!validLegacy && responseLost) {
          const frozen = await deps.ledger.ownerFreeze(req).catch(() => null);
          const frozenRecord = frozen?.record;
          const durableFreeze = frozen?.kind === "unknown" && frozen.state === "UNKNOWN" && frozenRecord?.state === "UNKNOWN"
            && frozenRecord.caller_nonce === tuple.caller_nonce && JSON.stringify(frozenRecord.tuple) === JSON.stringify(tuple);
          if (durableFreeze) return { status: "unknown_terminal", reason: "legacy permit response ambiguous" };
          return { status: "unavailable", reason: "legacy permit ambiguity not durably frozen" };
        }
        if (!validLegacy) {
          await deps.ledger.ownerAbort(req).catch(() => undefined);
          await releaseClaim();
          return { status: "unavailable", reason: "legacy permit unavailable or invalid" };
        }
        externalPermitId = legacy!.permit_id;
      }
      if (deps.beforeConfirm && !text(externalPermitId)) {
        await deps.ledger.ownerAbort(req).catch(() => undefined);
        await releaseClaim();
        return { status: "unavailable", reason: "legacy permit unavailable" };
      }
      const confirmed = await deps.ledger.ownerConfirm(
        { ...req, observation_kind: mirrored.kind, observation_digest: mirrorDigest }, mirrorDigest, mirrorDigest, externalPermitId,
      );
      if (confirmed.kind !== "permit_issued" || !confirmed.permit) {
        await releaseClaim(); return terminal(confirmed) ?? { status: "unauthorized" };
      }
      permit = confirmed.permit; ownerRecord = confirmed.record; state = "PERMIT_ISSUED";
    } else {
      const persisted = ownerRecord as { permit?: ContainmentEffectPermit; permit_id?: string | null } | undefined;
      if (!persisted?.permit || persisted.permit.permit_id !== persisted.permit_id) {
        await releaseClaim(); return { status: "unknown_terminal", reason: "missing or corrupt persisted permit" };
      }
      permit = persisted.permit;
    }
    if (state === "PERMIT_ISSUED") {
      if (deps.beforeBegin && !(await deps.beforeBegin(permit))) {
        await releaseClaim(); return { status: "before_drive_refused" };
      }
      const started = await deps.ledger.ownerBegin(req, permit.permit_id);
      if (!started.proof || (started.kind !== "already_started" && started.kind !== "owned")) {
        await releaseClaim(); return terminal(started) ?? { status: "unavailable" };
      }
      proof = started.proof; ownerRecord = started.record ?? ownerRecord; state = "PERMIT_ISSUED";
    } else {
      const persisted = ownerRecord as { effect_start_proof_id?: string | null } | undefined;
      if (!persisted?.effect_start_proof_id) {
        await releaseClaim(); return { status: "unknown_terminal", reason: "missing persisted start proof" };
      }
      // ownerBegin is idempotent for BOUND and returns the validated durable
      // proof; it does not create a second effect start.
      const started = await deps.ledger.ownerBegin(req, permit.permit_id);
      if (!started.proof || (started.kind !== "already_started" && started.kind !== "owned")) {
        await releaseClaim(); return { status: "unknown_terminal", reason: "corrupt persisted start proof" };
      }
      proof = started.proof;
    }
    // Always revalidate the binding before the BOUND -> DRIVING transition.
    // V2 BOUND records can repair an absent KV projection from the canonical
    // DO record; mismatches and KV outages fail before any provider call.
    const bound = await deps.ledger.ownerBind(req, permit.permit_id, proof.proof_id, binding);
    if (bound.kind !== "bound") { await releaseClaim(); return terminal(bound) ?? { status: "unauthorized" }; }
    const boundBinding = (bound.record as any)?.binding ?? binding;
    const driving = await deps.ledger.ownerMarkDriving(req, permit.permit_id, proof.proof_id);
    // A concurrent retry that observes an already-started transition is
    // terminal uncertainty, never permission to invoke the provider again.
    if (driving.kind === "already_started") {
      await releaseClaim(); return terminal(driving) ?? { status: "unknown_terminal" };
    }
    if (driving.kind !== "driving") {
      await releaseClaim();
      return terminal(driving) ?? { status: "unknown_terminal" };
    }
    effectStarted = true;
    let provider: ProviderDriveResult;
    try {
      provider = await deps.drive({
        ...deps.opts,
        containment_event_id: tuple.event_id,
        effect_id: tuple.effect_id,
        effect_permit_id: permit.permit_id,
        effect_proof_id: proof.proof_id,
        effect_binding: boundBinding,
      });
    } catch (error) {
      return retryableUnknownTerminal(error instanceof Error ? error.message : "provider failed");
    }
    if (!provider) {
      return retryableUnknownTerminal("provider returned no trusted receipt");
    }
    if ("status" in provider) return retryableUnknownTerminal(provider.reason ?? "provider refused after DRIVING");
    if (!text(provider.resource_id) || !text(provider.receipt_id) || !text(provider.provider_signature)) return retryableUnknownTerminal("provider returned no trusted receipt");
    if (provider.resource_id !== boundBinding.resource_id) return retryableUnknownTerminal("provider resource mismatch");
    const receiptBase = {
      schema_version: 1 as const,
      trusted: true as const,
      repo: tuple.repo,
      job_id: tuple.job_id,
      path: tuple.path,
      event_id: tuple.event_id,
      reservation_epoch: tuple.reservation_epoch,
      effect_id: tuple.effect_id,
      provider: boundBinding.provider,
      resource_id: boundBinding.resource_id,
      idempotency_key: boundBinding.idempotency_key,
      nonce: tuple.caller_nonce,
      permit_id: permit.permit_id,
      binding_sha256: boundBinding.binding_sha256,
      receipt_id: provider.receipt_id,
      provider_signature: provider.provider_signature,
    };
    const receipt: ContainmentEffectReceipt = {
      ...receiptBase,
      receipt_sha256: await sha256(JSON.stringify(receiptBase)),
    };
    const committed = await deps.ledger.ownerCommit(req, permit.permit_id, proof.proof_id, receipt);
    if (committed.kind !== "committed") return retryableUnknownTerminal();
    const finalized = deps.finalize ? await deps.finalize(receipt) : true;
    return { status: "committed", receipt, finalized };
  } catch (error) {
    if (!effectStarted) await releaseClaim();
    const reason = error instanceof Error ? error.message : "route failure";
    return effectStarted ? retryableUnknownTerminal(reason) : { status: "unavailable", reason };
  } finally {
    // Preparation can mint a credential before this route wins the external
    // spawn claim. If the provider was not started, revoke only that exact
    // invocation's credential. Cleanup failures are recorded by its durable
    // revocation path and must not alter the route's authoritative outcome.
    if (!effectStarted && deps.abandonPreparation) await deps.abandonPreparation().catch(() => undefined);
  }
}

export { containmentSpawnActiveKey, containmentSpawnAttemptKey, containmentSpawnMirrorKey };

export async function callerNonceForEffect(tuple: Omit<OwnerTuple, "caller_nonce">): Promise<string> {
  return (await sha256(JSON.stringify(tuple))).slice(0, 32);
}

export async function intakeOwnerTuple(repo: string, jobId: string, effectId: string, eventId: string): Promise<OwnerTuple> {
  return ownerTuple({ repo, job_id: jobId, path: "intake", event_id: eventId, reservation_epoch: null, effect_id: effectId,
    owner: `intake:${repo}/${jobId}`, token: `intake:${effectId}`, lease_epoch: 1,
    drain_owner: null, drain_lease_epoch: null });
}
export async function drainOwnerTuple(repo: string, jobId: string, effectId: string, eventId: string, owner: string, epoch: number): Promise<OwnerTuple> {
  return ownerTuple({ repo, job_id: jobId, path: "drain", event_id: eventId, reservation_epoch: null, effect_id: effectId,
    owner, token: `drain:${owner}:${epoch}`, lease_epoch: epoch,
    drain_owner: owner, drain_lease_epoch: epoch });
}
export async function redriveOwnerTuple(repo: string, jobId: string, effectId: string, owner: string, token: string, epoch: number): Promise<OwnerTuple> {
  return ownerTuple({ repo, job_id: jobId, path: "redrive", event_id: effectId, reservation_epoch: epoch, effect_id: effectId,
    owner, token, lease_epoch: epoch, drain_owner: `redrive:${owner}`, drain_lease_epoch: epoch,
  });
}

async function ownerTuple(base: Omit<OwnerTuple, "caller_nonce">): Promise<OwnerTuple> {
  return { ...base, caller_nonce: await callerNonceForEffect(base) };
}
