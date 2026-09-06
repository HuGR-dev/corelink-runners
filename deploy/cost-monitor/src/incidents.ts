import { createHash } from "node:crypto";

export type Signal = {
  sourceKey: string;
  failing: boolean;
  reason: string;
  highWaters: Record<string, number>;
  monitorTupleDigest: string;
};

export type Incident = {
  incidentId: string;
  sourceKey: string;
  monitorTupleDigest: string;
  status: "open" | "recovering" | "recovered";
  openedAt: number;
  lastFailureAt: number;
  allClearSince: number | null;
  initialHighWaters: Record<string, number>;
  latestHighWaters: Record<string, number>;
  humanAcknowledgedAt: number | null;
  escalationCreated: boolean;
  revision: number;
  lastReason: string;
};

export type IncidentDecision = {
  incident: Incident | null;
  alerts: Array<{ operationId: string; incidentId: string; kind: "initial" | "escalation" | "recovery" | "update"; reason: string }>;
};

const QUIET_MS = 330_000;
const ESCALATION_MS = 300_000;

function digest(value: unknown): string { return createHash("sha256").update(JSON.stringify(value)).digest("hex"); }
function copyWaters(waters: Record<string, number>): Record<string, number> { return Object.fromEntries(Object.entries(waters)); }
function validTime(value: number, field: string): void {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${field} is invalid`);
}
function validateSignal(signal: Signal, nowMs: number): void {
  if (!signal || typeof signal.sourceKey !== "string" || !signal.sourceKey || signal.sourceKey.trim() !== signal.sourceKey) throw new Error("sourceKey is invalid");
  if (typeof signal.reason !== "string" || signal.reason.trim() !== signal.reason) throw new Error("reason is invalid");
  if (typeof signal.monitorTupleDigest !== "string" || signal.monitorTupleDigest.trim() !== signal.monitorTupleDigest) throw new Error("monitorTupleDigest is invalid");
  validTime(nowMs, "nowMs");
  if (!signal.highWaters || Array.isArray(signal.highWaters)) throw new Error("highWaters is invalid");
  for (const [key, value] of Object.entries(signal.highWaters)) {
    if (!key || !Number.isSafeInteger(value) || value < 0) throw new Error("highWater is invalid");
  }
}
function validatePrevious(previous: Incident | null, nowMs: number): void {
  if (!previous) return;
  if (typeof previous !== "object" || Array.isArray(previous) || Object.getPrototypeOf(previous) !== Object.prototype) throw new Error("incident is invalid");
  const fields = ["incidentId", "sourceKey", "monitorTupleDigest", "status", "openedAt", "lastFailureAt", "allClearSince", "initialHighWaters", "latestHighWaters", "humanAcknowledgedAt", "escalationCreated", "revision", "lastReason"];
  const own = Object.keys(previous);
  if (own.length !== fields.length || fields.some((field) => !Object.prototype.hasOwnProperty.call(previous, field))) throw new Error("incident fields are invalid");
  for (const [field, value] of [["incidentId", previous.incidentId], ["sourceKey", previous.sourceKey], ["monitorTupleDigest", previous.monitorTupleDigest], ["lastReason", previous.lastReason]] as const) {
    if (typeof value !== "string" || value.length === 0 || value.length > 256 || value.trim() !== value) throw new Error(`${field} is invalid`);
  }
  if (!["open", "recovering", "recovered"].includes(previous.status)) throw new Error("status is invalid");
  if (typeof previous.escalationCreated !== "boolean") throw new Error("escalationCreated is invalid");
  if (!Number.isSafeInteger(previous.revision) || previous.revision <= 0) throw new Error("revision is invalid");
  const validateWaters = (waters: Record<string, number>, field: string): void => {
    if (!waters || typeof waters !== "object" || Array.isArray(waters) || Object.getPrototypeOf(waters) !== Object.prototype) throw new Error(`${field} is invalid`);
    for (const [key, value] of Object.entries(waters)) {
      if (key.length === 0 || key.length > 256 || key.trim() !== key || !Number.isSafeInteger(value) || value < 0) throw new Error(`${field} is invalid`);
    }
  };
  validateWaters(previous.initialHighWaters, "initialHighWaters");
  validateWaters(previous.latestHighWaters, "latestHighWaters");
  if (Object.keys(previous.initialHighWaters).some((key) => !(key in previous.latestHighWaters))) throw new Error("latestHighWaters is incomplete");
  validTime(previous.openedAt, "openedAt");
  validTime(previous.lastFailureAt, "lastFailureAt");
  if (previous.allClearSince !== null) validTime(previous.allClearSince, "allClearSince");
  if (previous.lastFailureAt < previous.openedAt || (previous.allClearSince !== null && previous.allClearSince < previous.lastFailureAt)) throw new Error("incident times are incoherent");
  if (previous.humanAcknowledgedAt !== null) validTime(previous.humanAcknowledgedAt, "humanAcknowledgedAt");
  if (previous.humanAcknowledgedAt !== null && (previous.humanAcknowledgedAt < previous.openedAt || previous.humanAcknowledgedAt > nowMs)) throw new Error("humanAcknowledgedAt is incoherent");
  if (previous.status === "open" && previous.allClearSince !== null) throw new Error("open incident has clear time");
  if ((previous.status === "recovering" || previous.status === "recovered") && previous.allClearSince === null) throw new Error("cleared incident is missing clear time");
  if (nowMs < previous.openedAt || nowMs < previous.lastFailureAt || (previous.allClearSince !== null && nowMs < previous.allClearSince)) throw new Error("nowMs regresses incident time");
}
function alert(incident: Incident, kind: "initial" | "escalation" | "recovery" | "update", reason: string) {
  return { operationId: digest([incident.incidentId, incident.revision, kind]), incidentId: incident.incidentId, kind, reason };
}
function newIncident(signal: Signal, nowMs: number): Incident {
  const initial = copyWaters(signal.highWaters);
  const revision = 1;
  return {
    incidentId: digest([signal.sourceKey, signal.monitorTupleDigest, nowMs, revision]),
    sourceKey: signal.sourceKey, monitorTupleDigest: signal.monitorTupleDigest, status: "open",
    openedAt: nowMs, lastFailureAt: nowMs, allClearSince: null,
    initialHighWaters: initial, latestHighWaters: copyWaters(initial), humanAcknowledgedAt: null,
    escalationCreated: false, revision, lastReason: signal.reason,
  };
}
function changedFailure(previous: Incident, signal: Signal): boolean {
  if (previous.sourceKey !== signal.sourceKey || previous.monitorTupleDigest !== signal.monitorTupleDigest || previous.lastReason !== signal.reason) return true;
  const keys = new Set([...Object.keys(previous.latestHighWaters), ...Object.keys(signal.highWaters)]);
  return [...keys].some((key) => previous.latestHighWaters[key] !== signal.highWaters[key]);
}
function allAdvanced(previous: Incident, signal: Signal): boolean {
  return Object.entries(previous.initialHighWaters).every(([key, value]) => Number.isSafeInteger(signal.highWaters[key]) && signal.highWaters[key] > value);
}

export function evaluateIncident(previous: Incident | null, signal: Signal, nowMs: number): IncidentDecision {
  validateSignal(signal, nowMs); validatePrevious(previous, nowMs);
  if (!signal.failing && !previous) return { incident: null, alerts: [] };
  if (signal.failing && (!previous || previous.status === "recovered" || previous.sourceKey !== signal.sourceKey || previous.monitorTupleDigest !== signal.monitorTupleDigest)) {
    const incident = newIncident(signal, nowMs);
    return { incident, alerts: [alert(incident, "initial", signal.reason)] };
  }
  if (!previous) return { incident: null, alerts: [] };
  const incident: Incident = { ...previous, initialHighWaters: copyWaters(previous.initialHighWaters), latestHighWaters: copyWaters(previous.latestHighWaters) };
  if (signal.failing) {
    const changed = changedFailure(previous, signal);
    incident.latestHighWaters = copyWaters(signal.highWaters);
    incident.lastFailureAt = nowMs;
    incident.lastReason = signal.reason;
    incident.status = "open";
    incident.allClearSince = null;
    if (!changed) return { incident, alerts: [] };
    incident.revision += 1;
    return { incident, alerts: [alert(incident, "update", signal.reason)] };
  }
  incident.latestHighWaters = copyWaters(signal.highWaters);
  if (previous.status === "recovered") return { incident, alerts: [] };
  if (!allAdvanced(previous, signal)) {
    incident.status = "open";
    incident.allClearSince = null;
    incident.lastReason = signal.reason;
    if (previous.allClearSince === null && !changedFailure(previous, signal)) return { incident, alerts: [] };
    incident.revision += 1;
    return { incident, alerts: [alert(incident, "update", signal.reason)] };
  }
  if (incident.allClearSince === null) {
    incident.allClearSince = nowMs;
    incident.status = "recovering";
    incident.revision += 1;
    return { incident, alerts: [alert(incident, "update", signal.reason)] };
  }
  if (nowMs - incident.allClearSince < QUIET_MS) {
    incident.status = "recovering";
    return { incident, alerts: [] };
  }
  incident.status = "recovered";
  incident.revision += 1;
  return { incident, alerts: [alert(incident, "recovery", signal.reason)] };
}

export function evaluateEscalation(previous: Incident | null, nowMs: number): IncidentDecision {
  validTime(nowMs, "nowMs");
  if (!previous) return { incident: null, alerts: [] };
  validatePrevious(previous, nowMs);
  if (previous.status === "recovered" || previous.humanAcknowledgedAt !== null || previous.escalationCreated || nowMs < previous.openedAt + ESCALATION_MS) return { incident: previous, alerts: [] };
  const incident: Incident = { ...previous, initialHighWaters: copyWaters(previous.initialHighWaters), latestHighWaters: copyWaters(previous.latestHighWaters), escalationCreated: true, revision: previous.revision + 1 };
  return { incident, alerts: [alert(incident, "escalation", incident.lastReason)] };
}
