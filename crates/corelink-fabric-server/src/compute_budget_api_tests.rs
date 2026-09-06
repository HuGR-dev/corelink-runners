//! Focused unit coverage for the standalone compute HTTP boundary.

#[cfg(test)]
mod tests {
    use super::super::compute_budget_api::{
        constant_time_eq, decimal_i64, parse_empty, valid_period,
    };

    #[test]
    fn request_shapes_and_decimal_forms_are_fail_closed() {
        assert!(parse_empty(br"{}").is_ok());
        assert!(parse_empty(br#"{"unexpected":true}"#).is_err());
        assert!(parse_empty(br"[]").is_err());
        assert_eq!(decimal_i64("0"), Some(0));
        assert_eq!(decimal_i64("19"), Some(19));
        assert_eq!(decimal_i64("01"), None);
        assert_eq!(decimal_i64("+1"), None);
        assert_eq!(decimal_i64("18446744073709551615"), None);
    }

    #[test]
    fn period_and_admin_comparison_cover_boundaries() {
        assert!(valid_period(197001));
        assert!(valid_period(999912));
        assert!(!valid_period(197000));
        assert!(!valid_period(196912));
        assert!(!valid_period(202613));
        assert!(constant_time_eq(b"operator", b"operator"));
        assert!(!constant_time_eq(b"operator", b"operator-x"));
        assert!(!constant_time_eq(b"operator", b"different"));
    }
}
