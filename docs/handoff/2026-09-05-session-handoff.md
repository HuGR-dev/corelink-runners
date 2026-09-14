# Session handoff — 2026-09-05

Read [the factual state snapshot](2026-09-05-session-state.md) first. This is a
handoff for a new session, not authorization to resume any stale command queue.
The owner is ending this session because organization did not produce complete
sprint delivery. Do not respond with more test counts as a substitute for that.

## Explicit acknowledgment from the outgoing orchestrator

I completely lost control of the situation and was not able to orchestrate
this work. The owner explicitly requires that this failure be recorded, not
softened into a routine handoff or explained away as technical difficulty.
Responsibility for orchestration was mine. The underlying cause is unknown
and has not been established. Observable failures are documented in the state
snapshot; they are evidence of what went wrong, not a proven root cause.
The incoming session should independently verify the repository, sprint scope
and delivery evidence before relying on my earlier status reports.

## Non-negotiable owner instructions

1. **Full CI only on COMPLETE SPRINTS**, on their composed stacked-PR tips.
   Never per WP, partial cohort or arbitrary bundle. Finish the entire agreed
   implementation scope first; use necessary focused checks while developing.
   Sprint readiness for CI does not mean CI or later live acceptance must
   already have passed. Avoid that circular prerequisite.
2. Deliver the planned sprints through stacked PRs, with clear professional
   descriptions, dependencies and evidence. Merge is a sprint delivery step,
   not a way to trickle unvalidated partial work into main. Do not silently
   change sprint boundaries or count a subset as a finished sprint.
3. Prefer CoreLink/self-hosted runners, with local execution as the billing
   workaround. Billing problems do not authorize zero-job merges, skipped
   required checks or weaker acceptance. This fallback also does not authorize
   premature full CI on incomplete sprints.
4. Orchestrate maximum useful parallel work: Luna first, Terra for harder
   bounded tasks. Freeze concrete interfaces, allowed files and acceptance
   before delegation. Separate author worktrees; one integrator owns an index.
   Do not fill slots with repeated generic reviews or duplicate full suites.
5. **600 LOC is per code file**, not per WP, PR, sprint, diff or document.
   Around400 LOC is preferred for new code files. Do not run code-file size
   guards against planning documents or cosmetically game the limit.
6. Autonomous work and deployment were authorized, but not invented external
   authority, unsafe data changes, unrelated-repo mutations or false evidence.
   Bring actual missing owner/provider choices as concrete requests, not vague
   blockers repeated indefinitely. Keep process thin and focus on delivery.

## What went wrong in the outgoing execution

- Four groups of12/14/15/13 WPs were written down, but no complete sprint was
  delivered. The grouping was not made into an effective delivery sequence.
- Full CI was repeatedly run on partial Sprint1 snapshots. Freezing a source
  snapshot and calling it a bundle did not satisfy the owner's instruction.
- Work continued on prepared descendants while critical prerequisites stayed
  open. Reviewed code, many passing tests and11 open PR layers were presented
  as progress without closure of a sprint. The owner explicitly rejected that.
- Some tests and reviews found real bugs, but fixing them does not excuse
  confusing partial-source validation with a complete sprint's acceptance.
- A runtime proof log was lost because the proof VM was stopped before export.
  A same-source focused rerun later produced the independently checked host log.
  Preserve evidence outside the guest before shutting down disposable systems.

**Do not repeat the pattern by immediately rerunning #558's matrix, creating
another partial-CI pipeline, or restarting a large governance/review cycle.**
The current local-memory policy explicitly supersedes older instructions that
allowed full CI merely because an incomplete stack tip was frozen.

## Safe starting point

The default workspace is stale and conflicted. Do not checkout/reset/abort it.
The state snapshot lists its exact conflicts and the clean isolated worktrees.
The two new session documents are intentionally left untracked there, without
staging or disturbing that conflicted index.

For current instructions and source, inspect:

```text
/private/tmp/corelink-sprint1-cleanup-next
  HEAD bb165bad54125460ebae213e405df3a75baa6566
  remote wp/sprint1-cleanup-preparation / PR#558

/private/tmp/corelink-sprint4-action-bundle.O2xzb3
  HEAD 8505fda9245ac02665ddac264c7e5f3414022de2
  remote wp/sprint4-action-preparation / no new PR
```

Recheck HEAD, tree and status before using either; do not rewrite the frozen
historical sources. If a temporary worktree is gone, inspect the pushed branch
and create a fresh isolated worktree. Do not mistake the conflicted checkout's
old `CLAUDE.md` for a current production report.

Read current `CLAUDE.md`, applicable `AGENTS.md`, the three dated planning/triage
files named in the state snapshot, and relevant skills before acting. Operations
instructions live at `.claude/skills/corelink-runner-ops/SKILL.md`; moat-related
skills are also under `.claude/skills/`. Local memory is
`/Users/gustavoschneiter/.codex/plans/corelink-deepseek-wp-acceleration.md`.
Its chronological entries contain superseded states; the latest owner CI
correction and verified end-state take precedence. Do not delegate reading or
interpreting applicable skill instructions to another agent.

## Suggested resumption sequence — not executed by this handoff

1. Establish the current clean source and read the owner's latest instructions.
   Keep the baseline clear:16/70 source WPs recorded delivered,54 unfinished,
   and zero complete sprints delivered in this four-sprint effort.
2. Turn the first sprint's existing WP list into a concise implementation versus
   missing-evidence/dependency view. Verify what actually blocks delivery. Do not
   replace this with another large plan rewrite or assume a green partial tip
   means all12 WPs are implemented.
3. Address the actual critical prerequisites, particularly T3-W18 and R1,
   distinguishing executable in-repo work from missing external capability or
   owner decisions. Preserve safety dependencies; do not impose an unrelated
   global freeze. If the sprint arrangement cannot be delivered, explain the
   concrete defect rather than silently changing its scope.
4. Fan out independently implementable work with bounded contracts and file
   ownership. Preserve completed preparation rather than rebuilding it. Keep
   full CI off while the sprint's implementation scope is incomplete.
5. Once the entire sprint is composed, freeze that complete sprint tip and run
   its full required CI in the approved infrastructure. Preserve exact commands,
   exits, source hashes and raw evidence outside ephemeral storage. Failed
   checks require fixes, not waived gates or invented passing jobs.
6. Complete the applicable acceptance/release steps and deliver the stacked
   sprint when its real requirements are met. Update WP/finding counts based
   on actual delivered scope, not commits, PR count or number of tests.

## Technical cautions that must survive the handoff

- Pending cleanup: no cap release based on an unknown cloud handle or generic
  post-provision500. Retain identity through failed ledger finish; File claims
  must fsync before publication; Pg claims must share real mutation fences.
- T6-W2 refusal78 proves refusal/no child execution, not a successful memoized HIT.
- Actual local act/Node24 proof of the composite is distinct from its test shim,
  a hosted GitHub run, customer onboarding and full Sprint4 CI.
- Production is contained/degraded, not newly declared durable or go-live.
  T3-W18's genuine live matrix and nonempty ordered resume remain incomplete.
  Preserve CONTAINMENT/v7 and plaintext bindings; do not roll back across the
  Durable Object migration or casually re-arm paused production behavior.
- Existing host PostgreSQL5432, other VMs and sibling HuGR repositories are
  outside the disposable proof scope. Never kill a saved PID blindly.
- Verify full SHAs using Git. Prior agent reports occasionally truncated pins
  or SHAs; do not copy unchecked suffixes into commands or evidence.
- Preserve authored/published history and valid DCO sign-offs. Do not publish
  obsolete local branches with malformed-DCO ancestry.

## End-of-session boundary

No further code, tests, CI, merges, deployments, PR edits or cleanup were run to
produce this handoff. Only read-only verification and these two requested
documents were performed. The dedicated proof VM was verified Stopped; the
conflicted files and unrelated untracked content were preserved. The outgoing
session does not claim a completed sprint or newly closed WP.
