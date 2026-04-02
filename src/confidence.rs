use chrono::{DateTime, Utc};

const HALF_LIFE_DAYS: f64 = 90.0;
const HARM_MULTIPLIER: f64 = 4.0;

/// Compute confidence score using exponential decay.
///
/// Formula: C = (S - 4H) * 0.5^(dt / 90)
///
/// - `success_count`: number of successful validations (S)
/// - `failure_count`: number of harmful outcomes (H)
/// - `last_validated`: timestamp of last validation
/// - `now`: current timestamp
///
/// Returns the decayed confidence as f64. Can be negative when failures dominate.
pub fn confidence(
    success_count: u32,
    failure_count: u32,
    last_validated: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f64 {
    let base = success_count as f64 - HARM_MULTIPLIER * failure_count as f64;
    let dt_days = (now - last_validated).num_seconds() as f64 / 86_400.0;
    let dt_days = dt_days.max(0.0); // clamp negative dt to zero
    base * (0.5_f64).powf(dt_days / HALF_LIFE_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    #[test]
    fn zero_counts_returns_zero() {
        let t = utc(2026, 1, 1);
        assert_eq!(confidence(0, 0, t, t), 0.0);
    }

    #[test]
    fn no_decay_at_same_timestamp() {
        let t = utc(2026, 1, 1);
        // base = 10 - 4*1 = 6, decay factor = 0.5^0 = 1
        assert!((confidence(10, 1, t, t) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn exact_half_life() {
        let t0 = utc(2026, 1, 1);
        let t1 = utc(2026, 4, 1); // 90 days later
        // base = 8 - 0 = 8, decay = 0.5^1 = 0.5 => 4.0
        assert!((confidence(8, 0, t0, t1) - 4.0).abs() < 0.1);
    }

    #[test]
    fn double_half_life() {
        let t0 = utc(2026, 1, 1);
        let t1 = utc(2026, 7, 1); // ~180 days
        // base = 8, decay = 0.5^2 = 0.25 => 2.0
        assert!((confidence(8, 0, t0, t1) - 2.0).abs() < 0.15);
    }

    #[test]
    fn negative_confidence_when_failures_dominate() {
        let t = utc(2026, 1, 1);
        // base = 1 - 4*2 = -7
        let c = confidence(1, 2, t, t);
        assert!((c - (-7.0)).abs() < 1e-9);
    }

    #[test]
    fn negative_confidence_decays_toward_zero() {
        let t0 = utc(2026, 1, 1);
        let t1 = utc(2026, 4, 1); // 90 days
        // base = -4, decay = 0.5 => -2.0
        let c = confidence(0, 1, t0, t1);
        assert!((c - (-2.0)).abs() < 0.1);
    }

    #[test]
    fn large_dt_decays_to_near_zero() {
        let t0 = utc(2020, 1, 1);
        let t1 = utc(2026, 1, 1); // ~2190 days, ~24 half-lives
        let c = confidence(100, 0, t0, t1);
        assert!(c.abs() < 0.001);
    }

    #[test]
    fn future_last_validated_clamps_to_zero_decay() {
        // If last_validated is in the future relative to now, treat dt as 0
        let t0 = utc(2026, 6, 1);
        let now = utc(2026, 1, 1);
        assert!((confidence(5, 0, t0, now) - 5.0).abs() < 1e-9);
    }
}
