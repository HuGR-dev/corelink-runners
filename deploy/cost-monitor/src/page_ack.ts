import { createHash } from "node:crypto";
import { createPageAckToken, type PageAckToken, type PageAckFields } from "./page_ack_token.js";
import type { MonitorStateStore, Stored } from "./state.js";
import type { DurableAuditLog, AuditReceipt } from "./evidence_log.js";
import type { TrustedClock } from "./trusted_time.js";
import type { SignerRegistryAuthority } from "./signer_registry.js";
import type { AsyncSigner } from "./acks.js";
import type { PendingDelivery } from "./outbox.js";
import type { Incident } from "./incidents.js";

const sha = (v: string) => createHash("sha256").update(v).digest("hex");
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
export interface HumanPrincipal { identity: string; scheduleDigest: string; expiresAt: number }
export interface HumanAuthenticator {
  authenticate(input: { method: "POST"; path: string; apiId: string; stage: string; requestContextIam: unknown; authorization: string; amzDate: string; securityToken?: string; idempotencyKey: string; bodyDigest: string; at: number }): Promise<HumanPrincipal>;
}
export interface OnCallSchedule { authorize(input: { identity: string; destination: string; action: "ACK"; at: number; scheduleDigest: string }): Promise<{ expiresAt: number }> }
export interface PageAckRequest { incident_id: string; page_id: string; delivery_id: string; destination: string; action: "ACK"; payload: string }
export interface PageAckResult { token: PageAckToken; auditReceipt: AuditReceipt }
export interface PageAckOptions {
  store: MonitorStateStore; audit: DurableAuditLog; clock: TrustedClock; registry: SignerRegistryAuthority;
  pageAckSigner: (auth: Awaited<ReturnType<SignerRegistryAuthority["authorize"]>>) => Promise<AsyncSigner>;
  authenticator: HumanAuthenticator; schedule: OnCallSchedule; namespace: string; destination: string; onCallScheduleDigest: string;
}
export class PageAckValidationError extends Error { override readonly name = "PageAckValidationError" }
type PendingPageAck = { status: "pending"; fingerprint: string; fields: PageAckFields; intentReceipt?: AuditReceipt; token?: PageAckToken; resultReceipt?: AuditReceipt };
type CompletedPageAck = Omit<PendingPageAck, "status" | "token" | "resultReceipt"> & { status: "completed"; token: PageAckToken; resultReceipt: AuditReceipt };

export class PageAckService {
  constructor(private readonly o: PageAckOptions) {
    if (!o.store || !o.audit || !o.clock || !o.registry || !o.pageAckSigner || !o.authenticator || !o.schedule || !o.namespace || !o.destination || !/^[0-9a-f]{64}$/.test(o.onCallScheduleDigest)) throw new TypeError("invalid page ACK configuration");
  }
  private async receiptPayload(receipt: AuditReceipt, payload: unknown): Promise<void> {
    await this.o.audit.verify(receipt);
    const record = await this.o.audit.read(receipt.journalReceipt);
    if (!same(record.payload, payload)) throw new PageAckValidationError("durable receipt payload mismatch");
  }
  private load(key: string) { return this.o.store.get<PendingPageAck | CompletedPageAck>(key); }

  async acknowledge(r: PageAckRequest, auth: { authorization: string; method: "POST"; path: string; apiId: string; stage: string; requestContextIam: unknown; amzDate: string; securityToken?: string; idempotencyKey: string }): Promise<PageAckResult> {
    if (!r || r.action !== "ACK" || r.destination !== this.o.destination || typeof r.payload !== "string" || !r.incident_id || !r.delivery_id) throw new PageAckValidationError("invalid page ACK request");
    const now = (await this.o.clock.now()).timeMs;
    if (!Number.isSafeInteger(now) || now <= 0) throw new PageAckValidationError("trusted time unavailable");
    const bodyDigest = sha(r.payload);
    if (sha(JSON.stringify(["page", r.incident_id, r.delivery_id, r.destination])) !== r.page_id) throw new PageAckValidationError("invalid page id");
    const expectedIdempotency = sha(JSON.stringify(["page-ack", r.incident_id, r.page_id, r.delivery_id, r.destination, bodyDigest]));
    if (auth.idempotencyKey !== expectedIdempotency) throw new PageAckValidationError("idempotency mismatch");
    const principal = await this.o.authenticator.authenticate({ ...auth, bodyDigest, at: now });
    if (!principal || !principal.identity || principal.scheduleDigest !== this.o.onCallScheduleDigest || principal.expiresAt < now) throw new PageAckValidationError("principal refused");
    const schedule = await this.o.schedule.authorize({ identity: principal.identity, destination: r.destination, action: "ACK", at: now, scheduleDigest: principal.scheduleDigest });
    if (!schedule || !Number.isSafeInteger(schedule.expiresAt) || schedule.expiresAt < now) throw new PageAckValidationError("schedule refused");
    const delivery = await this.o.store.get<PendingDelivery>(`${this.o.namespace}:delivery:${r.delivery_id}`);
    if (!delivery || delivery.value.operation.incidentId !== r.incident_id || delivery.value.operation.destination !== r.destination || delivery.value.operation.payload !== r.payload || delivery.value.operation.payloadDigest !== bodyDigest) throw new PageAckValidationError("delivery mismatch");

    const id = sha(JSON.stringify([r.incident_id, r.page_id, r.delivery_id, r.destination]));
    const key = `${this.o.namespace}:page-ack:${id}`;
    const fingerprint = JSON.stringify(r);
    let stored = await this.load(key);
    if (stored?.value.fingerprint !== fingerprint) throw new PageAckValidationError("page ACK request fork");
    if (stored?.value.status === "completed") {
      await this.receiptPayload(stored.value.resultReceipt, { type: "PAGE_ACK_RESULT", token: stored.value.token });
      return { token: stored.value.token, auditReceipt: stored.value.resultReceipt };
    }
    if (!stored) {
      const registry = await this.o.registry.current();
      let signerAuth;
      try {
        signerAuth = await this.o.registry.authorize({ role: "page-ack", keyId: registry.manifest.active_signer_key_id, epoch: registry.manifest.active_signer_epoch, tupleDigest: registry.manifest.monitor_rearm_tuple_digest, at: now });
      } catch {
        signerAuth = await this.o.registry.authorize({ role: "page-ack", keyId: registry.manifest.next_signer_key_id, epoch: registry.manifest.next_signer_epoch, tupleDigest: registry.manifest.monitor_rearm_tuple_digest, at: now });
      }
      const signer = await this.o.pageAckSigner(signerAuth);
      const fields: PageAckFields = {
        page_ack_version: "1", incident_id: r.incident_id, page_id: r.page_id, delivery_id: r.delivery_id, destination: r.destination,
        on_call_identity: principal.identity, on_call_schedule_digest: principal.scheduleDigest, action: "ACK", payload_digest: bodyDigest,
        monitor_rearm_tuple_digest: signerAuth.manifest.monitor_rearm_tuple_digest, signer_rotation_manifest_digest: signerAuth.manifestDigest,
        acknowledged_at: now, expires_at: schedule.expiresAt, signer_key_id: signer.identity.keyId, signer_epoch: signer.identity.epoch,
      };
      if ((await this.o.store.transact([{ key, expectedVersion: null, value: { status: "pending", fingerprint, fields } satisfies PendingPageAck }])) !== "committed") throw new PageAckValidationError("page ACK reservation conflict");
      stored = await this.load(key);
    }
    if (!stored || stored.value.status !== "pending") throw new PageAckValidationError("page ACK reservation unavailable");
    let state = stored as Stored<PendingPageAck>;
    const intentPayload = { type: "PAGE_ACK_INTENT", ...state.value.fields };
    let intentReceipt = state.value.intentReceipt;
    if (!intentReceipt) {
      intentReceipt = await this.o.audit.append(`${key}:intent`, intentPayload);
      await this.receiptPayload(intentReceipt, intentPayload);
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, intentReceipt } }])) !== "committed") throw new PageAckValidationError("page ACK intent attach conflict");
      // A new Stored value carries the post-intent CAS version.  Using the old
      // reservation version would make every normal ACK fail its final CAS.
      state = (await this.load(key))! as Stored<PendingPageAck>;
    } else {
      await this.receiptPayload(intentReceipt, intentPayload);
    }
    let token = state.value.token;
    if (!token) {
      const authorization = await this.o.registry.authorize({ role: "page-ack", keyId: state.value.fields.signer_key_id, epoch: state.value.fields.signer_epoch, tupleDigest: state.value.fields.monitor_rearm_tuple_digest, at: state.value.fields.acknowledged_at });
      token = await createPageAckToken(state.value.fields, await this.o.pageAckSigner(authorization));
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, token } }])) !== "committed") throw new PageAckValidationError("page ACK token attach conflict");
      state = (await this.load(key))! as Stored<PendingPageAck>;
      token = state.value.token!;
    }
    const resultPayload = { type: "PAGE_ACK_RESULT", token };
    let resultReceipt = state.value.resultReceipt;
    if (!resultReceipt) {
      resultReceipt = await this.o.audit.append(`${key}:result`, resultPayload);
      await this.receiptPayload(resultReceipt, resultPayload);
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, resultReceipt } }])) !== "committed") throw new PageAckValidationError("page ACK result attach conflict");
      state = (await this.load(key))! as Stored<PendingPageAck>;
    } else {
      await this.receiptPayload(resultReceipt, resultPayload);
    }
    // Refresh the incident after all durable receipts.  The value read before
    // the intent may have changed while the signer and journal operations ran.
    const incident = await this.o.store.get<Incident>(`${this.o.namespace}:incident:${r.incident_id}`);
    if (!incident) throw new PageAckValidationError("incident unavailable");
    const completed: CompletedPageAck = { ...state.value, status: "completed", token, resultReceipt };
    const nextIncident = { ...incident.value, humanAcknowledgedAt: incident.value.humanAcknowledgedAt ?? now };
    if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: completed }, { key: incident.key, expectedVersion: incident.version, value: nextIncident }])) !== "committed") throw new PageAckValidationError("page ACK commit conflict");
    return { token, auditReceipt: resultReceipt };
  }
}
