//! **STATUS — NOT WIRED INTO ANY LIVE PATH.** A pure decision module with no
//! caller: nothing outside this file's `#[cfg(test)]` block calls
//! [`evaluate_downgrade_admit`] or [`is_downgrade`]. Grace-to-expiry is NOT in
//! force.
//!
//! What happens instead: a downgraded tenant's next acquire still gets the
//! generic over-cap reject — the live cap path (`CapGate.check` /
//! `ledger.try_admit` in `corelink-fabric-server/src/handlers/leases.rs`)
//! answers `held >= new_cap` with the ordinary over-cap outcome, carrying none
//! of the distinct "retry as leases expire" framing this module provides.
//!
//! To take effect it would have to be wired into that same cap decision: call
//! [`evaluate_downgrade_admit`] where the acquire's over-cap outcome is
//! produced, using [`is_downgrade`] against the pre-downgrade cap recorded by
//! [`crate::plans::PlanRegistry::set_plan`] to choose the grace framing.
//!
//! Grace-to-expiry tier-downgrade admission policy (WP-DOWNGRADE-GRACE).
//!
//! A tier downgrade (e.g. Pro→Starter) via [`crate::plans::PlanRegistry::set_plan`]
//! lowers `max_concurrency` IMMEDIATELY: the next [`crate::caps::CapGate::check`]
//! reads the new, lower cap with no transition. A tenant holding 35 `Held`
//! leases who downgrades to a 20-cap is therefore instantly OVER the cap — and
//! today the gate answers every new acquire with a generic over-cap reject,
//! indistinguishable from a tenant who simply asked for too much under a stable
//! plan.
//!
//! The owner decision is **grace-to-expiry**:
//!
//! 1. **Existing `Held` leases are honored** — never evicted by the downgrade.
//!    They run to their own expiry; the tenant's occupancy drains naturally.
//! 2. **New acquires are gated at the NEW cap** — admitted only while
//!    `held < new_cap`. The downgrade takes effect for *growth* immediately,
//!    so the tenant cannot acquire fresh slots above the new ceiling.
//! 3. **Over-cap is a distinct, clear outcome** — when `held >= new_cap` (the
//!    over-cap-due-to-downgrade window) a new acquire gets
//!    [`GraceDecision::OverCapGrace`], carrying both numbers and a message that
//!    says *retry as leases expire* — NOT a generic reject. The condition is
//!    transient and self-healing: each expiry lowers `held` until it drops
//!    below `new_cap` and admission resumes.
//!
//! This module is a **pure decision module** — same shape as
//! [`crate::caps::CapGate`]: every input is an argument, no ledger/engine/box
//! symbol is reachable, so it can only ever *decide*, never start or evict
//! work. The lead wires it into the CapGate path; this file does not rewire the
//! gate itself (see the WIRING STUB in the crate docs / handoff).

/// Outcome of a downgrade-aware admission check for ONE new acquire.
///
/// Note the asymmetry that *is* grace-to-expiry: this decision governs only the
/// NEW acquire. Already-`Held` leases are never an input to an eviction — they
/// are honored to expiry by construction (this module has no power to evict),
/// so there is no "evict" variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraceDecision {
    /// `held < new_cap`: the new acquire fits under the (possibly lowered) cap.
    Admit,
    /// `held >= new_cap`: the tenant is over the new cap because of a downgrade.
    /// Existing leases run to expiry; the new acquire is declined *for now* with
    /// a clear, transient, self-healing reason — retry as leases expire.
    OverCapGrace {
        /// Current active/held lease count for the tenant.
        held: u32,
        /// The NEW (post-downgrade) concurrency cap being enforced.
        cap: u32,
    },
}

impl GraceDecision {
    /// Whether this decision admits the new acquire.
    pub fn is_admit(&self) -> bool {
        matches!(self, GraceDecision::Admit)
    }

    /// Whether this decision is an over-cap grace decline (downgrade in effect).
    pub fn is_over_cap_grace(&self) -> bool {
        matches!(self, GraceDecision::OverCapGrace { .. })
    }

    /// A clear, caller-facing reason for an [`GraceDecision::OverCapGrace`]
    /// outcome — distinct from a generic over-cap reject. `None` for an
    /// [`GraceDecision::Admit`]. The string names both numbers and tells the
    /// caller the condition is transient ("retry as leases expire").
    pub fn grace_message(&self) -> Option<String> {
        match self {
            GraceDecision::Admit => None,
            GraceDecision::OverCapGrace { held, cap } => Some(format!(
                "over cap (downgrade in effect): held {held} > cap {cap}, \
                 retry as leases expire"
            )),
        }
    }
}

/// Decide admission for one NEW acquire under grace-to-expiry, given the
/// tenant's current `held` count and the NEW (post-downgrade) `cap`.
///
/// The single rule: **admit iff `held < cap`**. This is intentionally cap-only
/// — it does not need the *old* cap to decide, because grace-to-expiry never
/// evicts: held leases drain by expiry regardless of how far the cap dropped.
/// `held >= cap` is the over-cap window and yields
/// [`GraceDecision::OverCapGrace`] carrying both numbers for a clear message.
///
/// Consequences worth noting:
/// - **No downgrade (cap unchanged or raised):** behaves exactly like a normal
///   cap check — `held < cap` admits, `held >= cap` is over-cap. A tenant that
///   was never downgraded is unaffected (the grace message still reads
///   correctly: they are simply at their cap).
/// - **`cap == 0`:** admits nothing (`held` is `u32`, so `held >= 0` always),
///   matching the fail-closed zero-cap contract in `caps.rs`.
/// - **Self-healing:** as `Held` leases expire, `held` falls; once it drops
///   below `cap`, the same function admits again — no state, no flag to clear.
pub fn evaluate_downgrade_admit(held: u32, new_cap: u32) -> GraceDecision {
    if held < new_cap {
        GraceDecision::Admit
    } else {
        GraceDecision::OverCapGrace { held, cap: new_cap }
    }
}

/// Whether a plan change from `old_cap` to `new_cap` is a downgrade that can put
/// an already-`held` tenant into the over-cap grace window.
///
/// A helper for the wiring path: only a *lowered* cap can create over-cap-due-
/// to-downgrade pressure, so a caller can use this to decide whether the
/// over-cap-grace message is the right framing (downgrade) versus an ordinary
/// at-cap reject (no downgrade). It does NOT gate admission on its own —
/// [`evaluate_downgrade_admit`] is the single admission rule.
pub fn is_downgrade(old_cap: u32, new_cap: u32) -> bool {
    new_cap < old_cap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_cap_holds_are_honored_new_acquire_declined_with_grace() {
        // Pro(40)→Starter(20): tenant holding 35 leases. Held leases are NOT an
        // input to eviction (no evict variant exists); the NEW acquire is
        // declined with a clear, transient over-cap-grace reason.
        let decision = evaluate_downgrade_admit(35, 20);
        assert_eq!(decision, GraceDecision::OverCapGrace { held: 35, cap: 20 });
        assert!(decision.is_over_cap_grace());
        assert!(!decision.is_admit());

        let msg = decision
            .grace_message()
            .expect("over-cap carries a message");
        // The message is distinct from a generic reject: it names both numbers
        // AND says the condition is transient (retry as leases expire).
        assert!(msg.contains("held 35"), "names current held count: {msg}");
        assert!(msg.contains("cap 20"), "names the new cap: {msg}");
        assert!(
            msg.contains("downgrade in effect"),
            "frames as downgrade: {msg}"
        );
        assert!(
            msg.contains("retry as leases expire"),
            "transient/self-heal: {msg}"
        );
    }

    #[test]
    fn at_exactly_new_cap_is_over_cap_grace() {
        // held == cap: the boundary is over-cap (a new acquire would exceed it).
        let decision = evaluate_downgrade_admit(20, 20);
        assert_eq!(decision, GraceDecision::OverCapGrace { held: 20, cap: 20 });
    }

    #[test]
    fn back_under_cap_admits_again_self_healing() {
        // As Held leases expire, held drains: 35 → ... → 19. The moment it drops
        // below the new cap, the SAME function admits again — no flag to clear.
        assert!(evaluate_downgrade_admit(35, 20).is_over_cap_grace());
        assert!(evaluate_downgrade_admit(20, 20).is_over_cap_grace());
        let healed = evaluate_downgrade_admit(19, 20);
        assert_eq!(healed, GraceDecision::Admit);
        assert!(healed.is_admit());
        assert!(
            healed.grace_message().is_none(),
            "admit carries no grace message"
        );
    }

    #[test]
    fn no_downgrade_unaffected_normal_admission() {
        // Stable plan, room to spare: ordinary admit. A tenant never downgraded
        // behaves exactly like a normal cap check.
        let decision = evaluate_downgrade_admit(5, 40);
        assert_eq!(decision, GraceDecision::Admit);
        assert!(decision.is_admit());
        assert!(decision.grace_message().is_none());
    }

    #[test]
    fn cap_zero_admits_nothing() {
        // Fail-closed zero-cap contract (matches caps.rs `cap_zero_admits_nothing`):
        // a cap of 0 admits nothing, even from an empty hold.
        assert!(evaluate_downgrade_admit(0, 0).is_over_cap_grace());
        assert!(evaluate_downgrade_admit(1, 0).is_over_cap_grace());
    }

    #[test]
    fn is_downgrade_detects_lowered_cap_only() {
        // Only a LOWERED cap is a downgrade (the case that can create over-cap
        // pressure on already-held leases).
        assert!(is_downgrade(40, 20), "Pro→Starter is a downgrade");
        assert!(
            !is_downgrade(20, 40),
            "Starter→Pro is an upgrade, not a downgrade"
        );
        assert!(!is_downgrade(40, 40), "unchanged cap is not a downgrade");
        assert!(is_downgrade(1, 0), "drop to zero is a downgrade");
    }

    #[test]
    fn admit_path_never_carries_a_message() {
        // Invariant: a message exists IFF the decision is over-cap-grace.
        for (held, cap) in [(0u32, 1u32), (5, 40), (39, 40), (0, 5)] {
            let d = evaluate_downgrade_admit(held, cap);
            assert!(d.is_admit());
            assert!(d.grace_message().is_none());
        }
        for (held, cap) in [(40u32, 40u32), (35, 20), (1, 0), (100, 80)] {
            let d = evaluate_downgrade_admit(held, cap);
            assert!(d.is_over_cap_grace());
            assert!(d.grace_message().is_some());
        }
    }
}
