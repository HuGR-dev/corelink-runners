# A2.7 final deploy attempt — rollback recovered — 2026-09-08

The clean requested tip `48052654943cffaf6827a2a7a729012d2aa2df5b` passed dry-run and a no-force pre-gate (`busy=0`, `checked=0`, `unverifiable=0`). Intake and redrive remained paused at `1/1`.

The local-equivalent deploy completed at 100% on Worker version `85171fd2-fc5b-49c2-b6f3-fc21d6fe8977`, deployment `f2f8a0be-65d7-41f6-9c69-6813e4d9b7c0`. DevEnv remained ready at UUID `a037c709-9f21-493c-8dcb-2414a588a1cd`, pinned to the `d105e11f...` digest. The authenticated fleet route returned HTTP 200 with `busy=0`, `checked=0`, `unverifiable=0`.

The final A2.5 cross-check failed: fabricd health endpoints returned HTTP 200, but provider state was `active=0`, `healthy=0`, `assigned=1`, with the listed instance `stopped`. Per the promotion rule, this was treated as material. The Worker was rolled back to `cb704a9d-f6e0-45d7-9e68-89a263a77993` at 100% via deployment `3d7050d1-f72f-4e46-ab61-3c790cd0b2ca`. Post-rollback authenticated fleet gate passed again at `0/0/0`, with `force=false`; fabricd was not changed.

A2.7 is therefore RED/rollback-recovered. No green credit is inferred from HTTP health while provider instance state is stopped.
