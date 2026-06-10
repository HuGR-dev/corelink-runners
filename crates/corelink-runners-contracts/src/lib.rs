//! Wire-contract types for the hugit ⇄ CoreLink Runners seam.
//!
//! Wire-contract types arrive in R1b after the R0 transcription freeze;
//! placeholder so gates run from day 1.

#[cfg(test)]
mod tests {
    /// Trivial day-1 test so `cargo test --workspace --locked` exercises
    /// something before the R1b contract types land.
    #[test]
    fn workspace_gate_exercises_a_test() {
        assert_eq!(env!("CARGO_PKG_NAME"), "corelink-runners-contracts");
    }
}
