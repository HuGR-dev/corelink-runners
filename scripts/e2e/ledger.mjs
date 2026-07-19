// The atom universe — parsed LIVE from the product ledger, never hand-transcribed.
// This is what makes G1 (completeness) drift-proof: the coverage map is checked against
// whatever the ledger actually contains right now, so adding an F-id/S-id to the ledger
// forces a cell to exist (the critic goes red until it does). No frozen copy to rot.
import { readFileSync } from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (rel) => readFileSync(new URL(rel, root), 'utf8');

// H3 cards. Features: "### F-4.10 …". Scenarios: "### S1.2.1 — …" (every S-id has >=1 dot).
const F_RE = /^###\s+(F-\d+\.\d+)\b/gm;
const S_RE = /^###\s+(S\d+(?:\.\d+)+)\b/gm;

function ids(text, re) {
  const out = [];
  for (const m of text.matchAll(re)) out.push(m[1]);
  return out;
}

export const F_IDS = ids(read('docs/product/FEATURES.md'), F_RE);
export const S_IDS = ids(read('docs/product/USE-SCENARIOS.md'), S_RE);
export const ALL_IDS = [...F_IDS, ...S_IDS];

// Frozen expected counts — a second, independent tripwire. If the ledger grows/shrinks,
// this fails loud so a coverage-map update is a conscious act, not a silent slide.
export const EXPECTED = { F: 55, S: 155 };
