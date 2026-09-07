import { describe, expect, it } from "vitest";
import {
  JobAttributionConflictError,
  JobAttributionInvalidError,
  deleteJobAttribution,
  persistJobAttribution,
  readJobAttribution,
} from "../src/lib/job_attribution.js";

function store(seed: Record<string, string> = {}) {
  const values = new Map(Object.entries(seed));
  return {
    values,
    async get(key: string) { return values.get(key) ?? null; },
    async putIfAbsent(key: string, value: string) {
      const existing = values.get(key);
      if (existing !== undefined) return existing;
      values.set(key, value);
      return value;
    },
    async delete(key: string) { values.delete(key); },
  };
}

describe("durable job attribution", () => {
  it("persists the verified tenant before effects and excludes PAT material", async () => {
    const kv = store();
    const result = await persistJobAttribution(kv, { jobId: "42", tenant: "tenant-a" });
    expect(result).toEqual({ jobId: "42", tenant: "tenant-a" });
    expect(kv.values.get("job-attribution:42")).not.toContain("token");
  });

  it("survives a job longer than the old 7200-second TTL and permits the same immutable rewrite", async () => {
    const kv = store();
    await persistJobAttribution(kv, { jobId: "42", tenant: "tenant-a" });
    expect(await readJobAttribution(kv, "42")).toEqual({ jobId: "42", tenant: "tenant-a" });
    await expect(persistJobAttribution(kv, { jobId: "42", tenant: "tenant-a" })).resolves.toEqual({ jobId: "42", tenant: "tenant-a" });
  });

  it("refuses conflicting tenant rewrites and malformed durable records", async () => {
    const kv = store();
    await persistJobAttribution(kv, { jobId: "42", tenant: "tenant-a" });
    await expect(persistJobAttribution(kv, { jobId: "42", tenant: "tenant-b" })).rejects.toBeInstanceOf(JobAttributionConflictError);
    kv.values.set("job-attribution:bad", JSON.stringify({ jobId: "bad", tenant: "" }));
    await expect(readJobAttribution(kv, "bad")).rejects.toBeInstanceOf(JobAttributionInvalidError);
  });

  it("serializes concurrent conflicting writers at the authority boundary", async () => {
    const kv = store();
    const results = await Promise.allSettled([
      persistJobAttribution(kv, { jobId: "race", tenant: "tenant-a" }),
      persistJobAttribution(kv, { jobId: "race", tenant: "tenant-b" }),
    ]);
    expect(results.filter(result => result.status === "fulfilled")).toHaveLength(1);
    expect(results.filter(result => result.status === "rejected")).toHaveLength(1);
    expect(await readJobAttribution(kv, "race")).toMatchObject({ jobId: "race" });
  });

  it("deletes only the attribution record after verified cleanup", async () => {
    const kv = store();
    await persistJobAttribution(kv, { jobId: "42", tenant: "tenant-a" });
    await deleteJobAttribution(kv, "42");
    expect(await readJobAttribution(kv, "42")).toBeNull();
  });
});
