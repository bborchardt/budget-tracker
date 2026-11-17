//! Everything the dashboard and plan screens show for one year.

use crate::calendar::{self, Week};
use crate::error::Result;
use crate::money::Cents;
use crate::pace::{self, Progress};
use crate::plan::{self, Category, IncomeSource, Plan, PlanTotals};
use crate::savings::{self, AccountBalance};
use crate::settings;
use crate::transactions;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct CategoryRow {
    #[serde(flatten)]
    pub category: Category,
    pub progress: Progress,
    /// Date the limit last changed (adjustment, reallocation, or merge), for the change marker
    pub limit_changed_on: Option<String>,
    /// Transactions in this category with a guessed ("auto") tag
    pub auto_count: i64,
}

#[derive(Debug, Serialize)]
pub struct IncomeRow {
    #[serde(flatten)]
    pub source: IncomeSource,
    pub actual_cents: Cents,
    pub expected_cents: Cents,
    pub current_share_cents: Cents,
    pub next_share_cents: Cents,
    /// An estimate with income received against it: time to confirm the actual amount
    pub needs_confirmation: bool,
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub limit_cents: Cents,
    pub spent_cents: Cents,
    pub remaining_cents: Cents,
}

#[derive(Debug, Serialize)]
pub struct IncomeTotals {
    pub projected_cents: Cents,
    pub actual_cents: Cents,
    pub expected_cents: Cents,
}

#[derive(Debug, Serialize)]
pub struct SavingsSummary {
    pub planned_holdback_cents: Cents,
    pub planned_draw_cents: Cents,
    pub saved_cents: Cents,
    pub drawn_cents: Cents,
    pub expected_saved_cents: Cents,
    pub expected_drawn_cents: Cents,
    pub accounts: Vec<AccountBalance>,
    pub total_balance_cents: Cents,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekSpend {
    pub week: Week,
    pub spent_cents: Cents,
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub year: i32,
    pub today: NaiveDate,
    /// The date progress is measured at: today in the current year, Dec 31 for past years
    pub as_of: NaiveDate,
    pub fraction_elapsed: f64,
    pub plan: Option<Plan>,
    pub tracking: bool,
    pub totals: Option<PlanTotals>,
    pub categories: Vec<CategoryRow>,
    pub spending: Totals,
    pub income: Vec<IncomeRow>,
    pub income_totals: IncomeTotals,
    pub savings: SavingsSummary,
    /// Spending per week from the start of the year through the as-of week
    pub weeks: Vec<WeekSpend>,
    pub current_week: Option<Week>,
    /// The most recent week that has fully ended
    pub last_week: Option<WeekSpend>,
    pub pending_count: i64,
    pub auto_count: i64,
}

fn as_of_for(year: i32, today: NaiveDate) -> NaiveDate {
    if year < today.year() {
        calendar::year_end(year)
    } else if year > today.year() {
        calendar::year_start(year) - Duration::days(1)
    } else {
        today
    }
}

fn weekly_spending(
    conn: &Connection,
    year: i32,
    as_of: NaiveDate,
    category_id: Option<i64>,
) -> Result<Vec<WeekSpend>> {
    let mut stmt = conn.prepare(
        "SELECT date, -SUM(amount_cents) FROM transactions
         WHERE type = 'expense' AND deleted_at IS NULL AND date >= ?1 AND date <= ?2
           AND (?3 IS NULL OR category_id = ?3)
         GROUP BY date",
    )?;
    let by_day: Vec<(String, Cents)> = stmt
        .query_map(
            params![
                format!("{year}-01-01"),
                format!("{year}-12-31"),
                category_id
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let mut per_week: HashMap<u32, Cents> = HashMap::new();
    for (date, spent) in by_day {
        if let Some(d) = calendar::parse_iso(&date) {
            *per_week.entry(calendar::week_of(d).index).or_default() += spent;
        }
    }
    Ok(calendar::weeks_in_year(year)
        .into_iter()
        .filter(|w| w.start <= as_of)
        .map(|w| WeekSpend {
            spent_cents: per_week.get(&w.index).copied().unwrap_or(0),
            week: w,
        })
        .collect())
}

/// Spending in one category over the last 12 weeks of its plan year (through today).
pub fn category_trend(
    conn: &Connection,
    category_id: i64,
    today: NaiveDate,
) -> Result<Vec<WeekSpend>> {
    let c = plan::category(conn, category_id)?;
    let p = plan::plan(conn, c.plan_id)?;
    let weeks = weekly_spending(conn, p.year, as_of_for(p.year, today), Some(category_id))?;
    let skip = weeks.len().saturating_sub(12);
    Ok(weeks.into_iter().skip(skip).collect())
}

pub fn overview(conn: &Connection, year: i32, today: NaiveDate) -> Result<Overview> {
    let as_of = as_of_for(year, today);
    let pace_settings = settings::pace(conn)?;
    let plan = plan::plan_for_year(conn, year)?;

    let mut categories = Vec::new();
    let mut income = Vec::new();
    let (mut totals, mut tracking) = (None, false);
    let mut auto_counts: HashMap<i64, i64> = HashMap::new();

    if let Some(p) = &plan {
        totals = Some(plan::totals(conn, p)?);
        tracking = plan::is_tracking(conn, p.id)?;
        let spent = transactions::spending_by_category(conn, p.id)?;
        let mut stmt = conn.prepare(
            "SELECT t.category_id, COUNT(*) FROM transactions t JOIN categories c ON c.id = t.category_id
             WHERE c.plan_id = ?1 AND t.is_auto = 1 AND t.deleted_at IS NULL GROUP BY t.category_id",
        )?;
        for row in stmt.query_map([p.id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (id, n) = row?;
            auto_counts.insert(id, n);
        }
        for c in plan::categories(conn, p.id)? {
            let progress = pace::category_progress(
                c.annual_limit_cents,
                spent.get(&c.id).copied().unwrap_or(0),
                c.is_lumpy,
                year,
                as_of,
                pace_settings,
            );
            categories.push(CategoryRow {
                limit_changed_on: plan::last_limit_change(conn, c.id)?,
                auto_count: auto_counts.get(&c.id).copied().unwrap_or(0),
                progress,
                category: c,
            });
        }
        for s in plan::income_sources(conn, p.id)? {
            let actual = plan::income_actual(conn, s.id)?;
            let (current, next) = s.shares();
            income.push(IncomeRow {
                expected_cents: pace::expected_by_now(
                    s.projected_cents,
                    s.lumpy_months(),
                    year,
                    as_of,
                ),
                needs_confirmation: s.is_estimate && actual > 0,
                actual_cents: actual,
                current_share_cents: current,
                next_share_cents: next,
                source: s,
            });
        }
    }

    let spending = Totals {
        limit_cents: categories.iter().map(|c| c.progress.limit_cents).sum(),
        spent_cents: categories.iter().map(|c| c.progress.spent_cents).sum(),
        remaining_cents: categories.iter().map(|c| c.progress.remaining_cents).sum(),
    };
    let income_totals = IncomeTotals {
        projected_cents: income.iter().map(|i| i.source.projected_cents).sum(),
        actual_cents: income.iter().map(|i| i.actual_cents).sum(),
        expected_cents: income.iter().map(|i| i.expected_cents).sum(),
    };

    // Savings: manual hold-back accrues evenly; split-year shares are expected when the payment is
    let fraction = calendar::fraction_elapsed(year, as_of);
    let (planned_holdback, planned_draw, expected_saved, expected_drawn) = match (&plan, &totals) {
        (Some(p), Some(t)) => {
            let auto_expected: Cents = income
                .iter()
                .map(|i| {
                    pace::expected_by_now(i.next_share_cents, i.source.lumpy_months(), year, as_of)
                })
                .sum();
            (
                t.total_holdback_cents,
                t.total_draw_cents,
                (p.planned_holdback_cents as f64 * fraction).round() as Cents + auto_expected,
                (t.total_draw_cents as f64 * fraction).round() as Cents,
            )
        }
        _ => (0, 0, 0, 0),
    };
    let (saved, drawn) = savings::year_flows(conn, year, as_of)?;
    let accounts: Vec<AccountBalance> = savings::list_accounts(conn)?
        .iter()
        .filter(|a| !a.archived)
        .map(|a| savings::balance(conn, a, as_of))
        .collect::<Result<_>>()?;
    let savings = SavingsSummary {
        planned_holdback_cents: planned_holdback,
        planned_draw_cents: planned_draw,
        saved_cents: saved,
        drawn_cents: drawn,
        expected_saved_cents: expected_saved,
        expected_drawn_cents: expected_drawn,
        total_balance_cents: accounts.iter().map(|a| a.balance_cents).sum(),
        accounts,
    };

    let weeks = weekly_spending(conn, year, as_of, None)?;
    let current_week = (year == today.year()).then(|| calendar::week_of(today));
    let last_week = weeks.iter().rev().find(|w| w.week.end < today).cloned();
    let (pending_count, auto_count): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(is_pending), 0), COALESCE(SUM(is_auto), 0) FROM transactions
         WHERE deleted_at IS NULL AND date >= ?1 AND date <= ?2",
        params![format!("{year}-01-01"), format!("{year}-12-31")],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    Ok(Overview {
        year,
        today,
        as_of,
        fraction_elapsed: fraction,
        plan,
        tracking,
        totals,
        categories,
        spending,
        income,
        income_totals,
        savings,
        weeks,
        current_week,
        last_week,
        pending_count,
        auto_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::plan::tests::{d, insert_expense, insert_income, steady};

    #[test]
    fn overview_pulls_it_together() {
        let conn = db::open_in_memory().unwrap();
        let start = d(2026, 1, 1);
        let p = plan::create_plan(&conn, 2026, None).unwrap();
        let groceries =
            plan::add_category(&conn, p.id, "Groceries", 600_000, false, start).unwrap();
        let salary =
            plan::add_income_source(&conn, p.id, &steady("Salary"), 7_300_000, start).unwrap();
        plan::set_savings_plan(&conn, p.id, 730_000, 0, "", start).unwrap();
        insert_expense(&conn, "2026-11-10", groceries.id, -8_612);
        insert_expense(&conn, "2026-11-12", groceries.id, -4_210);
        insert_expense(&conn, "2026-11-13", groceries.id, 1_000); // refund
        insert_income(&conn, "2026-11-01", salary.id, 280_000);

        let o = overview(&conn, 2026, d(2026, 11, 14)).unwrap();
        assert!(o.tracking);
        let g = o
            .categories
            .iter()
            .find(|c| c.category.id == groceries.id)
            .unwrap();
        assert_eq!(g.progress.spent_cents, 11_822);
        assert_eq!(o.income_totals.actual_cents, 280_000);
        // Nov 14 is day 318: 318/365 of $73,000
        assert_eq!(o.income[0].expected_cents, 6_360_000);
        assert_eq!(o.savings.expected_saved_cents, 636_000);
        // Week of Nov 6-12 (Fri-Thu) is the last full week before Nov 14
        let last = o.last_week.unwrap();
        assert_eq!(
            (last.week.start, last.week.end),
            (d(2026, 11, 6), d(2026, 11, 12))
        );
        assert_eq!(last.spent_cents, 12_822);
        assert_eq!(o.weeks.last().unwrap().spent_cents, -1_000);

        let past = overview(&conn, 2025, d(2026, 11, 14)).unwrap();
        assert!(past.plan.is_none());
        assert_eq!(past.as_of, d(2025, 12, 31));
    }
}
