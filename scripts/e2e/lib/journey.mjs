// The story-driven Journey runner — a real user's narrative, with state, as a test.
//
// A Journey is a NAMED user story (mapped to S-ids) run by a PERSONA (a real tenant PAT) as an
// ordered sequence of STEPS. State threads through `ctx` from step to step (lease ids, caps,
// observed values). Each step asserts the DEFINED behavior and captures its evidence. The first
// violated step fails the whole journey (a story that breaks mid-way is a failure, like for a
// real user). Cleanup ALWAYS runs (close every lease opened) so journeys never leak real boxes.
//
// The emitted evidence is the NARRATIVE: the full step-by-step trace with each step's assertion
// and captured artifact — not an isolated status code.
import { mkdirSync, writeFileSync } from 'node:fs';

const root = new URL('../../../', import.meta.url);

export class Journey {
  constructor(name, { sid = [], persona = 'user', atoms = [] } = {}) {
    this.name = name;
    this.sid = Array.isArray(sid) ? sid : [sid];
    this.persona = persona;
    this.atoms = atoms;
    this._steps = [];
    this._cleanups = [];
    this.trace = [];
    this.ctx = {};
  }

  // Add a narrative step. `fn(ctx)` performs the action and returns
  // { ok, assertion, artifact } — ok:false (or a throw) fails the journey here.
  step(title, fn) { this._steps.push({ title, fn }); return this; }

  // Register a teardown that ALWAYS runs (e.g. close a lease), even if a step failed.
  onCleanup(fn) { this._cleanups.push(fn); return this; }

  async run() {
    let failedAt = null;
    for (const s of this._steps) {
      let entry;
      try {
        const res = (await s.fn(this.ctx)) || {};
        const ok = res.ok !== false;
        entry = { step: s.title, ok, assertion: res.assertion || s.title, artifact: res.artifact ?? null };
      } catch (e) {
        entry = { step: s.title, ok: false, assertion: `threw: ${e.message}`, artifact: null };
      }
      this.trace.push(entry);
      if (!entry.ok) { failedAt = s.title; break; }
    }
    for (const c of this._cleanups) { try { await c(this.ctx); } catch { /* best-effort teardown */ } }
    this._emit(failedAt);
    if (failedAt) {
      const why = this.trace.find((t) => !t.ok);
      throw new Error(`Journey "${this.name}" FAILED at step "${failedAt}": ${why?.assertion}`);
    }
    return this.trace;
  }

  _emit(failedAt) {
    const rid = process.env.E2E_RUN_ID || 'journeys';
    const dir = new URL(`docs/validation/evidence/${rid}/`, root);
    mkdirSync(dir, { recursive: true });
    const rec = {
      journey: this.name, sid: this.sid, persona: this.persona, atoms: this.atoms,
      pass: !failedAt, failedAt: failedAt ?? null,
      steps: this.trace.length, narrative: this.trace,
      ts: new Date().toISOString(),
    };
    const slug = this.name.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '').toLowerCase();
    writeFileSync(new URL(`journey-${slug}.json`, dir), JSON.stringify(rec, null, 2) + '\n');
  }
}

// Sugar for a step result.
export const pass = (assertion, artifact) => ({ ok: true, assertion, artifact });
export const fail = (assertion, artifact) => ({ ok: false, assertion, artifact });
export const check = (cond, assertion, artifact) => ({ ok: !!cond, assertion, artifact });
