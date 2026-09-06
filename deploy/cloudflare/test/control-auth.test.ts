import { describe, expect, it } from "vitest";
import { controlAuthed, type ControlAuthEnv } from "../src/lib/control_auth";

const env: ControlAuthEnv = {
  CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-token",
  CLOUDFLARE_EXEC_AUTH_TOKEN: "exec-token",
  CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle-token",
};

const routes = [
  ["POST", "/v1/spawn", "CLOUDFLARE_SPAWN_AUTH_TOKEN"],
  ["POST", "/v1/exec", "CLOUDFLARE_EXEC_AUTH_TOKEN"],
  ["POST", "/v1/status", "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"],
  ["GET", "/v1/status/handle", "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"],
  ["POST", "/v1/teardown", "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"],
  ["POST", "/v1/egress-cutoff", "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"],
  ["POST", "/internal/v1/tenant-suspension", "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"],
] as const;

function request(method: string, path: string, token?: string): Request {
  return new Request(`https://worker.test${path}`, {
    method,
    headers: token === undefined ? undefined : { authorization: `Bearer ${token}` },
  });
}

describe("controlAuthed", () => {
  it.each(routes)("accepts the matching token for %s %s", (method, path, key) => {
    expect(controlAuthed(request(method, path, env[key]), env)).toBe(true);
  });

  it.each(routes)("rejects the other two tokens for %s %s", (method, path, key) => {
    for (const other of Object.keys(env).filter((candidate) => candidate !== key) as Array<keyof ControlAuthEnv>) {
      expect(controlAuthed(request(method, path, env[other]), env)).toBe(false);
    }
  });

  it("rejects missing, blank, trimmed, and duplicate configuration", () => {
    for (const key of Object.keys(env) as Array<keyof ControlAuthEnv>) {
      const missing = { ...env };
      delete missing[key];
      expect(controlAuthed(request("POST", "/v1/spawn", "spawn-token"), missing)).toBe(false);
      for (const value of ["", " ", " token", "token "]) {
        expect(controlAuthed(request("POST", "/v1/spawn", "spawn-token"), { ...env, [key]: value })).toBe(false);
      }
    }
    expect(controlAuthed(request("POST", "/v1/spawn", "same"), {
      ...env,
      CLOUDFLARE_SPAWN_AUTH_TOKEN: "same",
      CLOUDFLARE_EXEC_AUTH_TOKEN: "same",
    })).toBe(false);
  });

  it("invalidates the replaced token and accepts the replacement", () => {
    const rotated = { ...env, CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-rotated" };
    expect(controlAuthed(request("POST", "/v1/spawn", "spawn-token"), rotated)).toBe(false);
    expect(controlAuthed(request("POST", "/v1/spawn", "spawn-rotated"), rotated)).toBe(true);
  });

  it("requires the exact bearer form and rejects unknown routes or methods", () => {
    expect(controlAuthed(request("POST", "/v1/spawn", "Bearer spawn-token"), env)).toBe(false);
    expect(controlAuthed(new Request("https://worker.test/v1/spawn", { method: "POST", headers: { authorization: "bearer spawn-token" } }), env)).toBe(false);
    for (const [method, path] of [["GET", "/v1/spawn"], ["POST", "/v1/status/"], ["GET", "/v1/status"], ["POST", "/v1/unknown"]]) {
      expect(controlAuthed(request(method, path, "spawn-token"), env)).toBe(false);
    }
  });

  it("rejects malformed non-ASCII bearer values without throwing", () => {
    expect(() => controlAuthed(request("POST", "/v1/spawn", "spéwn-token"), env)).not.toThrow();
    expect(controlAuthed(request("POST", "/v1/spawn", "spéwn-token"), env)).toBe(false);
  });
});
