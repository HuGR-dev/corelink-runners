//! Checked decimal parsing for untrusted JSON entitlement numbers.
//!
//! The plan response carries `max_vcpu_h` as a JSON number. Keep the conversion
//! integer-only: a floating-point round trip could turn a tiny positive ceiling
//! into the unmetered `0` sentinel or wrap a large value into a small one.

use serde_json::Value;

use crate::app::PlanSourceError;
use corelink_fabric::compute_meter;

/// Parse `max_vcpu_h` into vCPU milliseconds.
///
/// Absent and explicit zero are deliberate unmetered values. Every other
/// present value must be a finite, non-negative number whose checked conversion
/// is nonzero and fits the signed ledger column. Fractions are truncated only
/// at the millisecond boundary, after exact integer arithmetic.
pub(super) fn max_vcpu_h_ceiling_ms(v: &Value) -> Result<u64, PlanSourceError> {
    let Some(field) = v.get("max_vcpu_h") else {
        return Ok(0);
    };
    let Some(number) = field.as_number() else {
        return Err(PlanSourceError::Unreachable);
    };
    let (mantissa, fractional_digits, exponent) = parse_number(&number.to_string())?;
    if mantissa == 0 {
        return Ok(0);
    }

    // `mantissa × 3_600_000 × 10^-scale`, calculated in u128 so malformed
    // JSON cannot wrap into a small, apparently valid ceiling.
    let scale = i32::try_from(fractional_digits)
        .ok()
        .and_then(|fractional| fractional.checked_sub(exponent))
        .ok_or(PlanSourceError::Unreachable)?;
    let numerator = mantissa
        .checked_mul(u128::from(compute_meter::MS_PER_VCPU_HOUR))
        .ok_or(PlanSourceError::Unreachable)?;
    let milliseconds = if scale <= 0 {
        let multiplier = checked_pow10(
            u32::try_from(scale.checked_neg().ok_or(PlanSourceError::Unreachable)?)
                .map_err(|_| PlanSourceError::Unreachable)?,
        )
        .ok_or(PlanSourceError::Unreachable)?;
        numerator
            .checked_mul(multiplier)
            .ok_or(PlanSourceError::Unreachable)?
    } else {
        let divisor =
            checked_pow10(u32::try_from(scale).map_err(|_| PlanSourceError::Unreachable)?)
                .ok_or(PlanSourceError::Unreachable)?;
        numerator / divisor
    };
    let milliseconds = u64::try_from(milliseconds).map_err(|_| PlanSourceError::Unreachable)?;
    if milliseconds == 0 || !compute_meter::fits_ledger(milliseconds) {
        return Err(PlanSourceError::Unreachable);
    }
    Ok(milliseconds)
}

/// Parse serde_json's canonical JSON-number spelling without floating point.
fn parse_number(raw: &str) -> Result<(u128, usize, i32), PlanSourceError> {
    let bytes = raw.as_bytes();
    let mut pos = 0;
    if bytes.is_empty() || bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        return Err(PlanSourceError::Unreachable);
    }
    let mut mantissa = 0u128;
    let mut integer_digits = 0usize;
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        mantissa = mantissa
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(bytes[pos] - b'0')))
            .ok_or(PlanSourceError::Unreachable)?;
        integer_digits = integer_digits
            .checked_add(1)
            .ok_or(PlanSourceError::Unreachable)?;
        pos += 1;
    }
    if integer_digits == 0 {
        return Err(PlanSourceError::Unreachable);
    }

    let mut fractional_digits = 0usize;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            mantissa = mantissa
                .checked_mul(10)
                .and_then(|value| value.checked_add(u128::from(bytes[pos] - b'0')))
                .ok_or(PlanSourceError::Unreachable)?;
            pos += 1;
        }
        fractional_digits = pos - start;
        if fractional_digits == 0 {
            return Err(PlanSourceError::Unreachable);
        }
    }

    let mut exponent = 0i32;
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        pos += 1;
        let negative = match bytes.get(pos) {
            Some(b'-') => {
                pos += 1;
                true
            }
            Some(b'+') => {
                pos += 1;
                false
            }
            _ => false,
        };
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            exponent = exponent
                .checked_mul(10)
                .and_then(|value| value.checked_add(i32::from(bytes[pos] - b'0')))
                .ok_or(PlanSourceError::Unreachable)?;
            pos += 1;
        }
        if pos == start {
            return Err(PlanSourceError::Unreachable);
        }
        if negative {
            exponent = exponent.checked_neg().ok_or(PlanSourceError::Unreachable)?;
        }
    }
    if pos != bytes.len() {
        return Err(PlanSourceError::Unreachable);
    }
    Ok((mantissa, fractional_digits, exponent))
}

fn checked_pow10(power: u32) -> Option<u128> {
    if power > 38 {
        return None;
    }
    let mut value = 1u128;
    for _ in 0..power {
        value = value.checked_mul(10)?;
    }
    Some(value)
}
