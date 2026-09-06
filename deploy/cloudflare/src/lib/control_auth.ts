import { safeEqual } from "../lib";

export interface ControlAuthEnv {
  CLOUDFLARE_SPAWN_AUTH_TOKEN?: string;
  CLOUDFLARE_EXEC_AUTH_TOKEN?: string;
  CLOUDFLARE_LIFECYCLE_AUTH_TOKEN?: string;
}

type AuthDomain = "spawn" | "exec" | "lifecycle";

function configured(value: string | undefined): value is string {
  return value !== undefined && value.length > 0 && safeEqual(value, value.trim());
}

function routeDomain(request: Request): AuthDomain | undefined {
  const url = new URL(request.url);
  if (request.method === "POST" && url.pathname === "/v1/spawn") return "spawn";
  if (request.method === "POST" && url.pathname === "/v1/exec") return "exec";
  if (request.method === "POST" && url.pathname === "/v1/status") return "lifecycle";
  if (request.method === "GET" && /^\/v1\/jobs\/[^/]+\/status$/.test(url.pathname)) return "lifecycle";
  if (request.method === "GET" && url.pathname.startsWith("/v1/status/")) {
    return url.pathname.slice("/v1/status/".length).length > 0 ? "lifecycle" : undefined;
  }
  if (request.method === "POST" && url.pathname === "/v1/teardown") return "lifecycle";
  if (request.method === "POST" && url.pathname === "/v1/egress-cutoff") return "lifecycle";
  return undefined;
}

/** Authenticate the three Worker control domains with independent credentials. */
export function controlAuthed(request: Request, env: ControlAuthEnv): boolean {
  const spawn = env.CLOUDFLARE_SPAWN_AUTH_TOKEN;
  const exec = env.CLOUDFLARE_EXEC_AUTH_TOKEN;
  const lifecycle = env.CLOUDFLARE_LIFECYCLE_AUTH_TOKEN;
  if (!configured(spawn) || !configured(exec) || !configured(lifecycle)) return false;
  if (safeEqual(spawn, exec) || safeEqual(spawn, lifecycle) || safeEqual(exec, lifecycle)) {
    return false;
  }

  const domain = routeDomain(request);
  if (!domain) return false;
  const token = domain === "spawn" ? spawn : domain === "exec" ? exec : lifecycle;
  return safeEqual(request.headers.get("authorization") ?? "", `Bearer ${token}`);
}
