ALTER TABLE reservations ADD COLUMN generation TEXT NOT NULL DEFAULT 'g1';
ALTER TABLE reservations ADD COLUMN receipt_envelope_json TEXT;

CREATE TABLE tenant_budgets (
  tenant_id TEXT PRIMARY KEY NOT NULL,
  ceiling_vcpu_ms INTEGER NOT NULL CHECK(ceiling_vcpu_ms > 0 AND ceiling_vcpu_ms <= 1000),
  reserved_vcpu_ms INTEGER NOT NULL DEFAULT 0 CHECK(reserved_vcpu_ms >= 0)
);

INSERT INTO tenant_budgets(tenant_id, ceiling_vcpu_ms, reserved_vcpu_ms)
  SELECT tenant_id, CAST(ceiling_vcpu_ms AS INTEGER), SUM(CAST(required_vcpu_ms AS INTEGER))
  FROM reservations WHERE state = 'prepared' GROUP BY tenant_id;
