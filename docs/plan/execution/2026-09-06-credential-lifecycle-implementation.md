# Credential lifecycle implementation boundary — F008

Root implementation decision under the owner's existing autonomy. This does
not waive legacy migration, CAS refusal timing or paired acceptance. Shared
compute: separate record; do not reopen that implementation wave.

## One generation authority

PostgreSQL owns a persistent counter per tenant. The existing suspension table
continues to own suspended/active state. Counter creation, reads, suspend and
resume rely on the SAME existing suspension advisory transaction lock. D1 and
Worker storage consume this counter; they never increment their own generation.

Generation0 identifies pre-migration credential history. On first touch, an
already suspended tenant initializes at0; an active tenant initializes at1.
Legacy outbox events receive generation0. Each confirmed suspended-to-active
transition increments exactly once with checked i64 arithmetic before deleting
the suspension row. Repeated active unsuspend does not increment. Suspend keeps
the current generation and captures it atomically in its exact durable event.
Repeated suspend preserves that event and generation; same-clock resume/suspend
has a new generation and event. Missing pointer repair captures its current
generation. Event generation never changes after creation.

`LeaseLedger::tenant_lifecycle(tenant)` returns tenant_id, generation(u64) and
suspended. `tenant_suspension_generation(event_id)` returns the generation
captured by that exact event. Unsupported backends refuse both methods. Existing
TenantSuspensionEvent wire stays stable internally; the reaper reads the exact
captured generation before composing the new consumer envelope. Unknown event
or corrupt/overflowing generation refuses; there is no wall-clock-derived epoch.

The issuer reads the current snapshot over an authenticated, bounded Fabric
HTTP route before preparing a credential. Generation travels over JSON as a
canonical nonnegative decimal STRING bounded by i64::MAX. A dedicated issuer
credential will be separate from spawn/exec/lifecycle/admin keys. Missing authority
configuration or a suspended snapshot refuses issuance. Root owns this route,
configuration and paired caller composition.

## Revocation projections and late issuance

Worker credential identities add `lifecycleGeneration?: string`; an absent
field in an existing stored record means legacy0, never the current generation.
Modern mint callers must supply the issuer's explicit generation. Exact PAT
identity cannot be rebound to a different generation. Existing job terminal
fences continue to close all generations of that job independently.

Add an atomic tenant revocation floor derived only from a valid suspension
event's generation. It monotonically records `revokedThrough`, with no local
increment and no TTL. Enumeration for that event selects the tenant's records
at generation <= revokedThrough; newer records remain registered. Registration
at/below an existing floor durably records a requested revocation then refuses
outside the transaction. Background retry also sees older registered records
covered by the floor, including those not reached before an enumeration crash.
Receipt replay cannot bypass the floor; malformed floors/identities refuse.

The paired server needs the same DERIVED floor in its D1 issuance transaction:
prepare/activate/adopt cannot create or hand off a credential whose generation
is closed there. The suspension route must commit the D1 floor and exact
revocation obligations before acknowledging completion to the producer. This
prevents a delayed pre-suspend issuance from becoming authenticatable after its
event's first enumeration. Post-resume credentials use PostgreSQL's newer
generation and survive an old delivery. Exact cache invalidation is a retained
obligation; no false success from a failed invalidation or unknown inventory.

Legacy0 will be a classification, NOT proof of complete historical inventory.
Coverage must join the server's known runner/DevEnv credential authority with
the Worker obligations; unrelated customer PATs cannot be swept just because
they belong to a tenant. Unknown/empty legacy history retains a migration/retry
gate until supported by actual reconciled coverage. The existing75s production CAS
refusal acceptance remains mandatory.

## Integration ownership

Root freezes types, HTTP/configuration, issuer and consumer call sites. Disjoint
Luna packets may implement PostgreSQL lifecycle storage and Worker credential
floor/selection independently on root-prepared worktrees. Server transaction
composition follows the reviewed generation contract; no source packet alone
closes F008. Only focused tests run until complete original Sprint1 composition.
