# Sprint 1/B1 Git routing delta

Recorded 2026-09-07. This delta is based only on `2026-09-06-git-closeout-inventory.json`; it does not rescan the repository globally.

The target set is the 12 Sprint 1/B1 WPs listed in the closeout plan: T4-W4, T3-W10, T6-W2, T6-W4, T6-W15, T2-W3, T8-W4b, T9-W1, T1-W5, T6-W9, T3-W18, and T6-W13.

Reachability was checked against integration head `766a40cbe1cb70f262fe7947d5c58aa640addb08` with `git merge-base --is-ancestor`. The eight WPs T4-W4, T3-W10, T6-W2, T6-W4, T8-W4b, T1-W5, T6-W9, and T3-W18 have inventory SHAs that resolve and are reachable from that head. Their refs are therefore `integrate`; the original refs remain evidence and require no duplicate integration.

T6-W15 is mixed. The inventory documents four source commits as `substituted_proven`: `aaa6815` by destination commits `92837d5`, `b025351`, and `144200e`; `7a400e1` by `ffaea8c`; `d735ff3` by `de13d45`; and `7e1bf11` by `c600bd4`. Every destination resolves and is reachable from the integration head, so these source packets are classified `substituted`. The detached sources `8873950`, `eacd95a`, `4b8b32f`, and `6e83b03` are not ancestors and remain `preserve`; the inventory separately records `cc9fa84` and `eacd95a` as approved-to-integrate material pending B1 composition. No source was deleted or re-routed.

Three target WPs are objectively unresolved at inventory scope:

| WP | Classification | Reason |
| --- | --- | --- |
| T2-W3 | `ambiguous_missing_inventory` | No tagged PR, branch, worktree, or known-agent commit in the basis inventory. |
| T9-W1 | `ambiguous_missing_inventory` | No tagged PR, branch, worktree, or known-agent commit in the basis inventory. |
| T6-W13 | `ambiguous_missing_inventory` | No tagged PR, branch, worktree, or known-agent commit in the basis inventory. |

Absence was not treated as `outside`, `integrate`, or `substituted`. Resolving these three requires an explicit inventory addition or a targeted source reference from the root coordinator. A global scan was intentionally not performed.

This document records analysis only. No branch, worktree, pull request, remote ref, or commit was deleted, reset, closed, pushed, or merged.
