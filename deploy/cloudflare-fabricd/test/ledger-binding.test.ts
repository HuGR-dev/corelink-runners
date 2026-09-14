import { beforeEach, describe, expect, it, vi } from "vitest";

const getContainer = vi.fn();
vi.mock("@cloudflare/containers", () => ({
  getContainer: (...args: unknown[]) => getContainer(...args),
  Container: class {
    envVars: Record<string, string> = {};
    constructor(_ctx?: unknown, _env?: unknown) {}
  },
}));

import worker, {
  FabricdContainer,
  resolveLedgerDatabaseUrl,
  type Env,
} from "../src/index";

const legacyUrl = "postgres://legacy.example/ledger";
const b2Url = "postgres://b2.example/ledger";

function baseEnv(overrides: Partial<Env> = {}): Env {
  return {
    FABRICD: {} as Env["FABRICD"],
    CORELINK_INTROSPECT_URL: "https://introspect.example",
    FABRIC_SIGNING_KEY: "signing-key",
    FABRIC_INTROSPECT_AUTH_KEY: "introspect-key",
    DATABASE_URL: legacyUrl,
    FABRIC_PG_DISABLED: "0",
    ...overrides,
  } as Env;
}

beforeEach(() => {
  getContainer.mockReset();
  getContainer.mockImplementation(() => ({
    fetch: () => Promise.resolve(new Response("forwarded", { status: 200 })),
  }));
});

describe("ledger binding selector", () => {
  it("defaults to legacy and ignores B2, including the legacy memory fallback", () => {
    expect(resolveLedgerDatabaseUrl(baseEnv({ DATABASE_URL_B2: b2Url }))).toEqual({
      ok: true,
      databaseUrl: legacyUrl,
    });
    expect(resolveLedgerDatabaseUrl(baseEnv({ DATABASE_URL: undefined, DATABASE_URL_B2: b2Url }))).toEqual({
      ok: true,
      databaseUrl: undefined,
    });
  });

  it("selects B2 while preserving the original secret string", () => {
    expect(
      resolveLedgerDatabaseUrl(
        baseEnv({ FABRIC_DATABASE_URL_SLOT: "b2", DATABASE_URL_B2: `  ${b2Url}  ` }),
      ),
    ).toEqual({ ok: true, databaseUrl: `  ${b2Url}  ` });
  });

  it.each([undefined, "", " ", "\t"]) (
    "rejects invalid B2 selector configuration %j before getContainer",
    async (b2Value) => {
      const response = await worker.fetch(
        new Request("http://fabricd/v1/health"),
        baseEnv({ FABRIC_DATABASE_URL_SLOT: "b2", DATABASE_URL_B2: b2Value }),
      );
      expect(response.status).toBe(503);
      expect(response.headers.get("retry-after")).toBe("1");
      expect(await response.json()).toEqual({ error: "fabricd ledger binding configuration invalid" });
      expect(getContainer).not.toHaveBeenCalled();
    },
  );

  it.each(["", " ", "legacy ", "B2"]) (
    "rejects selector value %j before getContainer",
    async (slot) => {
      const response = await worker.fetch(
        new Request("http://fabricd/v1/health"),
        baseEnv({ FABRIC_DATABASE_URL_SLOT: slot, DATABASE_URL_B2: b2Url }),
      );
      expect(response.status).toBe(503);
      expect(getContainer).not.toHaveBeenCalled();
    },
  );

  it("rejects an empty or unknown selector without disclosing binding details", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await worker.scheduled({} as ScheduledController, baseEnv({
        FABRIC_DATABASE_URL_SLOT: " ",
        DATABASE_URL_B2: "postgres://secret-b2.example/ledger",
      }));
      expect(getContainer).not.toHaveBeenCalled();
      expect(error).toHaveBeenCalledWith("fabricd ledger binding configuration invalid");
      const output = error.mock.calls.flat().join(" ");
      expect(output).not.toContain("postgres://");
      expect(output).not.toContain("FABRIC_DATABASE_URL_SLOT");
      expect(output).not.toContain("B2");
    } finally {
      error.mockRestore();
    }
  });

  it("injects the existing pg env quartet with the selected B2 URL", () => {
    const container = new FabricdContainer({} as DurableObjectState, baseEnv({
      FABRIC_DATABASE_URL_SLOT: "b2",
      DATABASE_URL_B2: b2Url,
    }));
    expect(container.envVars).toMatchObject({
      FABRIC_LEDGER_BACKEND: "pg",
      DATABASE_URL: b2Url,
      FABRIC_PG_TLS: "require",
      FABRIC_RUNNER_VCPU: "4",
    });
    expect(container.envVars).not.toHaveProperty("DATABASE_URL_B2");
    expect(container.envVars).not.toHaveProperty("FABRIC_DATABASE_URL_SLOT");
  });
});
