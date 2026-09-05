import { parseProbeFlag, tickConfig } from "./config";
import { CanaryTickOutbox } from "./tick_outbox";

/** Durable Object HTTP boundary. Only a small JSON command crosses from the
 * Worker; all secret-bearing configuration stays in this isolate's Env. */
export class CanaryTickOutboxAdapter {
  private readonly outbox: CanaryTickOutbox;
  private readonly env: Record<string, string | undefined>;

  constructor(
    state: DurableObjectState,
    env: Record<string, string | undefined>,
  ) {
    this.outbox = new CanaryTickOutbox(state);
    this.env = env;
  }

  async fetch(request: Request): Promise<Response> {
    if (request.method !== "POST")
      return new Response("method not allowed", { status: 405 });
    let command: unknown;
    try {
      command = await request.json();
    } catch {
      return new Response("bad command", { status: 400 });
    }
    if (
      !command ||
      typeof command !== "object" ||
      Array.isArray(command) ||
      Object.keys(command).length !== 1 ||
      (command as { command?: unknown }).command !== "scheduled-tick"
    )
      return new Response("bad command", { status: 400 });
    const configInvalid = !parseProbeFlag(this.env.FABRIC_PROBES_ENABLED).valid;
    const result = await this.outbox.enqueueAndDrain(
      tickConfig(this.env),
      Date.now(),
      configInvalid,
    );
    return new Response(result, { status: 200 });
  }

  async alarm(): Promise<void> {
    await this.outbox.alarm();
  }
}
