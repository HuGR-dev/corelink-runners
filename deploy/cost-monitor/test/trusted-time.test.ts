import { describe, expect, it, vi } from "vitest";
import { Rfc3161Clock, TrustedTimeError } from "../src/trusted_time.js";

const base = {
  endpoint: "https://tsa.invalid/",
  rootPem: "root",
  intermediatePem: "intermediate",
  crlUrls: ["https://tsa.invalid/root.crl"],
  timeoutMs: 500,
  maxResponseBytes: 1024,
  opensslPath: "openssl",
  minimumTimeMs: 0,
  maxAdvanceMs: Number.MAX_SAFE_INTEGER,
  loadFloor: async () => 0,
  commitFloor: async () => true,
};

describe("Rfc3161Clock", () => {
  it("rejects unsafe transport bounds and a missing CRL policy", () => {
    expect(() => new Rfc3161Clock({ ...base, timeoutMs: 5001 })).toThrow(TrustedTimeError);
    expect(() => new Rfc3161Clock({ ...base, crlUrls: [] })).toThrow(TrustedTimeError);
    expect(() => new Rfc3161Clock({ ...base, maxResponseBytes: 256 * 1024 + 1 })).toThrow(TrustedTimeError);
    expect(() => new Rfc3161Clock({ ...base, endpoint: "https://user:pass@tsa.invalid/path?x=1" })).toThrow(TrustedTimeError);
    expect(() => new Rfc3161Clock({ ...base, endpoint: "file:///tmp/tsa" })).toThrow(TrustedTimeError);
    expect(() => new Rfc3161Clock({ ...base, maxAdvanceMs: Number.MAX_SAFE_INTEGER, minimumTimeMs: Number.MAX_SAFE_INTEGER })).not.toThrow();
  });

  it("rejects a floor advance that would overflow the safe integer range", async () => {
    const fetcher = vi.fn<typeof fetch>(async () => new Response(new Uint8Array(2048), { status: 200 }));
    const clock = new Rfc3161Clock({ ...base, fetcher, maxAdvanceMs: Number.MAX_SAFE_INTEGER, loadFloor: async () => Number.MAX_SAFE_INTEGER });
    await expect(clock.now()).rejects.toMatchObject({ code: "INVALID" });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("bounds a timestamp reply before parsing it", async () => {
    const fetcher = vi.fn<typeof fetch>(async () => new Response(new Uint8Array(2048), { status: 200 }));
    const clock = new Rfc3161Clock({ ...base, fetcher });
    await expect(clock.now()).rejects.toMatchObject({ code: "INVALID" });
    expect(fetcher).toHaveBeenCalledWith("https://tsa.invalid/", expect.objectContaining({
      method: "POST",
      redirect: "error",
    }));
  });

  it("does not follow a redirect returned by the timestamp transport", async () => {
    const fetcher = vi.fn<typeof fetch>(async () => new Response(null, { status: 302, headers: { location: "https://other.invalid" } }));
    const clock = new Rfc3161Clock({ ...base, fetcher });
    await expect(clock.now()).rejects.toMatchObject({ code: "UNKNOWN" });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
});
