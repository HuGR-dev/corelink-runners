import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { createHandler } from "../src/index.js";
import { DynamoOnCallSchedule } from "../src/oncall.js";
import { MemoryStateStore } from "../src/state.js";

const digest = "a".repeat(64);
const now = 1_700_000_000_000;

describe("T6-W15 runtime and on-call hardening", () => {
  it("accepts only the exact two-field scheduler event and uses trusted time as its slot", async () => {
    const run = vi.fn(async (slot: number) => ({ slot }));
    const handler = createHandler({
      config: {} as never,
      ingest: { ingest: vi.fn() },
      recovery: { recover: vi.fn() },
      pageAck: { acknowledge: vi.fn() },
      scheduler: { run },
      clock: { now: async () => ({ timeMs: now }) },
    });
    await expect(handler({ kind: "monitor_tick", source: "aws-scheduler" } as never)).resolves.toMatchObject({ statusCode: 200 });
    expect(run).toHaveBeenCalledWith(now);
    await expect(handler({ kind: "monitor_tick", source: "aws-scheduler", scheduledFor: now } as never)).resolves.toMatchObject({ statusCode: 400 });
    await expect(handler({ rawPath: "/v1/ingest", requestContext: { http: { method: "POST" } } } as never)).resolves.toMatchObject({ statusCode: 400 });
  });

  it("requires a current durable on-call schedule and exact member/action binding", async () => {
    const store = new MemoryStateStore();
    const identity = "arn:aws:iam::123456789012:role/oncall";
    const memberKey = createHash("sha256").update(identity).digest("hex");
    await store.transact([
      { key: "ns:oncall:schedule:current", expectedVersion: null, value: { version: "1", scheduleDigest: digest, destination: "ops", effectiveAt: now - 1, expiresAt: now + 10_000 } },
      { key: `ns:oncall:schedule:${digest}:member:${memberKey}`, expectedVersion: null, value: { version: "1", principalArn: identity, destination: "ops", actions: ["ACK"], notBefore: now - 1, expiresAt: now + 5_000 } },
    ]);
    const schedule = new DynamoOnCallSchedule({ store, namespace: "ns", destination: "ops", scheduleDigest: digest, clock: { now: async () => ({ timeMs: now }) }, ttlMs: 1_000 });
    await expect(schedule.authorize({ identity, destination: "ops", action: "ACK", at: now, scheduleDigest: digest })).resolves.toEqual({ expiresAt: now + 1_000 });
    await expect(schedule.authorize({ identity, destination: "other", action: "ACK", at: now, scheduleDigest: digest })).rejects.toThrow("schedule mismatch");
  });

  it("binds every mandatory runtime authority in the deployment template", async () => {
    const path = join(dirname(fileURLToPath(import.meta.url)), "..", "infra", "monitor-runtime.yaml");
    const template = await readFile(path, "utf8");
    for (const binding of ["MONITOR_CONFIG_JSON", "MONITOR_AUDIT_LOG_ID", "MONITOR_API_ID", "MONITOR_API_STAGE", "MONITOR_ONCALL_ACCOUNT_IDS", "MONITOR_PAGE_ACK_MAX_SKEW_MS", "MONITOR_PAGE_ACK_TTL_MS"]) {
      expect(template).toContain(binding);
    }
    expect(template).toContain(`Input: '{"kind":"monitor_tick","source":"aws-scheduler"}'`);
  });

  it("keeps runtime IAM to the exact state, witness, secret, and KMS authorities", async () => {
    const path = join(dirname(fileURLToPath(import.meta.url)), "..", "infra", "monitor-foundation.yaml");
    const template = await readFile(path, "utf8");
    const runtimePolicy = template.slice(template.indexOf("  RuntimeRole:"), template.indexOf("  SchedulerRole:"));
    for (const required of ["dynamodb:GetItem, dynamodb:Query, dynamodb:TransactWriteItems", "lambda:InvokeFunction, Resource: !Ref WitnessFunctionArn", "secretsmanager:GetSecretValue, Resource: !Ref SourceSecretArns", "!GetAtt IngestKey.Arn", "!GetAtt AckKey.Arn"]) expect(runtimePolicy).toContain(required);
    for (const forbidden of ["dynamodb:PutItem", "dynamodb:UpdateItem", "kms:Decrypt", "kms:GenerateDataKey"]) expect(runtimePolicy).not.toContain(forbidden);
  });
});
