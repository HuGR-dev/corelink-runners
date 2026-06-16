//! Pure compute-metering arithmetic for the vCPU-h hard ceiling
//! (`pricing.md §3` enforcement wall — the loss-impossible guarantee mechanism).
//!
//! This module is **pure** (no I/O, no deps): integer vCPU·ms accounting, the
//! calendar-month period key, and the signed-`bigint` boundary guards. The
//! ledger (`pg_ledger.rs`) folds these into its atomic admit; this file owns
//! only the arithmetic so it is trivially testable and has no race surface.
//!
//! ## Why every value is `u64` vCPU·ms — and why that is not enough
//!
//! The unit is **vCPU·milliseconds** (`u64`, pure integer, no float): a 4-vCPU
//! box held 1 ms accrues `4`. A monthly ceiling of `H` vCPU-hours is
//! `H × 3_600_000` vCPU·ms. BUT every value ultimately lands in a Postgres
//! **signed `bigint`** via the codebase's universal `as i64` cast — lossless
//! only below `i64::MAX`. A `u64` that saturated past `i64::MAX` casts to a
//! **negative** `bigint`, which would *defeat* the ceiling (a negative
//! reservation subtracts honest tenants' headroom). So saturating `u64` math is
//! necessary but **not sufficient**: every value that will be stored or summed
//! must also be proven `≤ i64::MAX`. [`MAX_LEDGER_VCPU_MS`] + [`fits_ledger`] are
//! that guard; [`ceiling_vcpu_ms`] enforces it at plan-load (P1-G), and the
//! admit path rejects an over-`i64` reservation fail-closed (P0-C/P0-D).
//!
//! ## Period attribution
//!
//! Consumption is attributed to `period_key(created_at)` — the calendar month
//! (UTC) the lease was admitted in. A boundary-spanning lease is charged in full
//! to its admit-period (the reservation already covered its full run-to-deadline
//! worst case), which is sound because the autoscaler ttl is bounded well under
//! a month, so a lease spans at most one boundary. See the wave plan §8/§9 P0-3.

/// Milliseconds in one vCPU-hour. `ceiling_vcpu_ms = max_vcpu_h × this`.
pub const MS_PER_VCPU_HOUR: u64 = 3_600_000;

/// The largest value safely round-trippable through the signed `bigint` ledger
/// columns (`reserved_vcpu_ms`, `accrued_vcpu_ms`, the ceiling). A value above
/// this casts to a **negative** `bigint` and must be rejected at the boundary —
/// saturating to `u64::MAX` does NOT save you; the `as i64` is the hazard.
pub const MAX_LEDGER_VCPU_MS: u64 = i64::MAX as u64;

/// Does `v` fit the signed `bigint` ledger columns losslessly?
///
/// Used at every `u64 → bigint` boundary (admit reservation, accrual upsert):
/// `false` ⇒ reject fail-closed, never store a wrapping value.
#[inline]
pub fn fits_ledger(v: u64) -> bool {
    v <= MAX_LEDGER_VCPU_MS
}

/// vCPU·ms for a box of `vcpu` cores held `dur_ms` milliseconds.
///
/// Saturating: a crafted `dur_ms` can never panic or wrap to a small value —
/// it pins at `u64::MAX`, which [`fits_ledger`] then rejects at the boundary.
#[inline]
pub fn vcpu_ms(vcpu: u32, dur_ms: u64) -> u64 {
    (vcpu as u64).saturating_mul(dur_ms)
}

/// A per-month vCPU-hour ceiling as vCPU·ms, validated against the `bigint`
/// bound. `0 ⇒ Ok(0)` (the spec'd "disabled" sentinel — NEVER `u64::MAX`).
///
/// Returns `Err` if `max_vcpu_h × 3_600_000` would exceed `i64::MAX` (P1-G): an
/// "unlimited" tier must be expressed as `0` (disabled), not a huge number that
/// wraps the column to `-1` and reject-alls. Called at plan-load so a
/// mis-configured tier fails loudly, not silently.
pub fn ceiling_vcpu_ms(max_vcpu_h: u64) -> anyhow::Result<u64> {
    let ceiling = max_vcpu_h.saturating_mul(MS_PER_VCPU_HOUR);
    if !fits_ledger(ceiling) {
        anyhow::bail!(
            "vCPU-h ceiling {max_vcpu_h} (= {ceiling} vCPU·ms) overflows the i64 ledger \
             column; an unlimited tier must be configured as 0 (disabled), not a large value"
        );
    }
    Ok(ceiling)
}

/// The calendar-month period key `YYYYMM` (UTC) for an epoch-ms instant.
///
/// E.g. `2024-02-…` → `202402`. Consumption is attributed to the period of the
/// lease's `created_at`. Uses Howard Hinnant's `civil_from_days` (public-domain,
/// branch-free, valid for the full proleptic Gregorian range) — NO chrono/time
/// dep. The `1 ≤ month ≤ 12` invariant is asserted before packing so the
/// Dec→Jan `YYYY13`/`YYYY00` packing bug is unrepresentable, not just untested.
pub fn period_key(now_ms: u64) -> u32 {
    let days = (now_ms / 86_400_000) as i64;
    let (year, month) = civil_from_days(days);
    // Fail-loud on any civil-date regression rather than packing a bad key.
    assert!(
        (1..=12).contains(&month),
        "period_key: civil month {month} out of [1,12] for day {days}"
    );
    (year as u32) * 100 + month
}

/// Howard Hinnant's days-since-epoch → `(year, month)` (the day is unused here).
///
/// `z` is days since 1970-01-01. Returns the civil year and month `[1,12]`.
/// Verbatim from the reference algorithm (http://howardhinnant.github.io/date_algorithms.html#civil_from_days),
/// pinned by the [`tests::period_key_kat`] table.
fn civil_from_days(z: i64) -> (i64, u32) {
    let z = z + 719_468; // shift epoch to 0000-03-01
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day-of-era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day-of-year [0, 365]
    let mp = (5 * doy + 2) / 153; // month-prime [0, 11] (Mar=0)
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31] (unused)
    let _ = day;
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = year + i64::from(month <= 2); // Jan/Feb belong to the next civil year
    (year, month as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P1-H KAT — the Dec→Jan packing trap + leap boundaries. A civil-date
    /// regression that produces `YYYY13`/`YYYY00` or misplaces a leap day breaks
    /// here, not silently in production accounting.
    #[test]
    fn period_key_kat() {
        let cases: [(u64, u32, &str); 6] = [
            (0, 197001, "epoch"),
            (1_709_164_800_000, 202402, "leap Feb 29"),
            (1_709_251_200_000, 202403, "leap -> Mar rollover"),
            (1_704_067_199_999, 202312, "Dec last ms"),
            (
                1_704_067_200_000,
                202401,
                "Dec->Jan — must NOT be 202413/202400",
            ),
            (1_677_628_800_000, 202303, "non-leap Mar 1"),
        ];
        for (ms, want, what) in cases {
            assert_eq!(period_key(ms), want, "period_key KAT: {what}");
        }
    }

    #[test]
    fn vcpu_ms_saturates_never_wraps() {
        assert_eq!(vcpu_ms(4, 1_000), 4_000);
        assert_eq!(vcpu_ms(0, u64::MAX), 0);
        // A crafted huge duration pins at u64::MAX (no wrap to a small value),
        // which fits_ledger then rejects at the boundary.
        assert_eq!(vcpu_ms(2, u64::MAX), u64::MAX);
        assert!(!fits_ledger(vcpu_ms(2, u64::MAX)));
    }

    #[test]
    fn fits_ledger_boundary() {
        assert!(fits_ledger(0));
        assert!(fits_ledger(MAX_LEDGER_VCPU_MS));
        assert!(fits_ledger(i64::MAX as u64));
        assert!(!fits_ledger(i64::MAX as u64 + 1));
        assert!(!fits_ledger(u64::MAX));
    }

    #[test]
    fn ceiling_disabled_sentinel_is_zero() {
        // 0 (disabled) round-trips to 0 — NEVER u64::MAX.
        assert_eq!(ceiling_vcpu_ms(0).unwrap(), 0);
    }

    #[test]
    fn ceiling_real_tiers_fit_i64_with_margin() {
        // The ratified 40/60 ladder ceilings (pricing.md §2, 2026-06-16):
        // 100 / 240 / 600 / 1200 / 2400 vCPU-h. All must fit i64 with vast margin.
        for max_vcpu_h in [100u64, 240, 600, 1200, 2400] {
            let c = ceiling_vcpu_ms(max_vcpu_h).unwrap();
            assert_eq!(c, max_vcpu_h * MS_PER_VCPU_HOUR);
            assert!(fits_ledger(c));
            // margin: even the Max tier (2400h = 8.64e9 vCPU·ms) is ~1e9× under i64::MAX.
            assert!(c < (i64::MAX as u64) / 1_000_000);
        }
    }

    #[test]
    fn ceiling_rejects_i64_overflow() {
        // An "unlimited" tier mis-configured as a huge vCPU-h value must fail
        // loudly at plan-load, not wrap the bigint column (P1-G).
        let over = (i64::MAX as u64) / MS_PER_VCPU_HOUR + 1;
        assert!(ceiling_vcpu_ms(over).is_err());
        assert!(ceiling_vcpu_ms(u64::MAX).is_err());
    }
}
