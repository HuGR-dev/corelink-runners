// Node/vitest stub for the workerd-only `cloudflare:workers` virtual module.
// index.ts imports `{ DurableObject }` from it at module top-level; plain vitest
// (node) cannot resolve the virtual module, so vitest.config.ts aliases it here.
// This only needs to satisfy MODULE LOADING (`class X extends DurableObject`).
export class DurableObject<E = unknown> {
  ctx: unknown;
  env: E;
  constructor(ctx: unknown, env: E) {
    this.ctx = ctx;
    this.env = env;
  }
}
