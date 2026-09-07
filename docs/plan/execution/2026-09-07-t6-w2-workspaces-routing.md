# T6-W2 workspace release-guard routing

This record separates the T6-W2 release package from the Runner bundle. It is
metadata and evidence only; it does not publish, merge, or modify either
repository.

## Repository boundary and reachability

The release package and its guards belong to the separate repository
`https://github.com/HuGR-Labs/corelink-workspaces.git` (`origin` in
`/private/tmp/corelink-t6w2-release-guards`), not to `corelink-runners`.

| Material | Exact branch | Tip / source | Reachability observed locally |
| --- | --- | --- | --- |
| v0.1.12 package and acceptance evidence | `accept/t6-w2-clw-0.1.12` | `4644606d658636f060210e4c79c7549bfeb2d71d` → `ea53a72d82b4ed547d66b9686b9f41243de3706c` | Separate from the guard branch and not reachable from `origin/main` (`214c4e4`). |
| fail-closed signing and Windows release guards | `accept/t6-w2-release-failclosed` | `c4c822cc6829c6064be31f0be285f28c661c77d3` → `7f77f4a3d1188732aab5135109a24e507abecd6e` → `89c6c97df51987162c517962557b7ae0d1518145` → `edeeec8c165a4f24573b4256597ae816c9cad5c6` → `d3a3e19291b9377e63fd70b19fb65f43d6dfe6b0` | Its full stack is reachable from the local guard-branch tip and not from `origin/main`. |

The package evidence touches `corelink-workspaces/docs/validation/2026-09-06-t6-w2-acceptance.md`; the guard stack changes that repository's release workflow and scripts. Neither path belongs in the Runner product tree, so neither stack is cherry-picked here.

## Promotion route

1. Promote the two pinned stacks through the `corelink-workspaces` review and
   merge process, retaining their exact resulting workspace-main SHA and the
   package/signing evidence.
2. Bind that immutable workspace release SHA and published signing evidence to
   the B1 candidate as external dependency evidence. Runner source remains
   unchanged by that binding.
3. Run the shared B1 gate against the composed Runner candidate and the pinned
   workspace release availability. Signing, publication, and self-hosted
   Windows availability remain mandatory at that gate.

This creates no Runner PR, push, or merge. It preserves the exactly three
Runner bundle merges: the workspace promotion is a separate repository action,
and the Runner evidence is consumed only by B1.
