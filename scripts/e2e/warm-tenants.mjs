// Warm the per-tenant plan cache before the journeys run — deterministic on a fresh container.
//
// `/v1/usage` reads `plan_of`, a per-tenant cache warmed by `plan_of_resolving` on ACQUIRE
// (usage.rs:88 — a deliberate cheap-read design). Right after a container roll it is COLD, so a
// tenant that hasn't acquired reads `plan_cap: null` and any cap-asserting journey flakes. One
// throwaway acquire+close per tenant warms it, making the suite deterministic regardless of
// container boot state. No-op unless E2E_LIVE=1. Idempotent + self-cleaning (short leases, closed).
import { tenantPats, acquire, closeLease, loadPat, LIVE } from './lib/fabric.mjs';

if (!LIVE) {
  process.exit(0);
}

const P = tenantPats();
const pats = [P.free, P.solo, P.pro, P.enterprise, P.tenantB, P.tenantBAdmin, P.ro, P.rw, loadPat()]
  .filter(Boolean)
  // de-dupe (ro/rw/admin may share a tenant with pro/free)
  .filter((p, i, a) => a.indexOf(p) === i);

let warmed = 0;
for (const pat of pats) {
  try {
    const a = await acquire(pat, { expiryMs: 12000, tmpRoot: `/tmp/warm-${warmed}-${Date.now()}` });
    if (a.leaseId) {
      await closeLease(pat, a.leaseId);
      warmed += 1;
    }
  } catch {
    /* best-effort — a tenant that can't acquire just stays cold; its journeys handle it */
  }
}
console.log(`[warm] plan caches warmed for ${warmed}/${pats.length} tenants`);
