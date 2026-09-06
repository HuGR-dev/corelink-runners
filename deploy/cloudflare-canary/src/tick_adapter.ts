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
      Object.keys(command).length !== 2 ||
      (command as { command?: unknown }).command !== "scheduled-tick" ||
      !Number.isSafeInteger((command as { scheduled_for?: unknown }).scheduled_for) ||
      ((command as { scheduled_for: number }).scheduled_for) <= 0
    )
      return new Response("bad command", { status: 400 });
    const configInvalid = !parseProbeFlag(this.env.FABRIC_PROBES_ENABLED).valid;
    const result = await this.outbox.enqueueAndDrain(
      tickConfig(this.env),
      Date.now(),
      (command as { scheduled_for: number }).scheduled_for,
      configInvalid,
    );
    return new Response(result, { status: 200 });
  }

  async alarm(): Promise<void> {
    await this.outbox.alarm();
  }
}
