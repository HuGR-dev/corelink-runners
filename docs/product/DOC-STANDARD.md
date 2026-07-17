# Documentation craft standard — SOTA, impeccable, consistent

**Date:** 2026-07-17 · **Owner mandate:** "essa documentação deve ser feita SOTA, com estrutura,
capricho, impecável." · **Applies to:** `docs/product/FEATURES.md`, `docs/product/USE-SCENARIOS.md`,
and every future product/validation doc. Content completeness (the 4–5x critic loop) is necessary but
NOT sufficient — the docs must also be **publication-grade**: navigable, consistent, precise, beautiful.

## The 10 non-negotiables (every doc)
1. **Front-matter block** — title (one line), one-sentence PURPOSE, `Last updated: <date>`, `Status`,
   and a **Legend** (every badge/marker used, defined once). No undefined marker anywhere.
2. **Table of Contents** — linked, at the top, mirroring the exact heading hierarchy. Regenerated when
   headings change.
3. **A top-of-doc SUMMARY MATRIX** — the whole doc at a glance in one table (FEATURES: feature→status→
   where; USE-SCENARIOS: persona→JTBD→#stories→coverage). The reader sees the shape before the detail.
4. **Consistent, ID'd CARDS** — every feature / story is a card with a **stable ID** (`F-1.2`, `S1.3.1`)
   and an IDENTICAL field template (below). No ad-hoc sections. IDs are permanent anchors.
5. **Deterministic heading hierarchy** — H1 doc · H1/H2 persona-or-domain · H2/H3 theme · H3/H4 card.
   Never skip a level; never two H1s meaning different things.
6. **Status badges are a fixed vocabulary** — 🟢 LIVE-proven · 🟡 built-not-proven · 🔵 owner-gated ·
   ⚪ X4-external · ⚫ INERT/planned — used identically everywhere, defined in the Legend.
7. **Evidence is always cited** — `file:line`, a test name, a run ID, or an HTTP result. No unbacked
   claim. "Proven" vs "built" vs "gated" is never blurred (the tense-discipline rule).
8. **Cross-references resolve** — a feature card lists the story IDs that exercise it; a story card
   lists the feature IDs it uses. A broken/absent cross-ref is a defect. A cross-ref MATRIX appendix.
9. **Prose is lean + concrete** — house style (mirrors hugit + existing docs): no filler, no marketing
   fluff, active voice, one idea per sentence, tables for anything tabular, code-fenced for anything
   literal. Terminology is consistent (a Glossary defines every coined term, used verbatim thereafter).
10. **Impeccable mechanics** — no typos, no broken links, no orphan headings, no duplicated content,
    no drift vs code, consistent spacing/punctuation/casing. Renders cleanly in GitHub markdown.

## The card templates (identical every time)

**Feature card (FEATURES.md):**
```
### F-<id> — <Feature name>  <status-badge>
**What:** one line. **Where:** `path:line`. **Status:** <badge> — <one-line evidence>.
**Details:** states · config knobs (with defaults) · failure/fallback behavior.
**Exercised by:** S<id>, S<id>. **Validated by:** <test/probe/run-id | GAP: needs E4/E5/X4>.
```

**Story card (USE-SCENARIOS.md):**
```
### S<id> — <Short title>  <reality-badge>
**As a** <persona>, **I want** <goal>, **so that** <value>.
**Flow:** 1… 2… 3… (concrete steps). **Expected:** <behavior>.
**Acceptance / evidence:** <what a test/probe checks | citation | GAP>.
**Variations & failures:** <bullet list of edges/combinations/failure-modes>.
**Feature(s):** F-<id>, F-<id>.  **Reality:** <badge> — <one-line why>.
```

## Required appendices (every product doc)
- **Legend** (badges + terms). **Glossary** (coined terms). **Cross-reference matrix** (feature↔story).
- **Change log** (round-by-round: R1 built, R2 deepened, R3…, and the architecture/polish passes).
- **Coverage summary** (what % of features/stories is at each evidence grade; the honest residual).

## Process (craft interleaved with the completeness loop)
- The critic-deepen rounds add CONTENT.
- After a content round, a **documentation-architect pass** imposes THIS standard (TOC, cards, legend,
  cross-ref, glossary, mechanics) without losing content.
- The doc is "done" only when BOTH a completeness-critic AND a craft-critic come back DRY.
- Every round's DoD includes: "conforms to DOC-STANDARD.md" — a structure regression is a fail.
