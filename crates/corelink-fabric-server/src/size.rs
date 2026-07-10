//! Multi-size runner ladder — the `corelink-<size>` label → box-size resolver.
//!
//! **INERT until activation (ratified 2026-07-10, server-TL design steer).** This
//! module is the runner-side foundation for the size ladder: a customer's
//! `runs-on: corelink-standard-4` should provision a 4-vCPU box, with the size
//! carried acquire → box spec → billing. Per the joint design, the size is
//! **derived by parsing the existing managed label** (`RunnerSpec.labels`) — NOT
//! a new `AcquireRequest` field — so `conformance/AcquireRequest.json` stays
//! byte-identical and the frozen `deny_unknown_fields` request is untouched.
//!
//! ## Why this is a pure, unwired module right now
//! Activation is a **joint cross-repo lockstep** (server-TL owns all three):
//!   1. spawn `instance_type` on the wire → regenerates `conformance/cloudflare-spawn.json`;
//!   2. an `instance_type` dimension on `UsageEventData` → the billing aggregator
//!      grows a per-`(kind, instance_type)` price map FIRST, then the fabric emits;
//!   3. per-tenant `allowed_sizes` from introspect → the acquire gate (default =
//!      entry size only; bigger rungs unlock per tier — a margin-safe default).
//! None of that moves unilaterally. So this module ships the **resolver + registry
//! only** — pure logic, fully unit-tested, called nowhere on the live path yet.
//! With a single-rung registry (today) it resolves every acquire to the default
//! size, i.e. byte-identical to the current single-size fabric. When the owner
//! sets the **size taxonomy** (which rungs + $/slot-second — a product decision),
//! extra rungs are added here and the three activation seams are wired behind a
//! flag in lockstep with corelink-server.

/// The managed-label prefix. A size label is `"<PREFIX><size-name>"`, e.g.
/// `"corelink-standard-4"`. The bare prefix-less-suffix label `"corelink"`
/// (today's single managed label) carries no size and resolves to the default.
pub const SIZE_LABEL_PREFIX: &str = "corelink-";

/// The bare managed label — the current single-size default carrier.
pub const DEFAULT_MANAGED_LABEL: &str = "corelink";

/// One rung of the ladder: a named box size with its compute + CF instance shape.
///
/// `vcpu` feeds the compute-reservation gate (the monthly vCPU-h ceiling);
/// `instance_type` is the Cloudflare Durable-Object container class the spawn
/// path selects (per the server-TL steer: one DO class per size initially).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeSpec {
    /// The size name as it appears AFTER the prefix in the label
    /// (`corelink-standard-4` → `"standard-4"`). Also the billing
    /// `instance_type` dimension value once activation lands.
    pub name: String,
    /// Worst-case vCPU count of the box — the compute reservation is `vcpu × ttl`.
    pub vcpu: u32,
    /// The Cloudflare container `instance_type` / DO class for this rung
    /// (e.g. `"standard-4"`). Rides the spawn payload once activated.
    pub instance_type: String,
}

impl SizeSpec {
    /// Construct a rung. `name` is the label suffix; `instance_type` the CF class.
    pub fn new(name: impl Into<String>, vcpu: u32, instance_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vcpu,
            instance_type: instance_type.into(),
        }
    }
}

/// The size ladder: a default rung plus zero or more named rungs.
///
/// Resolution is by the label suffix; anything not matching a named rung — the
/// bare `corelink` label, an unknown size, or no managed label at all — falls
/// back to `default`. So a **single-rung** registry (only a default) resolves
/// every acquire to that one size: byte-identical to today's single-size fabric.
#[derive(Debug, Clone)]
pub struct SizeRegistry {
    default: SizeSpec,
    rungs: Vec<SizeSpec>,
}

impl SizeRegistry {
    /// A single-rung ladder — the INERT default (today's behaviour). Every
    /// resolve returns `default`.
    pub fn single(default: SizeSpec) -> Self {
        Self {
            default,
            rungs: Vec::new(),
        }
    }

    /// A ladder with a default rung and additional named rungs. A `rung` whose
    /// `name` duplicates the default or an earlier rung is ignored (first wins),
    /// so the ladder is deterministic and the default is never shadowed.
    pub fn with_rungs(default: SizeSpec, rungs: impl IntoIterator<Item = SizeSpec>) -> Self {
        let mut kept: Vec<SizeSpec> = Vec::new();
        for rung in rungs {
            let dup = rung.name == default.name || kept.iter().any(|r| r.name == rung.name);
            if !dup {
                kept.push(rung);
            }
        }
        Self {
            default,
            rungs: kept,
        }
    }

    /// The default (entry) rung — the fallback + the margin-safe `allowed_sizes`
    /// default for existing tenants.
    pub fn default_rung(&self) -> &SizeSpec {
        &self.default
    }

    /// Total rung count including the default (1 = the inert single-size ladder).
    /// Named `rung_count` (not `len`) — a ladder is never empty; it always has
    /// at least the default rung.
    pub fn rung_count(&self) -> usize {
        1 + self.rungs.len()
    }

    /// Whether the ladder is a single (inert) rung.
    pub fn is_single(&self) -> bool {
        self.rungs.is_empty()
    }

    /// Resolve a rung by size name (the label suffix). Matches the default too.
    pub fn by_name(&self, name: &str) -> Option<&SizeSpec> {
        if name == self.default.name {
            return Some(&self.default);
        }
        self.rungs.iter().find(|r| r.name == name)
    }

    /// Resolve the box size for an acquire from its managed labels.
    ///
    /// Scans for the FIRST label of the form `corelink-<size>` whose `<size>`
    /// names a rung, and returns that rung. If no label names a known rung — the
    /// bare `corelink` label, an unknown/unpriced size, or no managed label —
    /// returns the default rung (fail-safe: an unrecognised size never provisions
    /// a bigger-than-paid box; it lands on the entry rung).
    pub fn resolve_from_labels(&self, labels: &[String]) -> &SizeSpec {
        for label in labels {
            if let Some(size) = label.strip_prefix(SIZE_LABEL_PREFIX)
                && let Some(rung) = self.rungs.iter().find(|r| r.name == size)
            {
                return rung;
            }
        }
        &self.default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn ladder() -> SizeRegistry {
        SizeRegistry::with_rungs(
            SizeSpec::new("standard-2", 2, "standard-2"),
            [
                SizeSpec::new("standard-4", 4, "standard-4"),
                SizeSpec::new("standard-8", 8, "standard-8"),
            ],
        )
    }

    #[test]
    fn single_rung_is_inert_resolves_default_for_everything() {
        let reg = SizeRegistry::single(SizeSpec::new("standard-2", 2, "standard-2"));
        assert!(reg.is_single());
        assert_eq!(reg.rung_count(), 1);
        // Bare label, a "bigger" label, garbage, empty — all → the one default.
        for ls in [
            vec![],
            labels(&["corelink"]),
            labels(&["corelink-standard-8"]),
            labels(&["corelink-nonsense"]),
            labels(&["totally-other"]),
        ] {
            assert_eq!(reg.resolve_from_labels(&ls).vcpu, 2);
        }
    }

    #[test]
    fn resolves_named_rung_from_label_suffix() {
        let reg = ladder();
        assert_eq!(reg.rung_count(), 3);
        assert_eq!(
            reg.resolve_from_labels(&labels(&["corelink-standard-4"]))
                .vcpu,
            4
        );
        assert_eq!(
            reg.resolve_from_labels(&labels(&["corelink-standard-8"]))
                .instance_type,
            "standard-8"
        );
    }

    #[test]
    fn bare_and_unknown_and_empty_fall_back_to_default() {
        let reg = ladder();
        assert_eq!(
            reg.resolve_from_labels(&labels(&["corelink"])).name,
            "standard-2"
        );
        assert_eq!(
            reg.resolve_from_labels(&labels(&["corelink-standard-16"]))
                .name,
            "standard-2"
        );
        assert_eq!(reg.resolve_from_labels(&[]).name, "standard-2");
        // An unrecognised size never escalates past the entry rung (margin-safe).
        assert_eq!(reg.resolve_from_labels(&labels(&["corelink-huge"])).vcpu, 2);
    }

    #[test]
    fn first_matching_managed_label_wins() {
        let reg = ladder();
        // A job may carry several labels; the first that names a known rung wins.
        let ls = labels(&["self-hosted", "corelink-standard-4", "corelink-standard-8"]);
        assert_eq!(reg.resolve_from_labels(&ls).vcpu, 4);
    }

    #[test]
    fn duplicate_rung_names_are_ignored_default_never_shadowed() {
        let reg = SizeRegistry::with_rungs(
            SizeSpec::new("standard-2", 2, "standard-2"),
            [
                SizeSpec::new("standard-2", 99, "bogus"), // dup of default → ignored
                SizeSpec::new("standard-4", 4, "standard-4"),
                SizeSpec::new("standard-4", 44, "bogus"), // dup rung → first wins
            ],
        );
        assert_eq!(reg.rung_count(), 2);
        assert_eq!(reg.default_rung().vcpu, 2);
        assert_eq!(reg.by_name("standard-4").unwrap().vcpu, 4);
    }

    #[test]
    fn by_name_matches_default_and_rungs() {
        let reg = ladder();
        assert_eq!(reg.by_name("standard-2").unwrap().vcpu, 2);
        assert_eq!(reg.by_name("standard-8").unwrap().vcpu, 8);
        assert!(reg.by_name("standard-32").is_none());
    }
}
