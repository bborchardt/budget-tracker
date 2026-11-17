//! Weeks and the passage of the year.
//!
//! Weeks run Friday through Thursday. Week 1 runs from Jan 1 through the first Thursday and
//! the last week ends Dec 31, so every week belongs to exactly one plan year. Pace is
//! day-based; weeks are only for grouping spending.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Serialize;

pub const WEEK_START: Weekday = Weekday::Fri;

pub fn year_start(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year")
}

pub fn year_end(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 12, 31).expect("valid year")
}

pub fn days_in_year(year: i32) -> i64 {
    if NaiveDate::from_ymd_opt(year, 2, 29).is_some() {
        366
    } else {
        365
    }
}

pub fn month_end(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid month") - Duration::days(1)
}

fn on_or_after(date: NaiveDate, weekday: Weekday) -> NaiveDate {
    let offset = (7 + weekday.num_days_from_monday() as i64
        - date.weekday().num_days_from_monday() as i64)
        % 7;
    date + Duration::days(offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Week {
    pub year: i32,
    /// 1-based
    pub index: u32,
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// Shorter than 7 days (only at the start or end of a year)
    pub partial: bool,
}

pub fn week_of(date: NaiveDate) -> Week {
    let year = date.year();
    let first_end = on_or_after(year_start(year), WEEK_START.pred());
    let (index, start, end) = if date <= first_end {
        (1, year_start(year), first_end)
    } else {
        let second_start = first_end + Duration::days(1);
        let k = (date - second_start).num_days() / 7;
        let start = second_start + Duration::days(7 * k);
        let end = (start + Duration::days(6)).min(year_end(year));
        (2 + k as u32, start, end)
    };
    Week {
        year,
        index,
        start,
        end,
        partial: (end - start).num_days() < 6,
    }
}

pub fn weeks_in_year(year: i32) -> Vec<Week> {
    let mut weeks = Vec::new();
    let mut d = year_start(year);
    while d.year() == year {
        let w = week_of(d);
        d = w.end + Duration::days(1);
        weeks.push(w);
    }
    weeks
}

/// Days of `year` that have passed as of the end of `as_of` (0 before the year, all after it).
pub fn days_elapsed(year: i32, as_of: NaiveDate) -> i64 {
    if as_of < year_start(year) {
        0
    } else if as_of > year_end(year) {
        days_in_year(year)
    } else {
        (as_of - year_start(year)).num_days() + 1
    }
}

pub fn fraction_elapsed(year: i32, as_of: NaiveDate) -> f64 {
    days_elapsed(year, as_of) as f64 / days_in_year(year) as f64
}

/// Weeks left in the year after `as_of`, as a fraction (e.g. 26.0 at the end of June).
pub fn weeks_remaining(year: i32, as_of: NaiveDate) -> f64 {
    (days_in_year(year) - days_elapsed(year, as_of)) as f64 / 7.0
}

pub fn parse_iso(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn weeks_run_friday_to_thursday() {
        // 2027-01-01 is a Friday: week 1 is a full week
        let w = week_of(d(2027, 1, 1));
        assert_eq!(
            (w.index, w.start, w.end, w.partial),
            (1, d(2027, 1, 1), d(2027, 1, 7), false)
        );
        let w = week_of(d(2027, 1, 8));
        assert_eq!(
            (w.index, w.start, w.end),
            (2, d(2027, 1, 8), d(2027, 1, 14))
        );
        assert_eq!(w.start.weekday(), Weekday::Fri);
        assert_eq!(w.end.weekday(), Weekday::Thu);
    }

    #[test]
    fn partial_weeks_at_year_edges() {
        // 2026-01-01 is a Thursday: week 1 is just Jan 1
        let w = week_of(d(2026, 1, 1));
        assert_eq!(
            (w.index, w.start, w.end, w.partial),
            (1, d(2026, 1, 1), d(2026, 1, 1), true)
        );
        let w = week_of(d(2026, 1, 2));
        assert_eq!(
            (w.index, w.start, w.end, w.partial),
            (2, d(2026, 1, 2), d(2026, 1, 8), false)
        );
        // Last week is cut off at Dec 31 (a Thursday in 2026, so it's full)
        let w = week_of(d(2026, 12, 31));
        assert_eq!(
            (w.start, w.end, w.partial),
            (d(2026, 12, 25), d(2026, 12, 31), false)
        );
        // 2028-12-31 is a Sunday: last week is Fri-Sun
        let w = week_of(d(2028, 12, 31));
        assert_eq!(
            (w.start, w.end, w.partial),
            (d(2028, 12, 29), d(2028, 12, 31), true)
        );
    }

    #[test]
    fn weeks_cover_the_year_exactly() {
        for year in 2025..=2032 {
            let weeks = weeks_in_year(year);
            let days: i64 = weeks.iter().map(|w| (w.end - w.start).num_days() + 1).sum();
            assert_eq!(days, days_in_year(year));
            assert!(
                weeks
                    .windows(2)
                    .all(|p| p[1].start == p[0].end + Duration::days(1))
            );
            assert!(
                weeks
                    .iter()
                    .enumerate()
                    .all(|(i, w)| w.index as usize == i + 1)
            );
        }
    }

    #[test]
    fn elapsed_is_day_based() {
        assert_eq!(days_elapsed(2026, d(2025, 12, 31)), 0);
        assert_eq!(days_elapsed(2026, d(2026, 1, 1)), 1);
        assert_eq!(days_elapsed(2026, d(2026, 12, 31)), 365);
        assert_eq!(days_elapsed(2028, d(2029, 3, 1)), 366);
        assert!((fraction_elapsed(2026, d(2026, 7, 2)) - 183.0 / 365.0).abs() < 1e-9);
        assert_eq!(weeks_remaining(2026, d(2026, 12, 31)), 0.0);
    }

    #[test]
    fn month_ends() {
        assert_eq!(month_end(2028, 2), d(2028, 2, 29));
        assert_eq!(month_end(2026, 12), d(2026, 12, 31));
    }
}
