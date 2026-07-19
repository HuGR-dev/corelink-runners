// The coverage map — the declared cells of the e2e validation suite.
//
// Each cell binds a set of ledger atoms (F-ids / S-ids, by explicit id or by prefix) to:
//   - suite     : which test-suite runs it (TS-1..TS-6)
//   - primary   : the primary direction the cell is authored around
//   - directions: the direction set the suite must implement for this cell
//                  (happy · edge · adversarial · failure) — not every atom admits all four;
//                  a cell declares the ones its nature supports (honest, not aspirational).
//   - grade     : the highest evidence grade this cell reaches HERE (E0..E5 / X4)
//   - disposition: fabricable | owner-gated | x4 | campaign2
//
// The completeness-critic (completeness.test.mjs) expands prefixes against the LIVE ledger
// and asserts every atom lands in >= 1 cell. Overlap between cells is fine and expected
// (a live journey exercises many capabilities). Coverage — not partition — is the invariant.
//
// Prefix matching is exact-segment: 'S1.' matches S1.x.y but NOT S10./S15.; 'F-1.' matches
// F-1.x but NOT F-10.x (see the critic's startsWith checks).

const D_ALL = ['happy', 'edge', 'adversarial', 'failure'];

export const CELLS = [
  // ─────────────────────────── TS-2 · live-journey (the real user in prod) ──────────────
  { id: 'TS2-doorA-dogfood', suite: 'TS-2', primary: 'happy', directions: D_ALL, grade: 'E3',
    disposition: 'fabricable',
    atoms: ['F-2.1', 'F-7.1', 'F-5.8', 'F-4.3', 'F-6.1', 'F-6.5', 'F-4.1', 'F-5.1', 'F-10.3'],
    prefixes: ['S1.'], // P1 developer — the primary direct-ICP journey
    note: 'Door-A: a real runs-on:corelink job on the dogfood App install (144561227).' },

  { id: 'TS2-doorB-authed', suite: 'TS-2', primary: 'happy', directions: D_ALL, grade: 'E2',
    disposition: 'fabricable',
    atoms: ['F-5.1', 'F-4.1', 'F-8.2', 'F-4.10', 'F-5.4', 'F-9.1', 'F-2.3', 'F-9.5'],
    prefixes: ['S8.'], // P8 power-user — corelink run / acquire against the live fabric
    note: 'Door-B authenticated: f0005 tenant PAT proven live at review (400 not 401).' },

  { id: 'TS2-moat', suite: 'TS-2', primary: 'happy', directions: ['happy', 'edge', 'failure'],
    grade: 'E2', disposition: 'fabricable',
    atoms: ['F-5.9', 'F-4.3', 'F-1.2', 'F-6.1', 'F-10.1', 'F-10.2'],
    prefixes: ['S11.'], // P11 support — warm-boot / mint-redeem observed via counters
    note: 'mint->redeem is E2 live; full [clw] cache-hit tail is X4 (needs real CoreLink content).' },

  { id: 'TS2-reads-and-lifecycle', suite: 'TS-2', primary: 'happy', directions: D_ALL, grade: 'E2',
    disposition: 'fabricable',
    atoms: ['F-5.1', 'F-5.2', 'F-4.2'],
    prefixes: ['S9.', 'S13.', 'S14.'], // migrate · lifecycle · usage-API reads
    note: 'tenant-scoped /v1 reads via the f0005 PAT; tier/GDPR edges are owner-gated (per-atom).' },

  { id: 'TS2-ops-signals', suite: 'TS-2', primary: 'happy', directions: ['happy', 'failure'],
    grade: 'E2', disposition: 'fabricable',
    atoms: ['F-7.2', 'F-10.1', 'F-10.2', 'F-10.3', 'F-6.4'],
    prefixes: ['S15.'], // P15 SRE — golden signals, restart honesty
    note: 'operator reads golden counters on /internal/v1/status (evidence-only, not a user stimulus).' },

  // ─────────────────────────── TS-3 · stress (E4) ───────────────────────────────────────
  { id: 'TS3-concurrency-burst', suite: 'TS-3', primary: 'edge', directions: ['happy', 'edge', 'failure'],
    grade: 'E4', disposition: 'fabricable',
    atoms: ['F-5.2', 'F-1.3', 'F-1.4', 'F-1.6', 'F-1.1'],
    prefixes: [], // driven, not a persona-story; asserts atomic slot + fleet cap + zero thrash
    note: 'burst to fleet-cap + a bit over; owner-approved budget = cap+a-bit.' },

  // ─────────────────────────── TS-4 · chaos (E5) — RUN gated on owner ───────────────────
  { id: 'TS4-resilience', suite: 'TS-4', primary: 'failure', directions: ['failure'], grade: 'E5',
    disposition: 'fabricable',
    atoms: ['F-4.6', 'F-5.5', 'F-5.3', 'F-7.2', 'F-7.3', 'F-5.8'],
    prefixes: ['S5.'], // P5 operator — kill/inject/recover; the disruptive live set
    note: 'kill fabricd mid-flight, inject spawn-fail->dead-letter, deps-down, force a real canary breach. RUN is owner-gated.' },

  // ─────────────────────────── TS-5 · security-adversarial (E1/E2 + live) ───────────────
  { id: 'TS5-adversary', suite: 'TS-5', primary: 'adversarial', directions: ['adversarial', 'failure'],
    grade: 'E2', disposition: 'fabricable',
    atoms: ['F-4.2', 'F-4.4', 'F-4.5', 'F-4.9', 'F-5.1', 'F-3.2', 'F-1.6'],
    prefixes: ['S7.'], // P7 attacker — fence escape, exfil, forge, cross-tenant, replay, net_policy, single-use
    note: 'an adversary with a real user\'s reach; auth-fuzz every gate, secret non-leak, replay/timing, supply-chain.' },

  { id: 'TS5-agent-untrusted', suite: 'TS-5', primary: 'adversarial', directions: ['adversarial', 'failure'],
    grade: 'E1', disposition: 'fabricable',
    atoms: ['F-4.2', 'F-4.9', 'F-2.5'],
    prefixes: ['S4.'], // P4 agent — untrusted agent code, isolation, attested metrics
    note: 'full agent-loop e2e has an X4 tail (hugit dials it); isolation + attested metrics fabricable.' },

  // ─────────────────────────── TS-1 · correctness (E0/E1) ───────────────────────────────
  { id: 'TS1-contracts', suite: 'TS-1', primary: 'adversarial', directions: D_ALL, grade: 'E0',
    disposition: 'fabricable',
    atoms: ['F-3.1', 'F-3.2', 'F-3.3'],
    prefixes: ['S16.'], // P16 contract — drift tripwire, transcription law
    note: '17 conformance vectors, both sides, byte-identical.' },

  { id: 'TS1-exec-core', suite: 'TS-1', primary: 'happy', directions: D_ALL, grade: 'E1',
    disposition: 'fabricable',
    atoms: ['F-4.7', 'F-4.8', 'F-6.2', 'F-6.3', 'F-9.2', 'F-9.3', 'F-9.4', 'F-10.4', 'F-10.5'],
    prefixes: [], // execution-core + engines + client-tool logic, authored tests
    note: 'cargo/vitest unit + route coverage; the 705+281 existing suite maps here + gap-fill.' },

  { id: 'TS1-control-plane-logic', suite: 'TS-1', primary: 'edge', directions: D_ALL, grade: 'E1',
    disposition: 'fabricable',
    atoms: ['F-5.6', 'F-5.11', 'F-5.7', 'F-1.5', 'F-6.5'],
    prefixes: ['S12.'], // P12 finance — cost/vCPU signal, metering logic
    note: 'billing/metering + CoreLink-seam logic; F-5.7 shard contract is E0-fabricable (FNV-1a TS==Rust), N>1 live is owner-gated (RAISE-N); live push exporter armed-OFF (owner).' },

  { id: 'TS1-hugit-contract', suite: 'TS-1', primary: 'adversarial', directions: ['adversarial'],
    grade: 'E1', disposition: 'x4',
    atoms: ['F-2.2'],
    prefixes: ['S2.'], // P2 hugit — DISCONTINUED (campaign #3); contract cells only, hugit-driven = X4
    note: 'hugit is discontinued; these are X4 (no live driver) — kept for contract completeness.' },

  { id: 'TS1-compliance-comms', suite: 'TS-1', primary: 'edge', directions: ['edge'], grade: 'E1',
    disposition: 'owner-gated',
    atoms: ['F-4.10', 'F-5.4'],
    prefixes: ['S10.', 'S17.'], // P10 compliance · P17 comms — evidence exists; policy/surface = owner
    note: 'attestation/audit evidence fabricable; DPA/GDPR/SSO/statuspage are owner (legal/surface).' },

  { id: 'TS1-buyer-positioning', suite: 'TS-1', primary: 'happy', directions: ['happy'], grade: 'E1',
    disposition: 'owner-gated',
    atoms: ['F-1.1', 'F-1.3'],
    prefixes: ['S6.'], // P6 buyer — mechanism-proven; pricing/positioning = owner (GA)
    note: 'the pricing mechanism is fabricable; the go-to-market positioning is an owner decision.' },

  // ─────────────────────────── TS-6 · external / multi-tenant (X4) ──────────────────────
  { id: 'TS6-external-customer', suite: 'TS-6', primary: 'happy', directions: D_ALL, grade: 'X4',
    disposition: 'x4',
    atoms: ['F-8.1'],
    prefixes: [], // the external-org install click + real customer path
    note: 'non-HumanGuardrail install is an OAuth UI click (not headless); repo I can create, click is owner.' },

  // ─────────────────────────── campaign #2 · Workspaces (planned) ───────────────────────
  { id: 'C2-workspaces', suite: 'TS-1', primary: 'happy', directions: ['happy'], grade: 'E0',
    disposition: 'campaign2',
    atoms: ['F-2.4', 'F-4.8', 'F-5.10'],
    prefixes: ['S3.'], // P3 workspaces — spine E0; SKUs are campaign #2
    note: 'lifecycle spine has E0 coverage; the SKUs themselves are campaign #2 (out of scope here).' },
];

export const SUITES = ['TS-1', 'TS-2', 'TS-3', 'TS-4', 'TS-5', 'TS-6'];
export const DIRECTIONS = D_ALL;
export const DISPOSITIONS = ['fabricable', 'owner-gated', 'x4', 'campaign2'];
