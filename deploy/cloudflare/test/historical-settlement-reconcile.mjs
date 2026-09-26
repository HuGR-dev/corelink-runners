#!/usr/bin/env node
// Offline classifier for legacy usage:settled:* markers. This program never
// connects to KV, billing, or a provider and never emits record identities.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import assert from "node:assert/strict";
import { test } from "node:test";

export const MAX_INPUT_BYTES = 16 * 1024 * 1024;
export const MAX_CANDIDATES = 10_000;
const TENANT_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SHA256 = /^[0-9a-f]{64}$/;

const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const exactKeys = (value, required, optional = []) => value && typeof value === "object" && !Array.isArray(value)
  && required.every((key) => Object.hasOwn(value, key))
  && Object.keys(value).every((key) => required.includes(key) || optional.includes(key));
const isText = (value) => typeof value === "string" && value.length > 0;

function failClosed() {
  // Keep exception text generic: malformed snapshots may contain customer data.
  throw new Error("snapshot is ambiguous or invalid; no recovery action was produced");
}

function expectedIdemKey(source) {
  if (!exactKeys(source, ["jobId", "tenant", "startedMs", "completedMs", "region"])) failClosed();
  if (!isText(source.jobId) || source.jobId.length > 256 || !TENANT_UUID.test(source.tenant)
    || !Number.isSafeInteger(source.startedMs) || source.startedMs < 0
    || !Number.isSafeInteger(source.completedMs) || source.completedMs < source.startedMs
    || !/^[a-z]{3}$/.test(source.region)) failClosed();
  const date = new Date(source.completedMs);
  const period = `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, "0")}`;
  return sha256(`${source.jobId}|${period}`);
}

function validAcknowledgement(ack, expectedKey) {
  if (ack === null) return "unresolved";
  if (!exactKeys(ack, ["http_status", "body", "outcome_index"]) || !Number.isSafeInteger(ack.http_status)
    || !Number.isSafeInteger(ack.outcome_index) || ack.outcome_index < 0) return "unresolved";
  const body = ack.body;
  if (!exactKeys(body, ["outcomes", "accepted", "deduped", "rejected", "total"]) || !Array.isArray(body.outcomes)
    || body.outcomes.length === 0 || body.outcomes.length > 1024 || ack.outcome_index >= body.outcomes.length) return "unresolved";
  const max = body.outcomes.length;
  const count = (n) => Number.isSafeInteger(n) && n >= 0 && n <= max;
  if (!count(body.accepted) || !count(body.deduped) || !count(body.rejected) || !count(body.total)) return "unresolved";
  let accepted = 0; let deduped = 0; let rejected = 0; let conflicts = 0;
  for (let i = 0; i < max; i += 1) {
    const row = body.outcomes[i];
    if (!exactKeys(row, ["index", "idem_key", "outcome"], ["reason"]) || row.index !== i
      || !(row.idem_key === null || (typeof row.idem_key === "string" && SHA256.test(row.idem_key)))
      || !["accepted", "deduped", "rejected", "conflict"].includes(row.outcome)) return "unresolved";
    const needsReason = row.outcome === "rejected" || row.outcome === "conflict";
    if ((needsReason && !isText(row.reason)) || (!needsReason && Object.hasOwn(row, "reason"))) return "unresolved";
    if (row.outcome === "accepted") accepted += 1;
    else if (row.outcome === "deduped") deduped += 1;
    else if (row.outcome === "rejected") rejected += 1;
    else conflicts += 1;
  }
  if (accepted !== body.accepted || deduped !== body.deduped || rejected !== body.rejected
    || body.total !== accepted + deduped) return "unresolved";
  if ((conflicts > 0 && ack.http_status !== 409)
    || (conflicts === 0 && rejected === max && ack.http_status !== 422)
    || (conflicts === 0 && rejected !== max && ack.http_status !== 202)) return "unresolved";
  const matched = body.outcomes[ack.outcome_index];
  if (matched.idem_key !== expectedKey) return "unresolved";
  return matched.outcome;
}

function buildPlan(bytes, mode = "dry-run") {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_INPUT_BYTES
    || !["dry-run", "rollback"].includes(mode)) failClosed();
  let input;
  try { input = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)); } catch { failClosed(); }
  if (!exactKeys(input, ["schema_version", "scan_complete", "candidates"]) || input.schema_version !== 1
    || input.scan_complete !== true || !Array.isArray(input.candidates) || input.candidates.length > MAX_CANDIDATES) failClosed();

  const unique = new Map();
  let duplicateRows = 0;
  for (const candidate of input.candidates) {
    if (!exactKeys(candidate, ["marker_key", "marker_value", "source_key", "source_value", "acknowledgement"])) failClosed();
    if (!isText(candidate.marker_key) || !candidate.marker_key.startsWith("usage:settled:")
      || !isText(candidate.marker_value) || candidate.marker_value.length > 256
      || !(candidate.source_key === null || isText(candidate.source_key))
      || !(candidate.source_value === null || isText(candidate.source_value))
      || !(candidate.acknowledgement === null || (candidate.acknowledgement && typeof candidate.acknowledgement === "object"))) failClosed();
    const canonical = JSON.stringify(candidate);
    const prev = unique.get(candidate.marker_key);
    if (prev !== undefined) {
      if (prev !== canonical) failClosed();
      duplicateRows += 1;
      continue;
    }
    unique.set(candidate.marker_key, canonical);
  }

  const counts = { accepted: 0, deduped: 0, rejected: 0, conflict: 0, unresolved: 0 };
  const entries = [];
  for (const [markerKey, canonical] of unique) {
    const candidate = JSON.parse(canonical);
    let disposition = "unresolved";
    if (candidate.source_key !== null && candidate.source_value !== null) {
      if (!candidate.source_key.startsWith("usage:")) failClosed();
      let source;
      try { source = JSON.parse(candidate.source_value); } catch { failClosed(); }
      const idem = expectedIdemKey(source);
      if (candidate.source_key !== `usage:${source.jobId}` || markerKey !== `usage:settled:${encodeURIComponent(idem)}`) failClosed();
      disposition = validAcknowledgement(candidate.acknowledgement, idem);
    }
    counts[disposition] += 1;
    entries.push({
      marker_key: markerKey,
      source_key: candidate.source_key,
      source_present: candidate.source_value !== null,
      idem_key: markerKey.slice("usage:settled:".length),
      disposition,
      // Rejected is the only explicit non-acceptance outcome eligible for a
      // human-reviewed, once-only correction intent. Conflict is ambiguous.
      eligible_for_single_correction: disposition === "rejected" && candidate.source_key !== null && candidate.source_value !== null,
    });
  }
  const candidateSha = sha256(bytes);
  const classified = Object.values(counts).reduce((sum, count) => sum + count, 0);
  const receipt = {
    schema_version: 1,
    mode,
    candidate_sha256: candidateSha,
    counts: {
      scanned_rows: input.candidates.length,
      unique_candidates: unique.size,
      duplicate_rows: duplicateRows,
      classified_candidates: classified,
      ...counts,
    },
    planned_actions: {
      eligible_for_single_correction: entries.filter((entry) => entry.eligible_for_single_correction).length,
      marker_removal_after_durable_classification: entries.filter((entry) => entry.eligible_for_single_correction).length,
      source_deletion: 0,
      automatic_resend: 0,
    },
    rollback: mode === "rollback" ? "no-op; offline classification made no source or marker changes" : null,
  };
  return { receipt, entries };
}

/** Classify bounded offline export bytes and return only the redacted receipt. */
export function classifySnapshot(bytes, mode = "dry-run") {
  return buildPlan(bytes, mode).receipt;
}

/** Create a JSON-safe, local-only fixture journal; no external store is used. */
export function createLocalJournal(bytes) {
  const { receipt, entries } = buildPlan(bytes);
  const markers = Object.fromEntries(entries.map((entry) => [entry.marker_key, true]));
  const sources = Object.fromEntries(entries.filter((entry) => entry.source_key !== null).map((entry) => [entry.source_key, entry.source_present]));
  return { candidate_sha256: receipt.candidate_sha256, markers, sources, classifications: {}, correction_intents: [], complete: false };
}

/** Apply classification then eligible marker staging to an in-memory fixture journal. */
export function advanceLocalJournal(bytes, prior, { interruptAfter = Infinity } = {}) {
  const { receipt, entries } = buildPlan(bytes);
  if (!prior || prior.candidate_sha256 !== receipt.candidate_sha256 || !Number.isSafeInteger(interruptAfter) && interruptAfter !== Infinity) failClosed();
  const state = structuredClone(prior);
  let steps = 0;
  for (const entry of entries) {
    let durable = state.classifications[entry.marker_key];
    if (!durable) {
      durable = { outcome: entry.disposition, idem_key: entry.idem_key, eligible_for_single_correction: entry.eligible_for_single_correction };
      state.classifications[entry.marker_key] = durable; // durable classification precedes any marker change
      steps += 1;
      if (steps >= interruptAfter) return state;
    } else if (durable.outcome !== entry.disposition || durable.idem_key !== entry.idem_key
      || durable.eligible_for_single_correction !== entry.eligible_for_single_correction) failClosed();

    if (!entry.eligible_for_single_correction) continue;
    let intent = state.correction_intents.find((item) => item.marker_key === entry.marker_key);
    if (!intent) {
      // Persist operator-reviewed intent before staging marker removal. It never sends an event.
      intent = { marker_key: entry.marker_key, idem_key: entry.idem_key, attempt: 1 };
      state.correction_intents.push(intent);
      steps += 1;
      if (steps >= interruptAfter) return state;
    }
    if (intent.idem_key !== entry.idem_key || intent.attempt !== 1) failClosed();
    if (state.markers[entry.marker_key] !== false) {
      state.markers[entry.marker_key] = false;
      steps += 1;
      if (steps >= interruptAfter) return state;
    }
  }
  state.complete = true;
  return state;
}

/** Restore the original staged fixture state; production state is never touched. */
export function rollbackLocalJournal(bytes, prior) {
  const original = createLocalJournal(bytes);
  if (!prior || prior.candidate_sha256 !== original.candidate_sha256) failClosed();
  return original;
}

async function main(args) {
  const mode = args.includes("--rollback") ? "rollback" : "dry-run";
  const positional = args.filter((arg) => arg !== "--rollback");
  if (positional.length !== 1) throw new Error("usage: reconcile-historical-settlements.mjs [--rollback] SNAPSHOT.json");
  const bytes = await readFile(positional[0]);
  if (bytes.byteLength > MAX_INPUT_BYTES) failClosed();
  process.stdout.write(`${JSON.stringify(classifySnapshot(bytes, mode))}\n`);
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
if (!process.env.NODE_TEST_CONTEXT && invokedPath === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 2;
  });
}

// Keep the focused fixture matrix runnable on a hosted runner with the single
// command `node --test deploy/cloudflare/test/historical-settlement-reconcile.mjs`.
if (process.env.NODE_TEST_CONTEXT) {
  const fixturePath = new URL("./fixtures/issue-603/recovery-matrix.json", import.meta.url);
  const tenant = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
  const completedMs = Date.parse("2026-09-01T00:00:00Z");
  const fixtureTemplate = JSON.parse(await (await import("node:fs/promises")).readFile(fixturePath, "utf8"));
  const ids = Object.fromEntries(fixtureTemplate.cases.filter((item) => item.job_id).map((item) => [
    item.name === "interrupted_missing_source" ? "interrupted" : item.name,
    sha256(`${item.job_id}|2026-09`),
  ]));
  const source = (jobId) => JSON.stringify({ jobId, tenant, startedMs: completedMs - 5_000, completedMs, region: "iad" });
  const ack = (idem_key, outcome, http_status) => ({
    http_status,
    outcome_index: 0,
    body: {
      outcomes: [{ index: 0, idem_key, outcome, ...(["rejected", "conflict"].includes(outcome) ? { reason: outcome === "rejected" ? "invalid_record" : "payload_mismatch" } : {}) }],
      accepted: outcome === "accepted" ? 1 : 0,
      deduped: outcome === "deduped" ? 1 : 0,
      rejected: outcome === "rejected" ? 1 : 0,
      total: outcome === "accepted" || outcome === "deduped" ? 1 : 0,
    },
  });
  const candidate = (jobId, idem, proof) => ({
    marker_key: `usage:settled:${encodeURIComponent(idem)}`,
    marker_value: "1798761600000",
    source_key: `usage:${jobId}`,
    source_value: source(jobId),
    acknowledgement: proof,
  });
  const byName = new Map();
  const matrix = { schema_version: 1, scan_complete: true, candidates: [] };
  for (const item of fixtureTemplate.cases) {
    if (item.duplicate_of) {
      matrix.candidates.push(structuredClone(byName.get(item.duplicate_of)));
      continue;
    }
    const idem = ids[item.name === "interrupted_missing_source" ? "interrupted" : item.name];
    const value = item.source_present === false
      ? { marker_key: `usage:settled:${encodeURIComponent(idem)}`, marker_value: "1798761600000", source_key: null, source_value: null, acknowledgement: null }
      : candidate(item.job_id, idem, item.outcome === null ? null : ack(idem, item.outcome, item.http_status));
    byName.set(item.name, value);
    matrix.candidates.push(value);
  }

  test("classifies only exact accepted/deduped acknowledgements; rejected and missing evidence stay unresolved for action", async () => {
    const bytes = Buffer.from(JSON.stringify(matrix));
    const result = classifySnapshot(bytes);
    assert.deepEqual(result.counts, {
      scanned_rows: 7, unique_candidates: 6, duplicate_rows: 1, classified_candidates: 6,
      accepted: 1, deduped: 1, rejected: 1, conflict: 1, unresolved: 2,
    });
    assert.deepEqual(result.planned_actions, {
      eligible_for_single_correction: 1, marker_removal_after_durable_classification: 1,
      source_deletion: 0, automatic_resend: 0,
    });
    assert.match(result.candidate_sha256, SHA256);
    assert.equal(JSON.stringify(result).includes(tenant), false);
    assert.equal(JSON.stringify(result).includes("job-accepted"), false);
  });

  test("restart after durable classification, partial staging, replay, and rollback converge", async () => {
    const bytes = Buffer.from(JSON.stringify(matrix));
    const plan = classifySnapshot(bytes);
    const repeatedPlan = classifySnapshot(bytes);
    assert.deepEqual(plan.counts, repeatedPlan.counts);
    assert.equal(plan.candidate_sha256, repeatedPlan.candidate_sha256);
    const acceptedKey = `usage:settled:${ids.accepted}`;
    const dedupedKey = `usage:settled:${ids.deduped}`;
    const rejectedKey = `usage:settled:${ids.rejected}`;
    const conflictKey = `usage:settled:${ids.conflict}`;
    const missingKey = `usage:settled:${ids.missing_ack}`;
    const interruptedKey = `usage:settled:${ids.interrupted}`;
    const initial = createLocalJournal(bytes);
    const interrupted = advanceLocalJournal(bytes, initial, { interruptAfter: 1 });
    assert.equal(interrupted.classifications[acceptedKey].outcome, "accepted");
    assert.equal(interrupted.markers[rejectedKey], true);
    assert.equal(interrupted.correction_intents.length, 0);
    const resumedFromClassification = advanceLocalJournal(bytes, interrupted);
    assert.equal(resumedFromClassification.correction_intents.length, 1);
    assert.equal(resumedFromClassification.markers[rejectedKey], false);

    const stagedIntent = advanceLocalJournal(bytes, createLocalJournal(bytes), { interruptAfter: 4 });
    assert.equal(stagedIntent.correction_intents.length, 1);
    assert.equal(stagedIntent.markers[rejectedKey], true);
    const resumed = advanceLocalJournal(bytes, stagedIntent);
    const replayed = advanceLocalJournal(bytes, resumed);
    assert.equal(resumed.markers[acceptedKey], true);
    assert.equal(resumed.markers[dedupedKey], true);
    assert.equal(resumed.markers[rejectedKey], false);
    assert.equal(resumed.markers[conflictKey], true);
    assert.equal(resumed.markers[missingKey], true);
    assert.equal(resumed.markers[interruptedKey], true);
    assert.equal(resumed.sources["usage:job-rejected"], true);
    assert.equal(resumed.sources["usage:job-conflict"], true);
    assert.equal(resumed.sources["usage:job-missing"], true);
    assert.equal(resumed.correction_intents.length, 1);
    assert.equal(resumed.correction_intents[0].idem_key, ids.rejected);
    assert.equal(resumed.correction_intents[0].attempt, 1);
    assert.deepEqual(resumed, replayed);
    assert.equal(resumed.candidate_sha256, plan.candidate_sha256);

    const rolledBack = rollbackLocalJournal(bytes, resumed);
    assert.equal(rolledBack.markers[rejectedKey], true);
    assert.equal(rolledBack.sources["usage:job-rejected"], true);
    assert.deepEqual(rolledBack.correction_intents, []);
    assert.equal(advanceLocalJournal(bytes, rolledBack).correction_intents.length, 1);
  });

  test("a recovered interrupted snapshot can classify only after source and matching durable ACK arrive", () => {
    const initial = {
      schema_version: 1, scan_complete: true,
      candidates: [{
        marker_key: `usage:settled:${ids.interrupted}`,
        marker_value: "1798761600000", source_key: null, source_value: null, acknowledgement: null,
      }],
    };
    const before = classifySnapshot(Buffer.from(JSON.stringify(initial)));
    assert.equal(before.counts.unresolved, 1);
    initial.candidates[0].source_key = "usage:job-interrupted";
    initial.candidates[0].source_value = source("job-interrupted");
    initial.candidates[0].acknowledgement = ack(ids.interrupted, "deduped", 202);
    const after = classifySnapshot(Buffer.from(JSON.stringify(initial)));
    assert.equal(after.counts.deduped, 1);
    assert.equal(after.planned_actions.automatic_resend, 0);
  });

  test("fails closed for incomplete scans, malformed ACKs, and conflicting duplicates", () => {
    assert.throws(() => classifySnapshot(Buffer.from(JSON.stringify({ ...matrix, scan_complete: false }))));
    const malformed = structuredClone(matrix);
    for (const index of [0, 5]) malformed.candidates[index].acknowledgement.body.outcomes[0].index = 1;
    assert.equal(classifySnapshot(Buffer.from(JSON.stringify(malformed))).counts.unresolved, 3);
    const conflicting = structuredClone(matrix);
    conflicting.candidates[5].marker_value = "different";
    assert.throws(() => classifySnapshot(Buffer.from(JSON.stringify(conflicting))));
  });
}
