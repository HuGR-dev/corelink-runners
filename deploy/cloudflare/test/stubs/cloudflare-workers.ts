// Node/vitest stub for the workerd-only `cloudflare:workers` virtual module.
// index.ts imports `{ DurableObject }` from it at module top-level; plain vitest
// (node) cannot resolve the virtual module, so vitest.config.ts aliases it here.
// This only needs to satisfy MODULE LOADING (`class X extends DurableObject`) —
// the DO's storage-backed methods are exercised via the pure `decideRedeem`
// (unit-tested) and the live deploy exit-test, never this stub.
export class DurableObject<E = unknown> {
  ctx: unknown;
  env: E;
  constructor(ctx: unknown, env: E) {
    this.ctx = ctx;
    this.env = env;
  }
}
