import { describe, expect, it } from "vitest";
import {
  CLW_EXEC_ENVELOPE_MAX_BYTES,
  CLW_SNAPSHOT_REPORT_MAX_BYTES,
  parseClwExecResponse,
  parseClwSnapshotReport,
} from "../src/lib/clw";

const NAME = "workspace-test";
const ROOT = "0123456789abcdef".repeat(4);

function report(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    name: NAME,
    root: ROOT,
    files: 0,
    bytes_total: 0,
    chunks_total: 0,
    chunks_uploaded: 0,
    unchanged: false,
    skipped_external_symlinks: [],
    ...overrides,
  };
}

describe("DevEnv clw snapshot report contract", () => {
  it("accepts a valid empty snapshot with its real content digest", () => {
    expect(parseClwSnapshotReport(JSON.stringify(report()), NAME)).toEqual({
      root: ROOT,
      bytesTotal: 0,
      files: 0,
      chunksTotal: 0,
      chunksUploaded: 0,
      unchanged: false,
    });
  });

  const malformedReports: Array<[string, string]> = [
    ["empty output", ""],
    ["empty JSON object", "{}"],
    ["null", "null"],
    ["truncated JSON", '{"name":"workspace-test","root":"'],
    ["missing required counters", JSON.stringify({ name: NAME, root: ROOT, bytes_total: 0 })],
    ["unexpected incompatible field", JSON.stringify(report({ command: "hydrate" }))],
    ["wrong result name", JSON.stringify(report({ name: "other-workspace" }))],
    ["empty root", JSON.stringify(report({ root: "" }))],
    ["path-like root", JSON.stringify(report({ root: "/tmp/root" }))],
    ["non-canonical digest", JSON.stringify(report({ root: "A".repeat(64) }))],
    ["null digest", JSON.stringify(report({ root: null }))],
    ["negative byte count", JSON.stringify(report({ bytes_total: -1 }))],
    ["fractional file count", JSON.stringify(report({ files: 0.5 }))],
    ["coercible byte count", JSON.stringify(report({ bytes_total: "0" }))],
    ["boolean chunk count", JSON.stringify(report({ chunks_total: false }))],
    ["unsafe byte count", JSON.stringify(report({ bytes_total: Number.MAX_SAFE_INTEGER + 1 }))],
    ["uploaded exceeds total", JSON.stringify(report({ chunks_total: 1, chunks_uploaded: 2 }))],
    ["empty files with nonzero bytes", JSON.stringify(report({ files: 0, bytes_total: 1, chunks_total: 1, chunks_uploaded: 1 }))],
    ["bytes without a file", JSON.stringify(report({ files: 0, bytes_total: 1 }))],
    ["chunks without bytes", JSON.stringify(report({ chunks_total: 1 }))],
    ["more chunks than bytes", JSON.stringify(report({ bytes_total: 1, files: 1, chunks_total: 2, chunks_uploaded: 1 }))],
    ["unchanged report with uploads", JSON.stringify(report({ bytes_total: 1, files: 1, chunks_total: 1, chunks_uploaded: 1, unchanged: true }))],
    ["invalid unchanged flag", JSON.stringify(report({ unchanged: 0 }))],
    ["invalid skipped path entry", JSON.stringify(report({ skipped_external_symlinks: [""] }))],
    ["oversized CLI output", JSON.stringify(report({ skipped_external_symlinks: ["x".repeat(CLW_SNAPSHOT_REPORT_MAX_BYTES)] }))],
  ];

  it.each(malformedReports)("rejects %s", (_label, stdout) => {
    expect(() => parseClwSnapshotReport(stdout, NAME)).toThrow();
  });
});

describe("DevEnv exec-server envelope", () => {
  it("keeps stdout and stderr distinct for a valid command result", async () => {
    const result = await parseClwExecResponse(new Response(JSON.stringify({
      exit_code: 0,
      stdout: JSON.stringify(report()),
      stderr: "diagnostic only",
    })));
    expect(result).toEqual({ exitCode: 0, stdout: JSON.stringify(report()), stderr: "diagnostic only" });
  });

  const malformedEnvelopes: Array<[string, Record<string, unknown>]> = [
    ["missing fields", { exit_code: 0, stdout: "{}" }],
    ["extra envelope field", { exit_code: 0, stdout: "{}", stderr: "", command: "hydrate" }],
    ["coercible exit code", { exit_code: "0", stdout: "{}", stderr: "" }],
    ["boolean stdout", { exit_code: 0, stdout: false, stderr: "" }],
  ];

  it.each(malformedEnvelopes)("rejects an envelope with %s", async (_label, body) => {
    await expect(parseClwExecResponse(new Response(JSON.stringify(body)))).rejects.toThrow();
  });

  it("rejects invalid JSON and over-limit response bodies", async () => {
    await expect(parseClwExecResponse(new Response("{"))).rejects.toThrow("CLW_EXEC_ENVELOPE_INVALID_JSON");
    await expect(parseClwExecResponse(new Response("x".repeat(CLW_EXEC_ENVELOPE_MAX_BYTES + 1))))
      .rejects.toThrow("CLW_EXEC_ENVELOPE_INVALID_SIZE");
  });
});
