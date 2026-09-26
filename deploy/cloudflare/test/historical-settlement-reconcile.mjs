#!/usr/bin/env node
// Offline classification and restricted local review journal. No provider/KV access.
import { createHash, randomBytes } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import { link, lstat, open, readFile, readdir, rename, unlink } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";
import assert from "node:assert/strict";
import { test } from "node:test";

export const MAX_INPUT_BYTES = 16 * 1024 * 1024;
export const MAX_CANDIDATES = 10_000;
export const MAX_JOURNAL_BYTES = 8 * 1024 * 1024;
const TENANT_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SHA256 = /^[0-9a-f]{64}$/;
const JOURNAL_KEYS = ["schema_version", "candidate_sha256", "tenant_scope_sha256", "state", "classifications", "correction_intents", "journal_sha256"];
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const exactKeys = (value, required, optional = []) => value && typeof value === "object" && !Array.isArray(value)
  && required.every((key) => Object.hasOwn(value, key))
  && Object.keys(value).every((key) => required.includes(key) || optional.includes(key));
const isText = (value) => typeof value === "string" && value.length > 0;

function hasDuplicateJsonKeys(text) {
  let index = 0;
  const whitespace = () => { while (/\s/.test(text[index] ?? "")) index += 1; };
  const stringValue = () => {
    const start = index++;
    while (index < text.length) {
      if (text[index] === "\\") { index += 2; continue; }
      if (text[index++] === '"') break;
    }
    return JSON.parse(text.slice(start, index));
  };
  const value = () => {
    whitespace();
    if (text[index] === '"') { stringValue(); return false; }
    if (text[index] === "[") {
      index += 1; whitespace();
      while (text[index] !== "]") {
        if (value()) return true;
        whitespace();
        if (text[index] === ",") { index += 1; whitespace(); }
      }
      index += 1; return false;
    }
    if (text[index] === "{") {
      index += 1; whitespace();
      const keys = new Set();
      while (text[index] !== "}") {
        const key = stringValue();
        if (keys.has(key)) return true;
        keys.add(key); whitespace(); index += 1; // colon
        if (value()) return true;
        whitespace();
        if (text[index] === ",") { index += 1; whitespace(); }
      }
      index += 1; return false;
    }
    while (index < text.length && !/[\s,\]}]/.test(text[index])) index += 1;
    return false;
  };
  return value();
}

function failClosed() {
  // Snapshot and journal content can contain customer identifiers.
  throw new Error("input or local journal is invalid; no recovery action was produced");
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

function buildPlan(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_INPUT_BYTES) failClosed();
  let input; let inputText;
  try {
    inputText = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    input = JSON.parse(inputText);
  } catch { failClosed(); }
  if (hasDuplicateJsonKeys(inputText)) failClosed();
  if (!exactKeys(input, ["schema_version", "scan_complete", "expected_tenant", "candidates"])
    || input.schema_version !== 1 || input.scan_complete !== true || typeof input.expected_tenant !== "string"
    || !TENANT_UUID.test(input.expected_tenant)
    || !Array.isArray(input.candidates) || input.candidates.length > MAX_CANDIDATES) failClosed();

  const tenantScopeSha = sha256(input.expected_tenant.toLowerCase());
  const unique = new Map();
  let duplicateRows = 0;
  for (const candidate of input.candidates) {
    if (!exactKeys(candidate, ["marker_key", "marker_value", "source_key", "source_value", "acknowledgement"])) failClosed();
    if (!isText(candidate.marker_key) || !candidate.marker_key.startsWith("usage:settled:")
      || !isText(candidate.marker_value) || candidate.marker_value.length > 256
      || !(candidate.source_key === null || isText(candidate.source_key))
      || !(candidate.source_value === null || isText(candidate.source_value))
      || !(candidate.acknowledgement === null || (candidate.acknowledgement && typeof candidate.acknowledgement === "object"))) failClosed();
    if ((candidate.source_key === null) !== (candidate.source_value === null)) failClosed();
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
      if (source.tenant.toLowerCase() !== input.expected_tenant.toLowerCase()) failClosed();
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
      eligible_for_single_correction: disposition === "rejected" && candidate.source_key !== null && candidate.source_value !== null,
    });
  }
  const candidateSha = sha256(bytes);
  const receipt = {
    schema_version: 1,
    mode: "dry-run",
    candidate_sha256: candidateSha,
    counts: {
      scanned_rows: input.candidates.length,
      unique_candidates: unique.size,
      duplicate_rows: duplicateRows,
      classified_candidates: Object.values(counts).reduce((sum, count) => sum + count, 0),
      ...counts,
    },
    planned_actions: {
      eligible_for_single_correction: entries.filter((entry) => entry.eligible_for_single_correction).length,
      local_review_intents: entries.filter((entry) => entry.eligible_for_single_correction).length,
      marker_removal: 0,
      source_deletion: 0,
      automatic_resend: 0,
    },
  };
  return { receipt, entries, tenantScopeSha };
}

/** Classify bounded offline export bytes and return only a redacted receipt. */
export function classifySnapshot(bytes) { return buildPlan(bytes).receipt; }

function sealJournal(journal) {
  const { journal_sha256: _oldDigest, ...unsigned } = journal;
  return { ...unsigned, journal_sha256: sha256(JSON.stringify(unsigned)) };
}

function journalFor(plan, state = "staged", correctionIntents = []) {
  const classifications = Object.fromEntries(plan.entries.map((entry) => [entry.marker_key, {
    source_key: entry.source_key,
    idem_key: entry.idem_key,
    outcome: entry.disposition,
    eligible_for_single_correction: entry.eligible_for_single_correction,
  }]));
  return sealJournal({
    schema_version: 1,
    candidate_sha256: plan.receipt.candidate_sha256,
    tenant_scope_sha256: plan.tenantScopeSha,
    state,
    classifications,
    correction_intents: correctionIntents,
  });
}

function validateJournal(journal, plan) {
  if (!exactKeys(journal, JOURNAL_KEYS) || journal.schema_version !== 1
    || journal.candidate_sha256 !== plan.receipt.candidate_sha256
    || journal.tenant_scope_sha256 !== plan.tenantScopeSha
    || !["staged", "complete", "rolled_back"].includes(journal.state)
    || !exactKeys(journal.classifications, plan.entries.map((entry) => entry.marker_key))
    || !Array.isArray(journal.correction_intents)
    || journal.correction_intents.length > plan.entries.filter((entry) => entry.eligible_for_single_correction).length
    || journal.journal_sha256 !== sealJournal({ ...journal, journal_sha256: undefined }).journal_sha256) failClosed();
  for (const entry of plan.entries) {
    const value = journal.classifications[entry.marker_key];
    if (!exactKeys(value, ["source_key", "idem_key", "outcome", "eligible_for_single_correction"])
      || value.source_key !== entry.source_key || value.idem_key !== entry.idem_key
      || value.outcome !== entry.disposition || value.eligible_for_single_correction !== entry.eligible_for_single_correction) failClosed();
  }
  const intentMarkers = new Set();
  for (const intent of journal.correction_intents) {
    if (!exactKeys(intent, ["marker_key", "source_key", "idem_key", "attempt"])
      || intent.attempt !== 1 || typeof intent.marker_key !== "string" || typeof intent.source_key !== "string"
      || typeof intent.idem_key !== "string" || intentMarkers.has(intent.marker_key)) failClosed();
    const entry = plan.entries.find((item) => item.marker_key === intent.marker_key);
    if (!entry?.eligible_for_single_correction || intent.source_key !== entry.source_key || intent.idem_key !== entry.idem_key) failClosed();
    intentMarkers.add(intent.marker_key);
  }
  if (journal.state === "rolled_back" && journal.correction_intents.length !== 0) failClosed();
  if (journal.state === "staged" && journal.correction_intents.length !== 0) failClosed();
  if (journal.state === "complete" && journal.correction_intents.length
    !== plan.entries.filter((entry) => entry.eligible_for_single_correction).length) failClosed();
}

function processUid() {
  if (typeof process.getuid !== "function") failClosed();
  return process.getuid();
}

async function safePaths(journalPath, { allowMissingFile = false } = {}) {
  const target = path.resolve(journalPath);
  const parentPath = path.dirname(target);
  let cursor = parentPath;
  while (true) {
    let parentStat;
    try { parentStat = await lstat(cursor); } catch { failClosed(); }
    if (parentStat.isSymbolicLink() || (cursor === parentPath && (!parentStat.isDirectory()
      || parentStat.uid !== processUid() || (parentStat.mode & 0o777) !== 0o700))) failClosed();
    const next = path.dirname(cursor);
    if (next === cursor) break;
    cursor = next;
  }
  let fileStat;
  try { fileStat = await lstat(target); } catch (error) { if (error.code !== "ENOENT" || !allowMissingFile) failClosed(); }
  if (fileStat && fileStat.nlink === 2) {
    const tempPattern = new RegExp(`^\\.issue-603-journal-${processUid()}-[0-9a-f]{24}\\.tmp$`);
    const names = (await readdir(parentPath)).filter((name) => tempPattern.test(name));
    const matchingLinks = [];
    for (const name of names) {
      let tempStat;
      try { tempStat = await lstat(path.join(parentPath, name)); } catch { continue; }
      if (tempStat.isFile() && !tempStat.isSymbolicLink() && tempStat.nlink === 2
        && tempStat.uid === processUid() && (tempStat.mode & 0o777) === 0o600
        && tempStat.dev === fileStat.dev && tempStat.ino === fileStat.ino) matchingLinks.push(name);
    }
    if (matchingLinks.length !== 1) failClosed();
    await unlink(path.join(parentPath, matchingLinks[0]));
    const directory = await open(parentPath, fsConstants.O_RDONLY);
    try { await directory.sync(); } finally { await directory.close(); }
    fileStat = await lstat(target);
  }
  if (fileStat && (!fileStat.isFile() || fileStat.isSymbolicLink() || fileStat.nlink !== 1
    || fileStat.uid !== processUid() || (fileStat.mode & 0o777) !== 0o600 || fileStat.size > MAX_JOURNAL_BYTES)) failClosed();
  return { target, parentPath, exists: Boolean(fileStat) };
}

async function readJournal(journalPath, plan) {
  const { target } = await safePaths(journalPath);
  let handle;
  try {
    handle = await open(target, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0));
    const info = await handle.stat();
    if (!info.isFile() || info.nlink !== 1 || info.uid !== processUid() || (info.mode & 0o777) !== 0o600
      || info.size > MAX_JOURNAL_BYTES) failClosed();
    const buffer = Buffer.alloc(MAX_JOURNAL_BYTES + 1);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > MAX_JOURNAL_BYTES) failClosed();
    const bytes = buffer.subarray(0, bytesRead);
    let journal;
    try { journal = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)); } catch { failClosed(); }
    if (!bytes.equals(Buffer.from(`${JSON.stringify(journal)}\n`))) failClosed();
    validateJournal(journal, plan);
    return journal;
  } finally { await handle?.close(); }
}

async function durableWrite(journalPath, journal, { createOnly = false } = {}) {
  const { target, parentPath, exists } = await safePaths(journalPath, { allowMissingFile: true });
  const bytes = Buffer.from(`${JSON.stringify(journal)}\n`);
  if (bytes.byteLength > MAX_JOURNAL_BYTES) failClosed();
  const tempPath = path.join(parentPath, `.issue-603-journal-${processUid()}-${randomBytes(12).toString("hex")}.tmp`);
  let file;
  try {
    file = await open(tempPath, "wx", 0o600);
    await file.writeFile(bytes);
    await file.sync();
    await file.close();
    file = null;
    if (createOnly) {
      if (exists) failClosed();
      await link(tempPath, target);
      await unlink(tempPath);
    } else {
      if (exists) await safePaths(target);
      await rename(tempPath, target);
    }
    const directory = await open(parentPath, fsConstants.O_RDONLY);
    try { await directory.sync(); } finally { await directory.close(); }
  } catch {
    await file?.close().catch(() => {});
    // A leftover private temp file is ignored; atomic rename preserves prior state.
    throw new Error("local journal update did not complete cleanly; inspect the restricted journal before retrying");
  }
}

function redactedJournalReceipt(plan, state, journal) {
  return {
    schema_version: 1,
    mode: state,
    candidate_sha256: plan.receipt.candidate_sha256,
    counts: plan.receipt.counts,
    local_review_intents: journal.correction_intents.length,
    state: journal.state,
    marker_removal: 0,
    source_deletion: 0,
    external_send: 0,
  };
}

async function runCommand(args) {
  const readSnapshot = async (snapshotPath) => {
    let handle;
    try {
      handle = await open(snapshotPath, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0));
      const info = await handle.stat();
      if (!info.isFile() || info.size > MAX_INPUT_BYTES) failClosed();
      const buffer = Buffer.alloc(MAX_INPUT_BYTES + 1);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      if (bytesRead > MAX_INPUT_BYTES) failClosed();
      return buffer.subarray(0, bytesRead);
    } finally { await handle?.close(); }
  };
  const command = args[0];
  if (command === "--dry-run" && args.length === 2) {
    const plan = buildPlan(await readSnapshot(args[1]));
    return plan.receipt;
  }
  if (!["--stage", "--resume", "--rollback"].includes(command) || args.length !== 4 || args[2] !== "--journal") {
    throw new Error("usage: reconcile ... --dry-run SNAPSHOT | --stage SNAPSHOT --journal JOURNAL | --resume SNAPSHOT --journal JOURNAL | --rollback SNAPSHOT --journal JOURNAL");
  }
  const plan = buildPlan(await readSnapshot(args[1]));
  const journalPath = args[3];
  if (command === "--stage") {
    const { exists } = await safePaths(journalPath, { allowMissingFile: true });
    if (exists) failClosed();
    const journal = journalFor(plan);
    validateJournal(journal, plan);
    await durableWrite(journalPath, journal, { createOnly: true });
    return redactedJournalReceipt(plan, "stage", journal);
  }
  const journal = await readJournal(journalPath, plan);
  if (command === "--resume") {
    if (journal.state === "rolled_back" || journal.state === "complete") return redactedJournalReceipt(plan, "resume", journal);
    const intents = plan.entries.filter((entry) => entry.eligible_for_single_correction).map((entry) => ({
      marker_key: entry.marker_key, source_key: entry.source_key, idem_key: entry.idem_key, attempt: 1,
    }));
    const next = journalFor(plan, "complete", intents);
    validateJournal(next, plan);
    await durableWrite(journalPath, next);
    return redactedJournalReceipt(plan, "resume", next);
  }
  if (journal.state !== "rolled_back") {
    const rolledBack = journalFor(plan, "rolled_back", []);
    await durableWrite(journalPath, rolledBack);
    return redactedJournalReceipt(plan, "rollback", rolledBack);
  }
  return redactedJournalReceipt(plan, "rollback", journal);
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
if (!process.env.NODE_TEST_CONTEXT && invokedPath === fileURLToPath(import.meta.url)) {
  runCommand(process.argv.slice(2)).then((receipt) => process.stdout.write(`${JSON.stringify(receipt)}\n`)).catch(() => {
    process.stderr.write("reconciliation command failed; no recovery action was reported\n");
    process.exitCode = 2;
  });
}

// This focused test file is invoked only by the hosted issue-specific workflow.
if (process.env.NODE_TEST_CONTEXT) {
  const fixturePath = new URL("./fixtures/issue-603/recovery-matrix.json", import.meta.url);
  const completedMs = Date.parse("2026-09-01T00:00:00Z");
  const fixtureTemplate = JSON.parse(await readFile(fixturePath, "utf8"));
  const tenant = fixtureTemplate.expected_tenant;
  const ids = Object.fromEntries(fixtureTemplate.cases.filter((item) => item.job_id).map((item) => [
    item.name === "interrupted_missing_source" ? "interrupted" : item.name,
    sha256(`${item.job_id}|2026-09`),
  ]));
  const source = (jobId, sourceTenant = tenant) => JSON.stringify({ jobId, tenant: sourceTenant, startedMs: completedMs - 5_000, completedMs, region: "iad" });
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
  const candidate = (jobId, idem, proof, sourceTenant = tenant) => ({
    marker_key: `usage:settled:${encodeURIComponent(idem)}`,
    marker_value: "1798761600000",
    source_key: `usage:${jobId}`,
    source_value: source(jobId, sourceTenant),
    acknowledgement: proof,
  });
  const byName = new Map();
  const matrix = { schema_version: 1, scan_complete: true, expected_tenant: tenant, candidates: [] };
  for (const item of fixtureTemplate.cases) {
    if (item.duplicate_of) { matrix.candidates.push(structuredClone(byName.get(item.duplicate_of))); continue; }
    const idem = ids[item.name === "interrupted_missing_source" ? "interrupted" : item.name];
    const value = item.source_present === false
      ? { marker_key: `usage:settled:${encodeURIComponent(idem)}`, marker_value: "1798761600000", source_key: null, source_value: null, acknowledgement: null }
      : candidate(item.job_id, idem, item.outcome === null ? null : ack(idem, item.outcome, item.http_status));
    byName.set(item.name, value);
    matrix.candidates.push(value);
  }
  const bytesFor = (value = matrix) => Buffer.from(JSON.stringify(value));
  const { NODE_TEST_CONTEXT: _nodeTestContext, ...childEnv } = process.env;
  const command = (args) => spawnSync(process.execPath, [fileURLToPath(import.meta.url), ...args], { encoding: "utf8", env: childEnv });
  const parseReceipt = (result) => { assert.equal(result.status, 0, result.stderr); return JSON.parse(result.stdout); };
  const writeFixtureFile = async (target, bytes, flags = "wx") => {
    const handle = await open(target, flags, 0o600);
    try { await handle.chmod(0o600); await handle.writeFile(bytes); } finally { await handle.close(); }
  };

  test("classifies exact ACK outcomes with explicit tenant binding and redacted deterministic counts", () => {
    const bytes = bytesFor();
    const result = classifySnapshot(bytes);
    assert.deepEqual(result.counts, { scanned_rows: 8, unique_candidates: 7, duplicate_rows: 1, classified_candidates: 7, accepted: 1, deduped: 1, rejected: 2, conflict: 1, unresolved: 2 });
    assert.deepEqual(result.planned_actions, { eligible_for_single_correction: 2, local_review_intents: 2, marker_removal: 0, source_deletion: 0, automatic_resend: 0 });
    assert.match(result.candidate_sha256, SHA256);
    assert.equal(JSON.stringify(result).includes(tenant), false);
    assert.equal(JSON.stringify(result).includes("job-accepted"), false);
  });

  test("rejects mixed tenants and invalid/missing expected tenant before producing a plan", () => {
    const mixed = structuredClone(matrix);
    mixed.candidates[1].source_value = source("job-deduped", "a6ba2f85-82d4-4c70-9f62-3f6106ddc51b");
    assert.throws(() => classifySnapshot(bytesFor(mixed)));
    for (const expected_tenant of [undefined, "not-a-uuid"]) {
      const malformed = structuredClone(matrix);
      if (expected_tenant === undefined) delete malformed.expected_tenant;
      else malformed.expected_tenant = expected_tenant;
      assert.throws(() => classifySnapshot(bytesFor(malformed)));
    }
    const duplicateField = bytesFor().toString("utf8").replace(`"expected_tenant":"${tenant}"`, `"expected_tenant":"${tenant}","expected_tenant":"${tenant}"`);
    assert.throws(() => classifySnapshot(Buffer.from(duplicateField)));
  });

  test("separate CLI processes stage, resume idempotently, rollback durably, and never mutate markers", async () => {
    const root = await import("node:fs/promises").then(({ mkdtemp }) => mkdtemp(path.join(tmpdir(), "i603-")));
    const snapshot = path.join(root, "snapshot.json"); const journal = path.join(root, "journal.json");
    await writeFixtureFile(snapshot, bytesFor());
    try {
      const stageReceipt = parseReceipt(command(["--stage", snapshot, "--journal", journal]));
      assert.equal(stageReceipt.state, "staged");
      assert.equal(stageReceipt.marker_removal, 0); assert.equal(stageReceipt.source_deletion, 0); assert.equal(stageReceipt.external_send, 0);
      const staged = JSON.parse(await readFile(journal, "utf8"));
      assert.equal(staged.state, "staged"); assert.equal(staged.correction_intents.length, 0);
      const resumeReceipt = parseReceipt(command(["--resume", snapshot, "--journal", journal]));
      assert.equal(resumeReceipt.local_review_intents, 2);
      assert.equal(resumeReceipt.marker_removal, 0); assert.equal(resumeReceipt.source_deletion, 0); assert.equal(resumeReceipt.external_send, 0);
      const completedBytes = await readFile(journal);
      const completed = JSON.parse(completedBytes);
      assert.equal(completed.state, "complete"); assert.equal(completed.correction_intents.length, 2);
      assert.deepEqual(completed.correction_intents.map((intent) => intent.idem_key), [ids.rejected, ids.rejected_second]);
      assert.deepEqual(completed.correction_intents.map((intent) => intent.source_key), ["usage:job-rejected", "usage:job-rejected-second"]);
      parseReceipt(command(["--resume", snapshot, "--journal", journal]));
      assert.deepEqual(await readFile(journal), completedBytes);
      parseReceipt(command(["--rollback", snapshot, "--journal", journal]));
      const rolledBackBytes = await readFile(journal); const rolledBack = JSON.parse(rolledBackBytes);
      assert.equal(rolledBack.state, "rolled_back"); assert.equal(rolledBack.correction_intents.length, 0);
      parseReceipt(command(["--resume", snapshot, "--journal", journal]));
      assert.deepEqual(await readFile(journal), rolledBackBytes);
      parseReceipt(command(["--rollback", snapshot, "--journal", journal]));
      assert.deepEqual(await readFile(journal), rolledBackBytes);
    } finally {
      // Test-only cleanup of this freshly created private fixture directory.
      const { rm } = await import("node:fs/promises"); await rm(root, { recursive: true, force: true });
    }
  });

  test("ignores an interrupted private temp write while preserving the last atomic journal", async () => {
    const root = await import("node:fs/promises").then(({ mkdtemp }) => mkdtemp(path.join(tmpdir(), "i603-")));
    const snapshot = path.join(root, "snapshot.json"); const journal = path.join(root, "journal.json");
    await writeFixtureFile(snapshot, bytesFor());
    try {
      parseReceipt(command(["--stage", snapshot, "--journal", journal]));
      const prior = await readFile(journal);
      const temp = path.join(root, `.issue-603-journal-${processUid()}-interrupted.tmp`);
      await writeFixtureFile(temp, "{");
      const linkedTemp = path.join(root, `.issue-603-journal-${processUid()}-${randomBytes(12).toString("hex")}.tmp`);
      await link(journal, linkedTemp);
      parseReceipt(command(["--resume", snapshot, "--journal", journal]));
      assert.equal(JSON.parse(await readFile(journal, "utf8")).state, "complete");
      assert.notDeepEqual(await readFile(journal), prior);
      await assert.rejects(lstat(linkedTemp), { code: "ENOENT" });
    } finally { const { rm } = await import("node:fs/promises"); await rm(root, { recursive: true, force: true }); }
  });

  test("fails closed on tampered or mismatched journal and unsafe symlink path", async () => {
    const root = await import("node:fs/promises").then(({ mkdtemp }) => mkdtemp(path.join(tmpdir(), "i603-")));
    const snapshot = path.join(root, "snapshot.json"); const journal = path.join(root, "journal.json");
    await writeFixtureFile(snapshot, bytesFor());
    try {
      parseReceipt(command(["--stage", snapshot, "--journal", journal]));
      const valid = JSON.parse(await readFile(journal, "utf8"));
      valid.candidate_sha256 = "0".repeat(64);
      await writeFixtureFile(journal, JSON.stringify(valid), "w");
      assert.notEqual(command(["--resume", snapshot, "--journal", journal]).status, 0);
      const real = path.join(root, "real.json"); await writeFixtureFile(real, "{}");
      const link = path.join(root, "link.json"); await (await import("node:fs/promises")).symlink(real, link);
      assert.notEqual(command(["--stage", snapshot, "--journal", link]).status, 0);
      const hardlink = path.join(root, "hardlink.json"); await (await import("node:fs/promises")).link(real, hardlink);
      assert.notEqual(command(["--stage", snapshot, "--journal", hardlink]).status, 0);
      const unsafeParent = path.join(root, "unsafe-parent"); await (await import("node:fs/promises")).symlink(root, unsafeParent);
      assert.notEqual(command(["--stage", snapshot, "--journal", path.join(unsafeParent, "journal.json")]).status, 0);
      const changed = structuredClone(matrix);
      changed.candidates[0].marker_value = "changed-input";
      const changedSnapshot = path.join(root, "changed.json");
      await writeFixtureFile(changedSnapshot, bytesFor(changed));
      const bindingJournal = path.join(root, "binding.json");
      parseReceipt(command(["--stage", snapshot, "--journal", bindingJournal]));
      assert.notEqual(command(["--resume", changedSnapshot, "--journal", bindingJournal]).status, 0);
      const otherTenant = structuredClone(matrix);
      otherTenant.expected_tenant = "a6ba2f85-82d4-4c70-9f62-3f6106ddc51b";
      otherTenant.candidates = otherTenant.candidates.map((item) => {
        if (item.source_value === null) return item;
        const value = structuredClone(item);
        const parsed = JSON.parse(value.source_value); parsed.tenant = otherTenant.expected_tenant;
        value.source_value = JSON.stringify(parsed);
        return value;
      });
      const otherTenantSnapshot = path.join(root, "other-tenant.json");
      await writeFixtureFile(otherTenantSnapshot, bytesFor(otherTenant));
      assert.notEqual(command(["--resume", otherTenantSnapshot, "--journal", bindingJournal]).status, 0);
      const oversized = path.join(root, "oversized-journal.json");
      await writeFixtureFile(oversized, Buffer.alloc(MAX_JOURNAL_BYTES + 1));
      assert.notEqual(command(["--resume", snapshot, "--journal", oversized]).status, 0);
    } finally { const { rm } = await import("node:fs/promises"); await rm(root, { recursive: true, force: true }); }
  });

  test("fails closed for incomplete, oversized, malformed ACK and conflicting duplicate input", () => {
    assert.throws(() => classifySnapshot(bytesFor({ ...matrix, scan_complete: false })));
    assert.throws(() => classifySnapshot(Buffer.alloc(MAX_INPUT_BYTES + 1)));
    const malformed = structuredClone(matrix);
    malformed.candidates[0].acknowledgement.body.outcomes[0].index = 1;
    malformed.candidates[6].acknowledgement.body.outcomes[0].index = 1;
    assert.equal(classifySnapshot(bytesFor(malformed)).counts.unresolved, 3);
    const conflicting = structuredClone(matrix); conflicting.candidates[6].marker_value = "different";
    assert.throws(() => classifySnapshot(bytesFor(conflicting)));
  });
}
