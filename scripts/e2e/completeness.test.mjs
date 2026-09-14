// THE COMPLETENESS-CRITIC — the machine that proves G1.
//
// Run: `node --test scripts/e2e/completeness.test.mjs` (zero deps; Node >= 18 built-in runner).
// Wired as a CI gate. It fails LOUD if any ledger atom (55 F + 155 S) has no cell, if a cell
// references an atom the ledger doesn't contain, or if a cell is malformed. "Complete" is a
// PROVABLE claim here, not a promise: coverage cannot silently rot.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { F_IDS, S_IDS, ALL_IDS, EXPECTED } from './ledger.mjs';
import { CELLS, SUITES, DIRECTIONS, DISPOSITIONS } from './coverage-map.mjs';

// Exact-segment prefix: 'S1.' matches 'S1.2.3' but NOT 'S10.1'/'S15.1'; 'F-1.' not 'F-10.1'.
const matchesPrefix = (id, prefix) => id.startsWith(prefix);

// Resolve a cell to the concrete ledger atoms it covers (explicit ids + expanded prefixes).
function resolve(cell) {
  const hit = new Set(cell.atoms ?? []);
  for (const p of cell.prefixes ?? []) {
    for (const id of ALL_IDS) if (matchesPrefix(id, p)) hit.add(id);
  }
  return hit;
}

test('ledger parses to the expected atom counts (tripwire on ledger drift)', () => {
  assert.equal(F_IDS.length, EXPECTED.F, `expected ${EXPECTED.F} F-ids, parsed ${F_IDS.length}`);
  assert.equal(S_IDS.length, EXPECTED.S, `expected ${EXPECTED.S} S-ids, parsed ${S_IDS.length}`);
  assert.equal(new Set(ALL_IDS).size, ALL_IDS.length, 'duplicate atom id in the ledger');
});

test('every cell is well-formed', () => {
  const seen = new Set();
  for (const c of CELLS) {
    assert.ok(c.id && !seen.has(c.id), `cell id missing or duplicated: ${c.id}`);
    seen.add(c.id);
    assert.ok(SUITES.includes(c.suite), `${c.id}: unknown suite ${c.suite}`);
    assert.ok(DISPOSITIONS.includes(c.disposition), `${c.id}: unknown disposition ${c.disposition}`);
    assert.ok((c.directions?.length ?? 0) >= 1, `${c.id}: must declare >=1 direction`);
    for (const d of c.directions) assert.ok(DIRECTIONS.includes(d), `${c.id}: bad direction ${d}`);
    assert.ok(c.directions.includes(c.primary), `${c.id}: primary ${c.primary} not in its directions`);
    assert.ok((c.atoms?.length ?? 0) + (c.prefixes?.length ?? 0) >= 1, `${c.id}: binds no atoms`);
  }
});

test('no cell references an atom the ledger does not contain (no dead reference)', () => {
  const universe = new Set(ALL_IDS);
  for (const c of CELLS) {
    for (const a of c.atoms ?? []) {
      assert.ok(universe.has(a), `${c.id}: explicit atom ${a} is not in the ledger`);
    }
    // every prefix must resolve to >= 1 real atom, else it is dead weight / a typo
    for (const p of c.prefixes ?? []) {
      const n = ALL_IDS.filter((id) => matchesPrefix(id, p)).length;
      assert.ok(n >= 1, `${c.id}: prefix "${p}" matches zero ledger atoms`);
    }
  }
});

test('G1 — EVERY ledger atom is covered by >= 1 cell (the completeness proof)', () => {
  const covered = new Set();
  for (const c of CELLS) for (const a of resolve(c)) covered.add(a);
  const orphans = ALL_IDS.filter((id) => !covered.has(id));
  assert.deepEqual(
    orphans, [],
    `${orphans.length} atom(s) have NO cell — coverage is incomplete:\n  ${orphans.join('\n  ')}`,
  );
  assert.equal(covered.size, ALL_IDS.length, 'covered set size must equal the atom universe');
});
