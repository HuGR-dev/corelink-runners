import {
  containmentSpawnActiveKey,
  containmentSpawnAttemptKey,
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

export type ProviderDriveResult = ProviderDriveReceipt | ProviderNoEffectRefusal | void;

export interface CanonicalEffectRouteDeps<TOpts extends object> {
  ledger: Pick<ContainmentEffectLedger,
    "prepare" | "acquire" | "mirror" | "confirm" | "beginEffect" | "bind" |
    "markDriving" | "commitEffect" | "observe" | "abort">;
  tuple: OwnerTuple;
  opts: TOpts;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  claim: () => Promise<boolean>;
  release?: () => Promise<void>;
  beforeClaim?: () => Promise<void>;
  afterClaim?: () => Promise<boolean>;
  beforeDrive?: () => Promise<boolean>;
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
  | { status: "busy" | "unauthorized" | "mirror_tampered" | "claim_refused" | "before_drive_refused" | "provider_refused" | "unknown_terminal" | "unavailable"; reason?: string };

const hex = /^[0-9a-f]{64}$/;
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0;

async function sha256(value: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function request(tuple: OwnerTuple): SpawnOwnerRequest {
  return { schema_version: 1, tuple, caller_nonce: tuple.caller_nonce };
}

function receiptFrom(record: OwnerResult["record"]): ContainmentEffectReceipt | null {
  const value = record && (record as unknown as { effect_observation?: ContainmentEffectReceipt }).effect_observation;
  return value && value.trusted === true ? value : null;
}

function terminal(result: OwnerResult): CanonicalEffectRouteResult | null {
  if (result.kind === "committed" && result.record) {
    const receipt = receiptFrom(result.record);
    if (receipt) return { status: "committed", receipt, finalized: false };
  }
  if (result.kind === "unknown" || result.kind === "legacy_unknown") return { status: "unknown_terminal" };
  if (result.kind === "owned" && result.state === "DRIVING") return { status: "unknown_terminal" };
  if (result.kind === "busy" || result.kind === "already_started" || result.kind === "driving") return { status: "unknown_terminal" };
  return null;
}

function observationDigest(mirror: SpawnMirrorObservation): string | null {
  return mirror.kind === "exact" && mirror.payload_digest && hex.test(mirror.payload_digest) ? mirror.payload_digest : null;
}

/**
 * Execute one provider effect through the canonical owner ledger. Claim
 * admission happens before owner records are created, so a losing caller has
 * no provider, mirror, binding, or permit artifact. Once beginEffect creates
 * a proof, every failure is terminal/unknown and is never retried by time.
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
    // Finalization retries must observe the exact owner tuple before claim
    // admission. A committed pointer is already an idempotency record; asking
    // the provider or competing for the external claim again is forbidden.
    const existing = await deps.ledger.observe(containmentSpawnActiveKey(tuple), containmentSpawnAttemptKey(tuple));
    const recovered = existing.kind === "committed" || (existing.kind === "owned" && existing.state === "DRIVING")
      ? terminal(existing) : null;
    if (recovered) {
      if (recovered.status === "committed" && deps.finalize) recovered.finalized = await deps.finalize(recovered.receipt);
      return recovered;
    }
    if (deps.beforeClaim) await deps.beforeClaim();
    claimAdmitted = await deps.claim();
    if (!claimAdmitted) return { status: "claim_refused" };
    if (deps.afterClaim && !(await deps.afterClaim())) { await releaseClaim(); return { status: "busy" }; }
    if (deps.beforeDrive && !(await deps.beforeDrive())) { await releaseClaim(); return { status: "before_drive_refused" }; }

    const prepared = await deps.ledger.prepare(req);
    const previous = prepared.kind === "committed" || (prepared.kind === "owned" && prepared.state === "COMMITTED")
      ? terminal(prepared) : null;
    if (previous) {
      if (previous.status === "committed" && deps.finalize) previous.finalized = await deps.finalize(previous.receipt);
      return previous;
    }
    if (prepared.kind !== "prepared") { await releaseClaim(); return { status: "busy" }; }

    const acquired = await deps.ledger.acquire(req);
    if (acquired.kind !== "acquired") {
      const prior = terminal(acquired);
      await releaseClaim();
      return prior ?? { status: "busy" };
    }
    const mirrored = await deps.ledger.mirror(req, "acquired");
    const mirrorDigest = observationDigest(mirrored);
    if (mirrored.kind !== "exact" || !mirrorDigest) {
      await deps.ledger.abort(req).catch(() => undefined);
      await releaseClaim();
      return { status: mirrored.kind === "mismatch" ? "mirror_tampered" : "unauthorized" };
    }
    const confirmed = await deps.ledger.confirm(
      { ...req, observation_kind: mirrored.kind, observation_digest: mirrorDigest },
      mirrorDigest,
      mirrorDigest,
    );
    if (confirmed.kind !== "permit_issued" || !confirmed.permit) {
      const prior = terminal(confirmed);
      await releaseClaim();
      return prior ?? { status: "unauthorized" };
    }
    const permit = confirmed.permit;
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
    if (deps.beforeBegin && !(await deps.beforeBegin(permit))) {
      await releaseClaim();
      return { status: "before_drive_refused" };
    }
    const started = await deps.ledger.beginEffect(req, permit.permit_id);
    if (!started.proof || started.kind !== "already_started") {
      const prior = terminal(started);
      await releaseClaim();
      return prior ?? { status: "unauthorized" };
    }
    const proof = started.proof;
    const bound = await deps.ledger.bind(req, permit.permit_id, proof.proof_id, binding);
    if (bound.kind !== "bound") { await releaseClaim(); return terminal(bound) ?? { status: "unauthorized" }; }
    const driving = await deps.ledger.markDriving(req, permit.permit_id, proof.proof_id);
    if (driving.kind !== "driving" && driving.kind !== "already_started") {
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
        effect_binding: binding,
      });
    } catch (error) {
      return { status: "unknown_terminal", reason: error instanceof Error ? error.message : "provider failed" };
    }
    if (!provider) {
      return { status: "unknown_terminal", reason: "provider returned no trusted receipt" };
    }
    if ("status" in provider) return { status: "unknown_terminal", reason: provider.reason ?? "provider refused after DRIVING" };
    if (!text(provider.resource_id) || !text(provider.receipt_id) || !text(provider.provider_signature)) return { status: "unknown_terminal", reason: "provider returned no trusted receipt" };
    if (provider.resource_id !== binding.resource_id) return { status: "unknown_terminal", reason: "provider resource mismatch" };
    const receiptBase = {
      schema_version: 1 as const,
      trusted: true as const,
      repo: tuple.repo,
      job_id: tuple.job_id,
      path: tuple.path,
      event_id: tuple.event_id,
      reservation_epoch: tuple.reservation_epoch,
      effect_id: tuple.effect_id,
      provider: binding.provider,
      resource_id: binding.resource_id,
      idempotency_key: binding.idempotency_key,
      nonce: tuple.caller_nonce,
      permit_id: permit.permit_id,
      binding_sha256: binding.binding_sha256,
      receipt_id: provider.receipt_id,
      provider_signature: provider.provider_signature,
    };
    const receipt: ContainmentEffectReceipt = {
      ...receiptBase,
      receipt_sha256: await sha256(JSON.stringify(receiptBase)),
    };
    const committed = await deps.ledger.commitEffect(req, permit.permit_id, proof.proof_id, receipt);
    if (committed.kind !== "committed") return terminal(committed) ?? { status: "unknown_terminal" };
    const finalized = deps.finalize ? await deps.finalize(receipt) : true;
    return { status: "committed", receipt, finalized };
  } catch (error) {
    if (!effectStarted) await releaseClaim();
    return { status: "unavailable", reason: error instanceof Error ? error.message : "route failure" };
  }
}

export { containmentSpawnActiveKey, containmentSpawnAttemptKey };

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
    owner: `drain:${owner}`, token: `drain:${owner}:${epoch}`, lease_epoch: epoch,
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
