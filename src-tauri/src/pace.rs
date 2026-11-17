//! On-track status for categories, income, and savings.

use crate::calendar;
use crate::money::{Cents, split_twelfths};
use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy)]
pub struct PaceSettings {
    /// Red when ahead of pace by at least this many percentage points of the annual limit
    pub warning_threshold_pct: f64,
    /// Lumpy categories turn yellow at this % of the limit used
    pub lumpy_warning_pct: f64,
}

impl Default for PaceSettings {
    fn default() -> Self {
        PaceSettings {
            warning_threshold_pct: 10.0,
            lumpy_warning_pct: 90.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub limit_cents: Cents,
    /// Net spending (purchases minus refunds)
    pub spent_cents: Cents,
    /// Negative when over
    pub remaining_cents: Cents,
    pub pct_used: f64,
    /// Expected spending by now at an even pace; None for lumpy categories
    pub expected_cents: Option<Cents>,
    /// Spending beyond the even pace (negative = under pace); None for lumpy categories
    pub ahead_cents: Option<Cents>,
    /// How far over the annual limit (0 when not over)
    pub over_cents: Cents,
    /// Average weekly spend that would land exactly on the limit; None when the year is over
    pub weekly_target_cents: Option<Cents>,
    pub status: Status,
}

pub fn category_progress(
    limit: Cents,
    spent: Cents,
    lumpy: bool,
    year: i32,
    as_of: NaiveDate,
    settings: PaceSettings,
) -> Progress {
    let remaining = limit - spent;
    let pct_used = if limit > 0 {
        spent as f64 / limit as f64 * 100.0
    } else if spent > 0 {
        100.0
    } else {
        0.0
    };
    let weeks_left = calendar::weeks_remaining(year, as_of);
    let weekly_target =
        (weeks_left > 0.0).then(|| (remaining.max(0) as f64 / weeks_left).round() as Cents);
    let over = (spent - limit).max(0);

    let (expected, ahead, status) = if lumpy {
        let status = if over > 0 {
            Status::Red
        } else if limit > 0 && pct_used >= settings.lumpy_warning_pct {
            Status::Yellow
        } else {
            Status::Green
        };
        (None, None, status)
    } else {
        let expected = (limit as f64 * calendar::fraction_elapsed(year, as_of)).round() as Cents;
        let ahead = spent - expected;
        let status = if over > 0 {
            Status::Red
        } else if ahead <= 0 {
            Status::Green
        } else if ahead as f64 * 100.0 >= settings.warning_threshold_pct * limit as f64 {
            Status::Red
        } else {
            Status::Yellow
        };
        (Some(expected), Some(ahead), status)
    };

    Progress {
        limit_cents: limit,
        spent_cents: spent,
        remaining_cents: remaining,
        pct_used,
        expected_cents: expected,
        ahead_cents: ahead,
        over_cents: over,
        weekly_target_cents: weekly_target,
        status,
    }
}

/// How much of `amount` is expected to have arrived by `as_of`.
///
/// Steady amounts accrue evenly by day. Lumpy amounts are split evenly across their expected
/// months and each month's share counts once that month has ended. A lumpy amount with no
/// months is treated as steady.
pub fn expected_by_now(
    amount: Cents,
    lumpy_months: Option<&[u32]>,
    year: i32,
    as_of: NaiveDate,
) -> Cents {
    match lumpy_months {
        Some(months) if !months.is_empty() => {
            let n = months.len() as i64;
            let mut expected = 0;
            for (i, &m) in months.iter().enumerate() {
                // Spread any remainder cents onto the first months
                let share = amount / n + if (i as i64) < amount % n { 1 } else { 0 };
                if calendar::month_end(year, m) < as_of {
                    expected += share;
                }
            }
            expected
        }
        _ => (amount as f64 * calendar::fraction_elapsed(year, as_of)).round() as Cents,
    }
}

/// Parse a comma-separated month list like "3" or "6,12" (invalid entries ignored).
pub fn parse_months(s: &str) -> Vec<u32> {
    let mut months: Vec<u32> = s
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .filter(|m| (1..=12).contains(m))
        .collect();
    months.sort_unstable();
    months.dedup();
    months
}

/// Current-year and next-year shares of a split-year amount.
pub fn split_shares(amount: Cents, split_current_twelfths: Option<i64>) -> (Cents, Cents) {
    match split_current_twelfths {
        Some(tw) => split_twelfths(amount, tw),
        None => (amount, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // 2026-07-02 is day 183 of 365, almost exactly half the year
    const MID: (i32, u32, u32) = (2026, 7, 2);

    fn mid() -> NaiveDate {
        d(MID.0, MID.1, MID.2)
    }

    fn status(limit: Cents, spent: Cents) -> Status {
        category_progress(limit, spent, false, 2026, mid(), PaceSettings::default()).status
    }

    #[test]
    fn pace_colors() {
        assert_eq!(status(300_000, 135_000), Status::Green); // 45%, under pace
        assert_eq!(status(300_000, 165_000), Status::Yellow); // 55%, ~5 points ahead
        assert_eq!(status(300_000, 180_500), Status::Red); // ~60%, 10 points ahead
        assert_eq!(status(300_000, 310_000), Status::Red); // over the limit
    }

    #[test]
    fn over_limit_reports_amount_over() {
        let p = category_progress(
            300_000,
            310_000,
            false,
            2026,
            mid(),
            PaceSettings::default(),
        );
        assert_eq!(p.over_cents, 10_000);
        assert_eq!(p.remaining_cents, -10_000);
        assert!(p.pct_used > 100.0);
    }

    #[test]
    fn lumpy_ignores_timing() {
        let p = |spent| {
            category_progress(
                250_000,
                spent,
                true,
                2026,
                d(2026, 2, 1),
                PaceSettings::default(),
            )
        };
        assert_eq!(p(200_000).status, Status::Green); // 80% in February is fine
        assert_eq!(p(225_000).status, Status::Yellow); // 90%
        assert_eq!(p(260_000).status, Status::Red);
        assert!(p(0).expected_cents.is_none());
    }

    #[test]
    fn weekly_target() {
        let p = category_progress(
            360_000,
            180_000,
            false,
            2026,
            mid(),
            PaceSettings::default(),
        );
        // $1,800 left over 182 days = 26 weeks -> about $69/week
        assert_eq!(p.weekly_target_cents, Some(6_923));
        let done = category_progress(
            360_000,
            180_000,
            false,
            2026,
            d(2026, 12, 31),
            PaceSettings::default(),
        );
        assert_eq!(done.weekly_target_cents, None);
    }

    #[test]
    fn adjusted_limit_resets_pace() {
        // Spent $1,800 by mid-year: ahead of pace at $3,000, on pace at $3,600
        assert_eq!(status(300_000, 180_000), Status::Yellow);
        assert_eq!(status(360_000, 180_000), Status::Green);
    }

    #[test]
    fn refunds_can_make_spending_negative() {
        assert_eq!(status(100_000, -5_000), Status::Green);
    }

    #[test]
    fn zero_limit() {
        assert_eq!(status(0, 0), Status::Green);
        assert_eq!(status(0, 100), Status::Red);
    }

    #[test]
    fn income_expected_steady_and_lumpy() {
        assert_eq!(expected_by_now(365_000, None, 2026, d(2026, 1, 10)), 10_000);
        let march = [3u32];
        assert_eq!(
            expected_by_now(1_200_000, Some(&march[..]), 2026, d(2026, 3, 31)),
            0
        );
        assert_eq!(
            expected_by_now(1_200_000, Some(&march[..]), 2026, d(2026, 4, 1)),
            1_200_000
        );
        let twice = [6u32, 12];
        assert_eq!(
            expected_by_now(1_001, Some(&twice[..]), 2026, d(2026, 7, 1)),
            501
        );
        assert_eq!(
            expected_by_now(1_001, Some(&twice[..]), 2027, d(2028, 1, 1)),
            1_001
        );
        assert_eq!(
            expected_by_now(1_000, Some(&[][..]), 2026, d(2027, 1, 1)),
            1_000
        );
    }

    #[test]
    fn months_parse() {
        assert_eq!(parse_months("12, 6,6, x, 13"), vec![6, 12]);
        assert!(parse_months("").is_empty());
    }
}
