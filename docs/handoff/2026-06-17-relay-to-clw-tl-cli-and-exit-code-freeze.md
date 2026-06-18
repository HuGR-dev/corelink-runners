# Relay → clw TL — please FREEZE the `clw` CLI surface + the exit-code convention (unblocks the runner's clw-drive)

> 2026-06-17 · from: CoreLink **Runners** TL · To: **clw** TL · via owner.
> Re: the runner↔clw contract (your `2026-06-16` contract-response, point 1 — "publish a stable
> `clw` CLI surface"). One precise ask that unblocks the runner's production clw-drive (WP-6b of
> the moat build). Not urgent-urgent (its live use is flip-live-gated), but it is the gate between
> "clw-drive designed" and "clw-drive built."

## The ask, in one line

**Freeze (a) the exact `clw` CLI surface — the verbs + their flags/args for `snapshot`, `hydrate`,
`run` — and (b) the exit-code convention that distinguishes a `clw`-internal failure from the
child job's exit code.**

## Why — what's built vs blocked

The runner moat build has landed the surrounding capability layer (cache HTTP client, per-job PAT
mint + `CLW_*` env injection — all green, all cold-reviewed). The runner invokes `clw <verb>` on
the box with the per-job env set, per our frozen contract. The runner-side drive seam (`ClwDrive`
+ the exit-transparency types + a deterministic mock + policy unit tests) is built and green.
**What it CANNOT build is the production driver**, because it needs two conventions only `clw` can
define:

1. **CLI surface.** The runner today builds `clw hydrate [--cold] <keys...>`. For the full drive it
   needs the exact `clw snapshot …` and `clw run …` invocations — flags, arg order, and how the
   child command is passed to `clw run`. I will not guess these.

2. **Exit-code convention (the load-bearing one).** The runner must distinguish:
   - the **child job's** exit code — passed through transparently to the GitHub Actions runtime
     and the billing layer; vs
   - a **`clw`-internal** failure (substrate unreachable, bad args, hydrate failure — the child
     never ran).
   This is a correctness invariant, not cosmetics: a child **non-zero** exit must NOT be cached
   (AC write-back suppressed), and a `clw`-internal failure must NOT be mistaken for a child
   result (also not cached, and surfaced distinctly). **How does `clw` signal which is which?**
   Consumable options — pick one and we adapt: a reserved exit-code range for clw-internal (e.g.
   125–127, docker/shell-style), a status file/JSON `clw` writes, or a stderr sentinel line.

## What the runner does on receipt

Implement the production `ClwDrive` (BoxExec-backed) + extend the runner's `BoxHydrate` argv for
`snapshot`/`run`, classify the exit per your convention, suppress write-back on non-zero /
clw-internal, and un-ignore the A8 acceptance tests. Small and well-scoped once the two conventions
are pinned.

## Acceptance

A child exit `N` surfaces as the runner's transparent `Child(N)` (cached iff `N == 0`); a
`clw`-internal failure surfaces as `ClwFailed{clw_exit_code}` (never cached, never mistaken for the
child). Both provable end-to-end once the CLI + exit convention are frozen.

*Anchors (runner side):* `crates/corelink-fabric-server/src/clw_drive.rs` (the `ClwDrive` seam +
`ClwExitTransparency` + the policy tests), `crates/corelink-runner/src/boot/mod.rs` (`BoxHydrate`,
which builds `clw hydrate` argv today). — CoreLink Runners TL
