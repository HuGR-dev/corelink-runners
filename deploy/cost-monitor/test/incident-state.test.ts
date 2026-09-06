import { describe, expect, it } from "vitest";
import { evaluateEscalation, evaluateIncident, type Incident, type Signal } from "../src/incidents.js";

const failure = (reason = "down", highWaters = { source: 1 }): Signal => ({ sourceKey: "monitor:a", failing: true, reason, highWaters, monitorTupleDigest: "tuple-1" });
const healthy = (highWaters: Record<string, number>): Signal => ({ sourceKey: "monitor:a", failing: false, reason: "clear", highWaters, monitorTupleDigest: "tuple-1" });

describe("incident reducer", () => {
  it("does not create an incident for healthy unbound state", () => {
    expect(evaluateIncident(null, healthy({ source: 1 }), 0)).toEqual({ incident: null, alerts: [] });
  });

  it("creates one initial alert and makes repeated identical input idempotent", () => {
    const first = evaluateIncident(null, failure(), 1000);
    expect(first.alerts).toHaveLength(1);
    expect(first.alerts[0].kind).toBe("initial");
    expect(evaluateIncident(first.incident, failure(), 1000)).toEqual({ incident: first.incident, alerts: [] });
  });

  it("updates changed failures even after a human acknowledgement", () => {
    const first = evaluateIncident(null, failure(), 0).incident!;
    const acknowledged: Incident = { ...first, humanAcknowledgedAt: 10 };
    const changed = evaluateIncident(acknowledged, failure("changed", { source: 2 }), 20);
    expect(changed.alerts[0].kind).toBe("update");
    expect(changed.incident?.status).toBe("open");
    expect(evaluateEscalation(changed.incident, 300_000).alerts).toEqual([]);
  });

  it("escalates at 300000ms, while acknowledgement suppresses only escalation", () => {
    const incident = evaluateIncident(null, failure(), 0).incident!;
    expect(evaluateEscalation(incident, 299_999).alerts).toEqual([]);
    const escalation = evaluateEscalation(incident, 300_000);
    expect(escalation.alerts[0].kind).toBe("escalation");
    expect(evaluateEscalation(escalation.incident, 600_000).alerts).toEqual([]);
    expect(evaluateEscalation({ ...incident, humanAcknowledgedAt: 1 }, 300_000).alerts).toEqual([]);
  });

  it("requires every initial highwater to advance and observes quiet boundaries", () => {
    let incident = evaluateIncident(null, failure("down", { a: 1, b: 1 }), 0).incident!;
    const partial = evaluateIncident(incident, healthy({ a: 2, b: 1 }), 1);
    expect(partial.incident?.status).toBe("open");
    incident = partial.incident!;
    const recovering = evaluateIncident(incident, healthy({ a: 2, b: 2 }), 2);
    expect(recovering.incident?.status).toBe("recovering");
    incident = recovering.incident!;
    expect(evaluateIncident(incident, healthy({ a: 2, b: 2 }), 329_999).incident?.status).toBe("recovering");
    const recovered = evaluateIncident(incident, healthy({ a: 2, b: 2 }), 330_002);
    expect(recovered.incident?.status).toBe("recovered");
    expect(recovered.alerts[0].kind).toBe("recovery");
  });

  it("resets quiet on regression, UNKNOWN, and subsequent failure", () => {
    let incident = evaluateIncident(null, failure("down", { a: 1 }), 0).incident!;
    incident = evaluateIncident(incident, healthy({ a: 2 }), 10).incident!;
    expect(incident.status).toBe("recovering");
    incident = evaluateIncident(incident, healthy({ a: 1 }), 20).incident!;
    expect(incident.status).toBe("open");
    expect(incident.allClearSince).toBeNull();
    incident = evaluateIncident(incident, healthy({ a: 2 }), 30).incident!;
    expect(incident.status).toBe("recovering");
    incident = evaluateIncident(incident, failure("UNKNOWN", { a: 3 }), 40).incident!;
    expect(incident.status).toBe("open");
    expect(incident.allClearSince).toBeNull();
  });

  it("runs the initial, escalation, recovery sequence once", () => {
    const initial = evaluateIncident(null, failure(), 0);
    const escalation = evaluateEscalation(initial.incident, 300_000);
    const recovering = evaluateIncident(escalation.incident, healthy({ source: 2 }), 300_001);
    const recovered = evaluateIncident(recovering.incident, healthy({ source: 2 }), 630_001);
    expect(initial.alerts.map((a) => a.kind)).toEqual(["initial"]);
    expect(escalation.alerts.map((a) => a.kind)).toEqual(["escalation"]);
    expect(recovering.alerts.map((a) => a.kind)).toEqual(["update"]);
    expect(recovered.alerts.map((a) => a.kind)).toEqual(["recovery"]);
    expect(evaluateIncident(recovered.incident, healthy({ source: 2 }), 700_000).alerts).toEqual([]);
  });

  it("rejects unsafe and regressing trusted time", () => {
    expect(() => evaluateIncident(null, failure(), Number.NaN)).toThrow();
    const incident = evaluateIncident(null, failure(), 10).incident!;
    expect(() => evaluateIncident(incident, failure(), 9)).toThrow();
    expect(() => evaluateEscalation(incident, 9)).toThrow();
  });
});
