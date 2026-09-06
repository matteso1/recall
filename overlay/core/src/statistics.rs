//! Uncertainty for aggregate observations; never a prediction of causal build benefit.
//!
//! These intervals describe sampling uncertainty under binomial assumptions. The source does
//! not randomize builds: match length, skill, opponents, and when players finish items confound
//! observed win rates. Repeated API snapshots overlap and must never be treated as new samples.

use serde::{Deserialize, Serialize};

/// All intervals in this module use a fixed two-sided 95% confidence level.
pub const CONFIDENCE_LEVEL: f64 = 0.95;
const Z_95: f64 = 1.959_963_984_540_054;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Interval {
    pub lower: f64,
    pub upper: f64,
}

/// Wilson score interval without continuity correction for a single observed win proportion.
/// No interval is returned for an empty sample or impossible counts.
/// Formula: <https://www.itl.nist.gov/div898/handbook/prc/section2/prc241.htm>.
pub fn wilson_interval(wins: u32, games: u32) -> Option<Interval> {
    if games == 0 || wins > games {
        return None;
    }
    let n = f64::from(games);
    let p = f64::from(wins) / n;
    let z2 = Z_95 * Z_95;
    let denominator = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denominator;
    let margin = Z_95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denominator;
    Some(Interval {
        lower: (center - margin).clamp(0.0, 1.0),
        upper: (center + margin).clamp(0.0, 1.0),
    })
}

/// Newcombe hybrid-score interval for A's observed win rate minus B's. This assumes independent
/// binomial samples, not repeated snapshots or overlapping builds. Even an interval excluding
/// zero does not establish that changing a player's build causes a benefit.
/// Formula: <https://support.sas.com/documentation/cdl/en/statug/68162/HTML/default/statug_freq_details53.htm>.
pub fn independent_difference_interval(
    wins_a: u32,
    games_a: u32,
    wins_b: u32,
    games_b: u32,
) -> Option<Interval> {
    let a = wilson_interval(wins_a, games_a)?;
    let b = wilson_interval(wins_b, games_b)?;
    let p_a = f64::from(wins_a) / f64::from(games_a);
    let p_b = f64::from(wins_b) / f64::from(games_b);
    let difference = p_a - p_b;
    Some(Interval {
        lower: (difference - (p_a - a.lower).hypot(b.upper - p_b)).clamp(-1.0, 1.0),
        upper: (difference + (a.upper - p_a).hypot(p_b - b.lower)).clamp(-1.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_matches_reference_values_including_boundary_samples() {
        // Independent 95% Wilson score values (without continuity correction).
        for (wins, games, lower, upper) in [
            (50, 100, 0.4038315304, 0.5961684696),
            (0, 100, 0.0, 0.0369934982),
            (100, 100, 0.9630065018, 1.0),
        ] {
            let interval = wilson_interval(wins, games).unwrap();
            assert!(
                (interval.lower - lower).abs() < 1e-9,
                "{wins}/{games}: {interval:?}"
            );
            assert!(
                (interval.upper - upper).abs() < 1e-9,
                "{wins}/{games}: {interval:?}"
            );
        }
    }

    #[test]
    fn no_games_or_impossible_wins_have_no_interval() {
        for (wins, games) in [(0, 0), (1, 0), (11, 10), (u32::MAX, 1)] {
            assert_eq!(wilson_interval(wins, games), None);
            assert_eq!(independent_difference_interval(wins, games, 5, 10), None);
            assert_eq!(independent_difference_interval(5, 10, wins, games), None);
        }
    }

    #[test]
    fn real_core_samples_do_not_resolve_an_observed_rate_difference() {
        let popular = wilson_interval(882, 1542).unwrap();
        let alternative = wilson_interval(384, 639).unwrap();
        assert!(popular.lower < alternative.upper && alternative.lower < popular.upper);
        let difference = independent_difference_interval(384, 639, 882, 1542).unwrap();
        assert!(
            difference.lower < 0.0 && difference.upper > 0.0,
            "{difference:?}"
        );
        assert!((difference.lower - -0.0166423222).abs() < 1e-9);
        assert!((difference.upper - 0.0737359424).abs() < 1e-9);
    }

    #[test]
    fn difference_interval_reverses_sign_when_samples_are_swapped() {
        let forward = independent_difference_interval(80, 100, 40, 100).unwrap();
        let backward = independent_difference_interval(40, 100, 80, 100).unwrap();
        assert!(forward.lower > 0.0);
        assert!((forward.lower + backward.upper).abs() < 1e-12);
        assert!((forward.upper + backward.lower).abs() < 1e-12);
    }

    #[test]
    fn large_samples_remain_finite_and_boundary_differences_stay_bounded() {
        let interval = wilson_interval(u32::MAX / 2, u32::MAX).unwrap();
        assert!(interval.lower.is_finite() && interval.upper.is_finite());
        assert!(interval.lower < 0.5 && interval.upper > 0.5);
        let difference = independent_difference_interval(1, 1, 0, 1).unwrap();
        assert!(difference.lower >= -1.0 && difference.upper <= 1.0);
        assert!(
            difference.lower < 0.0,
            "one win versus one loss is still uncertain"
        );
    }
}
