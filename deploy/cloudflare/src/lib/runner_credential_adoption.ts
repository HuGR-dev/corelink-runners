import { cfAccessHeaders, type MintEnv } from "../lib";

const ADOPTION_ERROR = "runner credential adoption unavailable";
const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function exactNonEmpty(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.trim() === value;
}

function validOperationId(value: unknown): value is string {
  return exactNonEmpty(value) && UUID.test(value);
}

export async function adoptIssuedRunnerCredential(
  env: MintEnv,
  operationId: string,
  patId: string,
): Promise<void> {
  const key = env.CORELINK_RUNNER_MINT_AUTH_KEY;
  if (!exactNonEmpty(key) || !validOperationId(operationId) || !exactNonEmpty(patId)) {
    throw new Error(ADOPTION_ERROR);
  }

  try {
    const response = await fetch(
      `${env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com"}/internal/v1/runner/adopt`,
      {
        method: "POST",
        headers: {
          ...cfAccessHeaders(env),
          "x-corelink-internal-auth": key,
          "content-type": "application/json",
        },
        body: JSON.stringify({ operation_id: operationId, pat_id: patId }),
        signal: AbortSignal.timeout(5_000),
      },
    );
    if (response.status !== 204) throw new Error(ADOPTION_ERROR);
  } catch {
    throw new Error(ADOPTION_ERROR);
  }
}
