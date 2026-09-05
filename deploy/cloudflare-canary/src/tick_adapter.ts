import { parseProbeFlag, tickConfig } from "./config";
import { CanaryTickOutbox } from "./tick_outbox";

/** Durable Object HTTP boundary. Only a small JSON command crosses from the
 * Worker; all secret-bearing configuration stays in this isolate's Env. */
export class CanaryTickOutboxAdapter {
  private readonly outbox: CanaryTickOutbox;

  constructor(state: DurableObjectState) {
    this.outbox = new CanaryTickOutbox(state);
  }

  async fetch(request: Request, env: Record<string, string | undefined>): Promise<Response> {
    if (request.method !== "POST") return new Response("method not allowed", { status: 405 });
    let command: unknown;
    try { command = await request.json(); } catch { return new Response("bad command", { status: 400 }); }
    if (!command || typeof command !== "object" || Array.isArray(command) || Object.keys(command).length !== 1 || (command as { command?: unknown }).command !== "scheduled-tick") return new Response("bad command", { status: 400 });
    const configInvalid = !parseProbeFlag(env.FABRIC_PROBES_ENABLED).valid;
    const result = await this.outbox.enqueueAndDrain(tickConfig(env), Date.now(), configInvalid);
    return new Response(result, { status: 200 });
  }

  async alarm(): Promise<void> { await this.outbox.alarm(); }
}
