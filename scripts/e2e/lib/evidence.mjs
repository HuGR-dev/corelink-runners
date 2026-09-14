// G2 evidence emitter — every cell writes a behavioral assertion + captured artifact.
//
// A cell is not "green" on a status code. It records the STIMULUS, the BEHAVIORAL
// assertion (what "correct" means beyond the code), and the ARTIFACT (the raw captured
// evidence). Records land in docs/validation/evidence/<run-id>/ keyed by cell id.
import { mkdirSync, writeFileSync } from 'node:fs';

const root = new URL('../../../', import.meta.url);

// One run-id per suite invocation. Deterministic-ish, human-sortable.
function runId() {
  if (process.env.E2E_RUN_ID) return process.env.E2E_RUN_ID;
  const d = new Date();
  const p = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getUTCFullYear()}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}-${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}`;
}

export class Evidence {
  constructor(suite) {
    this.suite = suite;
    this.runId = runId();
    this.dir = new URL(`docs/validation/evidence/${this.runId}/`, root);
    mkdirSync(this.dir, { recursive: true });
    this.records = [];
  }

  // Record one cell's evidence. `pass` is the caller's behavioral verdict (not just a code).
  record({ cell, atoms, direction, grade, stimulus, assertion, pass, artifact }) {
    const rec = {
      cell, atoms, direction, grade, suite: this.suite,
      stimulus, assertion, pass: !!pass, artifact,
      ts: new Date().toISOString(),
    };
    this.records.push(rec);
    writeFileSync(new URL(`${cell}.json`, this.dir), JSON.stringify(rec, null, 2) + '\n');
    return rec;
  }

  // Flush the run index — atom -> grade -> cited artifact, the campaign deliverable shape.
  flush() {
    const index = {
      suite: this.suite, runId: this.runId, ts: new Date().toISOString(),
      cells: this.records.length,
      passed: this.records.filter((r) => r.pass).length,
      records: this.records.map((r) => ({
        cell: r.cell, atoms: r.atoms, direction: r.direction, grade: r.grade,
        pass: r.pass, assertion: r.assertion,
      })),
    };
    writeFileSync(new URL('index.json', this.dir), JSON.stringify(index, null, 2) + '\n');
    return { dir: this.dir.pathname, ...index };
  }
}
