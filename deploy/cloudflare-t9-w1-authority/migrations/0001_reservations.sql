CREATE TABLE reservations (
  reservation_id TEXT PRIMARY KEY NOT NULL,
  grant_digest TEXT NOT NULL,
  tenant_id TEXT NOT NULL,
  workload_id TEXT NOT NULL,
  required_vcpu_ms TEXT NOT NULL,
  ceiling_vcpu_ms TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('prepared', 'cancelled')),
  created_at_ms INTEGER NOT NULL,
  receipt_json TEXT
);

CREATE INDEX reservations_active_tenant ON reservations(tenant_id, state);
