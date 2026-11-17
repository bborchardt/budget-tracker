//! Annual plans: categories, income sources, the savings plan, and the adjustment log.
//!
//! Changes are recorded in the adjustment log once a plan is "tracking", meaning it has at
//! least one transaction. Before that, edits are just setup and aren't logged.

use crate::error::{AppError, Result};
use crate::money::Cents;
use crate::pace;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

pub const MISC_NAME: &str = "Misc";
pub const OTHER_INCOME_NAME: &str = "Other / Gifts";

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: i64,
    pub year: i32,
    pub planned_holdback_cents: Cents,
    pub planned_draw_cents: Cents,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub id: i64,
    pub plan_id: i64,
    pub name: String,
    pub annual_limit_cents: Cents,
    pub is_lumpy: bool,
    pub is_misc: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncomeSource {
    pub id: i64,
    pub plan_id: i64,
    pub name: String,
    pub projected_cents: Cents,
    /// "steady" or "lumpy"
    pub timing: String,
    pub expected_months: Vec<u32>,
    pub is_estimate: bool,
    pub is_other: bool,
    pub split_current_twelfths: Option<i64>,
    pub sort_order: i64,
}

impl IncomeSource {
    pub fn lumpy_months(&self) -> Option<&[u32]> {
        (self.timing == "lumpy").then_some(self.expected_months.as_slice())
    }
    pub fn shares(&self) -> (Cents, Cents) {
        pace::split_shares(self.projected_cents, self.split_current_twelfths)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IncomeSourceInput {
    pub name: String,
    pub timing: String,
    #[serde(default)]
    pub expected_months: Vec<u32>,
    #[serde(default)]
    pub is_estimate: bool,
    #[serde(default)]
    pub split_current_twelfths: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Adjustment {
    pub id: i64,
    pub date: String,
    pub kind: String,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub to_category_id: Option<i64>,
    pub to_category_name: Option<String>,
    pub income_source_id: Option<i64>,
    pub income_source_name: Option<String>,
    pub old_cents: Option<Cents>,
    pub new_cents: Option<Cents>,
    pub amount_cents: Option<Cents>,
    pub detail: String,
    pub reason: String,
}

/// The plan's balance sheet:
/// projected income − hold-back to savings + draws from savings = available, all assigned.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlanTotals {
    /// Full projected take-home income (split-year sources at their full amount)
    pub projected_income_cents: Cents,
    /// Next-year shares of split-year income, held back in savings automatically
    pub auto_holdback_cents: Cents,
    pub planned_holdback_cents: Cents,
    pub total_holdback_cents: Cents,
    /// Last year's split-year income carried into this year, drawn from savings automatically
    pub carried_in_cents: Cents,
    pub planned_draw_cents: Cents,
    pub total_draw_cents: Cents,
    pub available_cents: Cents,
    pub assigned_cents: Cents,
    /// Positive = not yet assigned; negative = over-assigned
    pub unassigned_cents: Cents,
}

pub fn compute_totals(
    sources: &[IncomeSource],
    planned_holdback: Cents,
    planned_draw: Cents,
    carried_in: Cents,
    category_limits: impl IntoIterator<Item = Cents>,
) -> PlanTotals {
    let projected: Cents = sources.iter().map(|s| s.projected_cents).sum();
    let auto_holdback: Cents = sources.iter().map(|s| s.shares().1).sum();
    let total_holdback = planned_holdback + auto_holdback;
    let total_draw = planned_draw + carried_in;
    let available = projected - total_holdback + total_draw;
    let assigned: Cents = category_limits.into_iter().sum();
    PlanTotals {
        projected_income_cents: projected,
        auto_holdback_cents: auto_holdback,
        planned_holdback_cents: planned_holdback,
        total_holdback_cents: total_holdback,
        carried_in_cents: carried_in,
        planned_draw_cents: planned_draw,
        total_draw_cents: total_draw,
        available_cents: available,
        assigned_cents: assigned,
        unassigned_cents: available - assigned,
    }
}

// ---------- reading ----------

fn plan_from_row(r: &Row) -> rusqlite::Result<Plan> {
    Ok(Plan {
        id: r.get(0)?,
        year: r.get(1)?,
        planned_holdback_cents: r.get(2)?,
        planned_draw_cents: r.get(3)?,
        archived: r.get(4)?,
    })
}

const PLAN_COLS: &str = "id, year, planned_holdback_cents, planned_draw_cents, archived";

pub fn list_plans(conn: &Connection) -> Result<Vec<Plan>> {
    let mut stmt = conn.prepare(&format!("SELECT {PLAN_COLS} FROM plans ORDER BY year DESC"))?;
    let rows = stmt
        .query_map([], plan_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn plan_for_year(conn: &Connection, year: i32) -> Result<Option<Plan>> {
    Ok(conn
        .query_row(
            &format!("SELECT {PLAN_COLS} FROM plans WHERE year = ?1"),
            [year],
            plan_from_row,
        )
        .optional()?)
}

pub fn plan(conn: &Connection, id: i64) -> Result<Plan> {
    conn.query_row(
        &format!("SELECT {PLAN_COLS} FROM plans WHERE id = ?1"),
        [id],
        plan_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("plan"))
}

fn category_from_row(r: &Row) -> rusqlite::Result<Category> {
    Ok(Category {
        id: r.get(0)?,
        plan_id: r.get(1)?,
        name: r.get(2)?,
        annual_limit_cents: r.get(3)?,
        is_lumpy: r.get(4)?,
        is_misc: r.get(5)?,
        sort_order: r.get(6)?,
    })
}

const CATEGORY_COLS: &str = "id, plan_id, name, annual_limit_cents, is_lumpy, is_misc, sort_order";

pub fn categories(conn: &Connection, plan_id: i64) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CATEGORY_COLS} FROM categories WHERE plan_id = ?1 AND removed_at IS NULL
         ORDER BY is_misc, sort_order, name COLLATE NOCASE"
    ))?;
    let rows = stmt
        .query_map([plan_id], category_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// An active (not removed) category.
pub fn category(conn: &Connection, id: i64) -> Result<Category> {
    conn.query_row(
        &format!("SELECT {CATEGORY_COLS} FROM categories WHERE id = ?1 AND removed_at IS NULL"),
        [id],
        category_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("category"))
}

pub fn misc_category(conn: &Connection, plan_id: i64) -> Result<Category> {
    conn.query_row(
        &format!("SELECT {CATEGORY_COLS} FROM categories WHERE plan_id = ?1 AND is_misc = 1"),
        [plan_id],
        category_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Misc category"))
}

fn source_from_row(r: &Row) -> rusqlite::Result<IncomeSource> {
    let months: String = r.get(5)?;
    Ok(IncomeSource {
        id: r.get(0)?,
        plan_id: r.get(1)?,
        name: r.get(2)?,
        projected_cents: r.get(3)?,
        timing: r.get(4)?,
        expected_months: pace::parse_months(&months),
        is_estimate: r.get(6)?,
        is_other: r.get(7)?,
        split_current_twelfths: r.get(8)?,
        sort_order: r.get(9)?,
    })
}

const SOURCE_COLS: &str = "id, plan_id, name, projected_cents, timing, expected_months, is_estimate, is_other, \
                           split_current_twelfths, sort_order";

pub fn income_sources(conn: &Connection, plan_id: i64) -> Result<Vec<IncomeSource>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SOURCE_COLS} FROM income_sources WHERE plan_id = ?1 AND removed_at IS NULL
         ORDER BY is_other, sort_order, name COLLATE NOCASE"
    ))?;
    let rows = stmt
        .query_map([plan_id], source_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn income_source(conn: &Connection, id: i64) -> Result<IncomeSource> {
    conn.query_row(
        &format!("SELECT {SOURCE_COLS} FROM income_sources WHERE id = ?1 AND removed_at IS NULL"),
        [id],
        source_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("income source"))
}

pub fn other_income_source(conn: &Connection, plan_id: i64) -> Result<IncomeSource> {
    conn.query_row(
        &format!("SELECT {SOURCE_COLS} FROM income_sources WHERE plan_id = ?1 AND is_other = 1"),
        [plan_id],
        source_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Other / Gifts income source"))
}

/// Total income received for a source (all its transactions; sources belong to one year).
pub fn income_actual(conn: &Connection, source_id: i64) -> Result<Cents> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount_cents), 0) FROM transactions
         WHERE income_source_id = ?1 AND type = 'income' AND deleted_at IS NULL",
        [source_id],
        |r| r.get(0),
    )?)
}

/// Next-year shares of last year's split-year income. Uses the actual amount received when
/// there is one, otherwise last year's projection.
pub fn carried_in(conn: &Connection, year: i32) -> Result<Cents> {
    let Some(prev) = plan_for_year(conn, year - 1)? else {
        return Ok(0);
    };
    let mut total = 0;
    for s in income_sources(conn, prev.id)? {
        if s.split_current_twelfths.is_none() {
            continue;
        }
        let actual = income_actual(conn, s.id)?;
        let base = if actual > 0 {
            actual
        } else {
            s.projected_cents
        };
        total += pace::split_shares(base, s.split_current_twelfths).1;
    }
    Ok(total)
}

pub fn totals(conn: &Connection, plan: &Plan) -> Result<PlanTotals> {
    let sources = income_sources(conn, plan.id)?;
    let limits = categories(conn, plan.id)?
        .into_iter()
        .map(|c| c.annual_limit_cents);
    Ok(compute_totals(
        &sources,
        plan.planned_holdback_cents,
        plan.planned_draw_cents,
        carried_in(conn, plan.year)?,
        limits,
    ))
}

/// A plan is tracking once it has transactions; from then on, changes are logged.
pub fn is_tracking(conn: &Connection, plan_id: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM transactions t
            LEFT JOIN categories c ON c.id = t.category_id
            LEFT JOIN income_sources s ON s.id = t.income_source_id
            WHERE t.deleted_at IS NULL AND (c.plan_id = ?1 OR s.plan_id = ?1))",
        [plan_id],
        |r| r.get(0),
    )?)
}

pub fn adjustments(conn: &Connection, plan_id: i64) -> Result<Vec<Adjustment>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.date, a.kind, a.category_id, c.name, a.to_category_id, tc.name,
                a.income_source_id, s.name, a.old_cents, a.new_cents, a.amount_cents, a.detail, a.reason
         FROM adjustments a
         LEFT JOIN categories c ON c.id = a.category_id
         LEFT JOIN categories tc ON tc.id = a.to_category_id
         LEFT JOIN income_sources s ON s.id = a.income_source_id
         WHERE a.plan_id = ?1
         ORDER BY a.date DESC, a.id DESC",
    )?;
    let rows = stmt
        .query_map([plan_id], |r| {
            Ok(Adjustment {
                id: r.get(0)?,
                date: r.get(1)?,
                kind: r.get(2)?,
                category_id: r.get(3)?,
                category_name: r.get(4)?,
                to_category_id: r.get(5)?,
                to_category_name: r.get(6)?,
                income_source_id: r.get(7)?,
                income_source_name: r.get(8)?,
                old_cents: r.get(9)?,
                new_cents: r.get(10)?,
                amount_cents: r.get(11)?,
                detail: r.get(12)?,
                reason: r.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Most recent date each category's limit changed (for the "↑ on Jul 1" marker).
pub fn last_limit_change(conn: &Connection, category_id: i64) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT MAX(date) FROM adjustments
         WHERE kind IN ('category_limit', 'reallocation', 'category_removed')
           AND (category_id = ?1 OR to_category_id = ?1)",
        [category_id],
        |r| r.get(0),
    )?)
}

// ---------- writing ----------

#[derive(Default)]
struct NewAdjustment<'a> {
    kind: &'a str,
    category_id: Option<i64>,
    to_category_id: Option<i64>,
    income_source_id: Option<i64>,
    old_cents: Option<Cents>,
    new_cents: Option<Cents>,
    amount_cents: Option<Cents>,
    detail: String,
    reason: &'a str,
}

fn log_if_tracking(
    conn: &Connection,
    plan_id: i64,
    today: NaiveDate,
    a: NewAdjustment,
) -> Result<()> {
    if !is_tracking(conn, plan_id)? {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO adjustments (plan_id, date, kind, category_id, to_category_id, income_source_id,
                                  old_cents, new_cents, amount_cents, detail, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            plan_id,
            today.to_string(),
            a.kind,
            a.category_id,
            a.to_category_id,
            a.income_source_id,
            a.old_cents,
            a.new_cents,
            a.amount_cents,
            a.detail,
            a.reason.trim()
        ],
    )?;
    Ok(())
}

fn clean_name(name: &str) -> Result<String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err(AppError::invalid("name can't be empty"));
    }
    Ok(n)
}

fn check_amount(cents: Cents) -> Result<()> {
    if cents < 0 {
        return Err(AppError::invalid("amount can't be negative"));
    }
    Ok(())
}

fn category_name_taken(
    conn: &Connection,
    plan_id: i64,
    name: &str,
    except: Option<i64>,
) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM categories WHERE plan_id = ?1 AND removed_at IS NULL
                        AND name = ?2 COLLATE NOCASE AND id IS NOT ?3)",
        params![plan_id, name, except],
        |r| r.get(0),
    )?)
}

fn source_name_taken(
    conn: &Connection,
    plan_id: i64,
    name: &str,
    except: Option<i64>,
) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM income_sources WHERE plan_id = ?1 AND removed_at IS NULL
                        AND name = ?2 COLLATE NOCASE AND id IS NOT ?3)",
        params![plan_id, name, except],
        |r| r.get(0),
    )?)
}

/// Create a plan for `year` with the built-in Misc category and Other / Gifts income source.
/// With `copy_from`, also copies that year's categories, limits, income sources, and savings plan.
pub fn create_plan(conn: &Connection, year: i32, copy_from: Option<i32>) -> Result<Plan> {
    if !(2000..=2200).contains(&year) {
        return Err(AppError::invalid("year out of range"));
    }
    if plan_for_year(conn, year)?.is_some() {
        return Err(AppError::invalid(format!(
            "a plan for {year} already exists"
        )));
    }
    let source = match copy_from {
        Some(y) => Some(
            plan_for_year(conn, y)?.ok_or_else(|| AppError::not_found(format!("plan for {y}")))?,
        ),
        None => None,
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO plans (year, planned_holdback_cents, planned_draw_cents) VALUES (?1, ?2, ?3)",
        params![
            year,
            source.as_ref().map_or(0, |p| p.planned_holdback_cents),
            source.as_ref().map_or(0, |p| p.planned_draw_cents)
        ],
    )?;
    let plan_id = tx.last_insert_rowid();

    let misc_limit = match &source {
        Some(p) => misc_category(&tx, p.id)?.annual_limit_cents,
        None => 0,
    };
    tx.execute(
        "INSERT INTO categories (plan_id, name, annual_limit_cents, is_misc, sort_order) VALUES (?1, ?2, ?3, 1, 9999)",
        params![plan_id, MISC_NAME, misc_limit],
    )?;
    tx.execute(
        "INSERT INTO income_sources (plan_id, name, is_other, sort_order) VALUES (?1, ?2, 1, 9999)",
        params![plan_id, OTHER_INCOME_NAME],
    )?;

    if let Some(p) = &source {
        for c in categories(&tx, p.id)?.into_iter().filter(|c| !c.is_misc) {
            tx.execute(
                "INSERT INTO categories (plan_id, name, annual_limit_cents, is_lumpy, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    plan_id,
                    c.name,
                    c.annual_limit_cents,
                    c.is_lumpy,
                    c.sort_order
                ],
            )?;
        }
        for s in income_sources(&tx, p.id)?
            .into_iter()
            .filter(|s| !s.is_other)
        {
            // Irregular amounts for a new year start out as estimates
            let estimate =
                s.is_estimate || s.timing == "lumpy" || s.split_current_twelfths.is_some();
            tx.execute(
                "INSERT INTO income_sources (plan_id, name, projected_cents, timing, expected_months,
                                             is_estimate, split_current_twelfths, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    plan_id,
                    s.name,
                    s.projected_cents,
                    s.timing,
                    months_to_string(&s.expected_months),
                    estimate,
                    s.split_current_twelfths,
                    s.sort_order
                ],
            )?;
        }
    }
    tx.commit()?;
    plan(conn, plan_id)
}

pub fn set_archived(conn: &Connection, plan_id: i64, archived: bool) -> Result<Plan> {
    conn.execute(
        "UPDATE plans SET archived = ?2 WHERE id = ?1",
        params![plan_id, archived],
    )?;
    plan(conn, plan_id)
}

fn months_to_string(months: &[u32]) -> String {
    months
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn add_category(
    conn: &Connection,
    plan_id: i64,
    name: &str,
    limit: Cents,
    lumpy: bool,
    today: NaiveDate,
) -> Result<Category> {
    let name = clean_name(name)?;
    check_amount(limit)?;
    plan(conn, plan_id)?;
    if category_name_taken(conn, plan_id, &name, None)? {
        return Err(AppError::invalid(format!(
            "a category named \"{name}\" already exists"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let next_order: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM categories WHERE plan_id = ?1 AND is_misc = 0",
        [plan_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO categories (plan_id, name, annual_limit_cents, is_lumpy, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![plan_id, name, limit, lumpy, next_order],
    )?;
    let id = tx.last_insert_rowid();
    log_if_tracking(
        &tx,
        plan_id,
        today,
        NewAdjustment {
            kind: "category_added",
            category_id: Some(id),
            new_cents: Some(limit),
            ..Default::default()
        },
    )?;
    tx.commit()?;
    category(conn, id)
}

/// Rename a category or change whether it's lumpy (not logged: limits are unchanged).
pub fn update_category(conn: &Connection, id: i64, name: &str, lumpy: bool) -> Result<Category> {
    let c = category(conn, id)?;
    let name = clean_name(name)?;
    if c.is_misc && name != c.name {
        return Err(AppError::invalid(
            "the built-in Misc category can't be renamed",
        ));
    }
    if category_name_taken(conn, c.plan_id, &name, Some(id))? {
        return Err(AppError::invalid(format!(
            "a category named \"{name}\" already exists"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE categories SET name = ?2, is_lumpy = ?3 WHERE id = ?1",
        params![id, name, lumpy],
    )?;
    // Learned tagging follows the rename
    tx.execute(
        "UPDATE tag_rules SET category_name = ?2 WHERE category_name = ?1 COLLATE NOCASE",
        params![c.name, name],
    )?;
    tx.commit()?;
    category(conn, id)
}

pub fn set_category_limit(
    conn: &Connection,
    id: i64,
    new_limit: Cents,
    reason: &str,
    today: NaiveDate,
) -> Result<Category> {
    check_amount(new_limit)?;
    let c = category(conn, id)?;
    if c.annual_limit_cents == new_limit {
        return Ok(c);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE categories SET annual_limit_cents = ?2 WHERE id = ?1",
        params![id, new_limit],
    )?;
    log_if_tracking(
        &tx,
        c.plan_id,
        today,
        NewAdjustment {
            kind: "category_limit",
            category_id: Some(id),
            old_cents: Some(c.annual_limit_cents),
            new_cents: Some(new_limit),
            reason,
            ..Default::default()
        },
    )?;
    tx.commit()?;
    category(conn, id)
}

/// Move part of one category's limit to another as a single logged change.
pub fn reallocate(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    amount: Cents,
    reason: &str,
    today: NaiveDate,
) -> Result<()> {
    if amount <= 0 {
        return Err(AppError::invalid("amount must be more than zero"));
    }
    if from_id == to_id {
        return Err(AppError::invalid("choose two different categories"));
    }
    let from = category(conn, from_id)?;
    let to = category(conn, to_id)?;
    if from.plan_id != to.plan_id {
        return Err(AppError::invalid("categories must be in the same plan"));
    }
    if amount > from.annual_limit_cents {
        return Err(AppError::invalid(format!(
            "{} doesn't have that much to move",
            from.name
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE categories SET annual_limit_cents = annual_limit_cents - ?2 WHERE id = ?1",
        params![from_id, amount],
    )?;
    tx.execute(
        "UPDATE categories SET annual_limit_cents = annual_limit_cents + ?2 WHERE id = ?1",
        params![to_id, amount],
    )?;
    log_if_tracking(
        &tx,
        from.plan_id,
        today,
        NewAdjustment {
            kind: "reallocation",
            category_id: Some(from_id),
            to_category_id: Some(to_id),
            amount_cents: Some(amount),
            reason,
            ..Default::default()
        },
    )?;
    tx.commit()?;
    Ok(())
}

/// Remove a category by merging it into another: its transactions move to `merge_into`, and
/// with `move_limit` its limit moves too. A category without transactions can be removed
/// without a target (its limit is then either moved to `merge_into` or released).
/// Returns how many transactions moved.
pub fn remove_category(
    conn: &Connection,
    id: i64,
    merge_into: Option<i64>,
    move_limit: bool,
    reason: &str,
    today: NaiveDate,
) -> Result<i64> {
    let c = category(conn, id)?;
    if c.is_misc {
        return Err(AppError::invalid(
            "the built-in Misc category can't be removed",
        ));
    }
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE category_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let target = match merge_into {
        Some(t) => {
            let t = category(conn, t)?;
            if t.plan_id != c.plan_id || t.id == c.id {
                return Err(AppError::invalid(
                    "choose another category in the same plan",
                ));
            }
            Some(t)
        }
        None if count > 0 => {
            return Err(AppError::invalid(
                "choose a category to move this category's transactions into",
            ));
        }
        None => None,
    };
    let tx = conn.unchecked_transaction()?;
    // Log before moving transactions so "tracking" reflects the plan's state
    let moved_limit = if move_limit && target.is_some() {
        c.annual_limit_cents
    } else {
        0
    };
    log_if_tracking(
        &tx,
        c.plan_id,
        today,
        NewAdjustment {
            kind: "category_removed",
            category_id: Some(id),
            to_category_id: target.as_ref().map(|t| t.id),
            old_cents: Some(c.annual_limit_cents),
            amount_cents: Some(moved_limit),
            detail: format!("{count} transactions moved"),
            reason,
            ..Default::default()
        },
    )?;
    if let Some(t) = &target {
        tx.execute(
            "UPDATE transactions SET category_id = ?2 WHERE category_id = ?1",
            params![id, t.id],
        )?;
        tx.execute(
            "UPDATE categories SET annual_limit_cents = annual_limit_cents + ?2 WHERE id = ?1",
            params![t.id, moved_limit],
        )?;
        tx.execute(
            "UPDATE tag_rules SET category_name = ?2 WHERE category_name = ?1 COLLATE NOCASE",
            params![c.name, t.name],
        )?;
    }
    tx.execute(
        "UPDATE categories SET removed_at = ?2, merged_into_id = ?3 WHERE id = ?1",
        params![
            id,
            crate::db::now_timestamp(),
            target.as_ref().map(|t| t.id)
        ],
    )?;
    tx.commit()?;
    Ok(count)
}

fn validate_source_input(input: &IncomeSourceInput) -> Result<(String, String, Vec<u32>)> {
    let name = clean_name(&input.name)?;
    let timing = match input.timing.as_str() {
        "steady" | "lumpy" => input.timing.clone(),
        _ => return Err(AppError::invalid("timing must be steady or lumpy")),
    };
    let mut months: Vec<u32> = input
        .expected_months
        .iter()
        .copied()
        .filter(|m| (1..=12).contains(m))
        .collect();
    months.sort_unstable();
    months.dedup();
    if let Some(tw) = input.split_current_twelfths
        && !(0..=12).contains(&tw)
    {
        return Err(AppError::invalid("split must be between 0 and 12 twelfths"));
    }
    Ok((name, timing, months))
}

/// Default split for a payment arriving in `month`: the months from its arrival through
/// December fund this year (e.g. arriving in May -> 8/12).
pub fn default_split_twelfths(month: u32) -> i64 {
    (13 - month.clamp(1, 12)) as i64
}

pub fn add_income_source(
    conn: &Connection,
    plan_id: i64,
    input: &IncomeSourceInput,
    projected: Cents,
    today: NaiveDate,
) -> Result<IncomeSource> {
    check_amount(projected)?;
    plan(conn, plan_id)?;
    let (name, timing, months) = validate_source_input(input)?;
    if source_name_taken(conn, plan_id, &name, None)? {
        return Err(AppError::invalid(format!(
            "an income source named \"{name}\" already exists"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let next_order: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM income_sources WHERE plan_id = ?1 AND is_other = 0",
        [plan_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO income_sources (plan_id, name, projected_cents, timing, expected_months, is_estimate,
                                     split_current_twelfths, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            plan_id,
            name,
            projected,
            timing,
            months_to_string(&months),
            input.is_estimate,
            input.split_current_twelfths,
            next_order
        ],
    )?;
    let id = tx.last_insert_rowid();
    log_if_tracking(
        &tx,
        plan_id,
        today,
        NewAdjustment {
            kind: "income_added",
            income_source_id: Some(id),
            new_cents: Some(projected),
            ..Default::default()
        },
    )?;
    tx.commit()?;
    income_source(conn, id)
}

/// Change a source's name, timing, months, estimate flag, or split (not its projection).
pub fn update_income_source(
    conn: &Connection,
    id: i64,
    input: &IncomeSourceInput,
) -> Result<IncomeSource> {
    let s = income_source(conn, id)?;
    let (name, timing, months) = validate_source_input(input)?;
    if s.is_other && name != s.name {
        return Err(AppError::invalid(
            "the built-in Other / Gifts source can't be renamed",
        ));
    }
    if source_name_taken(conn, s.plan_id, &name, Some(id))? {
        return Err(AppError::invalid(format!(
            "an income source named \"{name}\" already exists"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE income_sources SET name = ?2, timing = ?3, expected_months = ?4, is_estimate = ?5,
                                   split_current_twelfths = ?6
         WHERE id = ?1",
        params![
            id,
            name,
            timing,
            months_to_string(&months),
            input.is_estimate,
            input.split_current_twelfths
        ],
    )?;
    tx.execute(
        "UPDATE tag_rules SET income_source_name = ?2 WHERE income_source_name = ?1 COLLATE NOCASE",
        params![s.name, name],
    )?;
    tx.commit()?;
    income_source(conn, id)
}

/// Change a source's projection. With `confirm`, the amount is the actual (no longer an estimate).
pub fn set_income_projection(
    conn: &Connection,
    id: i64,
    new_projection: Cents,
    confirm: bool,
    reason: &str,
    today: NaiveDate,
) -> Result<IncomeSource> {
    check_amount(new_projection)?;
    let s = income_source(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE income_sources SET projected_cents = ?2, is_estimate = CASE WHEN ?3 THEN 0 ELSE is_estimate END
         WHERE id = ?1",
        params![id, new_projection, confirm],
    )?;
    if s.projected_cents != new_projection {
        let reason = if reason.trim().is_empty() && confirm {
            "Confirmed actual amount"
        } else {
            reason
        };
        log_if_tracking(
            &tx,
            s.plan_id,
            today,
            NewAdjustment {
                kind: "income_projection",
                income_source_id: Some(id),
                old_cents: Some(s.projected_cents),
                new_cents: Some(new_projection),
                reason,
                ..Default::default()
            },
        )?;
    }
    tx.commit()?;
    income_source(conn, id)
}

/// Remove an income source. Its transactions (if any) move to `move_to`.
pub fn remove_income_source(
    conn: &Connection,
    id: i64,
    move_to: Option<i64>,
    reason: &str,
    today: NaiveDate,
) -> Result<()> {
    let s = income_source(conn, id)?;
    if s.is_other {
        return Err(AppError::invalid(
            "the built-in Other / Gifts source can't be removed",
        ));
    }
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE income_source_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let target = match move_to {
        Some(t) => {
            let t = income_source(conn, t)?;
            if t.plan_id != s.plan_id || t.id == s.id {
                return Err(AppError::invalid(
                    "choose another income source in the same plan",
                ));
            }
            Some(t)
        }
        None if count > 0 => {
            return Err(AppError::invalid(
                "choose an income source to move this source's income into",
            ));
        }
        None => None,
    };
    let tx = conn.unchecked_transaction()?;
    log_if_tracking(
        &tx,
        s.plan_id,
        today,
        NewAdjustment {
            kind: "income_removed",
            income_source_id: Some(id),
            old_cents: Some(s.projected_cents),
            detail: format!("{count} transactions moved"),
            reason,
            ..Default::default()
        },
    )?;
    if let Some(t) = &target {
        tx.execute(
            "UPDATE transactions SET income_source_id = ?2 WHERE income_source_id = ?1",
            params![id, t.id],
        )?;
        tx.execute(
            "UPDATE tag_rules SET income_source_name = ?2 WHERE income_source_name = ?1 COLLATE NOCASE",
            params![s.name, t.name],
        )?;
    }
    tx.execute(
        "UPDATE income_sources SET removed_at = ?2 WHERE id = ?1",
        params![id, crate::db::now_timestamp()],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn set_savings_plan(
    conn: &Connection,
    plan_id: i64,
    holdback: Cents,
    draw: Cents,
    reason: &str,
    today: NaiveDate,
) -> Result<Plan> {
    check_amount(holdback)?;
    check_amount(draw)?;
    let p = plan(conn, plan_id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE plans SET planned_holdback_cents = ?2, planned_draw_cents = ?3 WHERE id = ?1",
        params![plan_id, holdback, draw],
    )?;
    for (kind, old, new) in [
        ("savings_holdback", p.planned_holdback_cents, holdback),
        ("savings_draw", p.planned_draw_cents, draw),
    ] {
        if old != new {
            log_if_tracking(
                &tx,
                plan_id,
                today,
                NewAdjustment {
                    kind,
                    old_cents: Some(old),
                    new_cents: Some(new),
                    reason,
                    ..Default::default()
                },
            )?;
        }
    }
    tx.commit()?;
    plan(conn, plan_id)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db;

    pub fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    pub fn steady(name: &str) -> IncomeSourceInput {
        IncomeSourceInput {
            name: name.into(),
            timing: "steady".into(),
            expected_months: vec![],
            is_estimate: false,
            split_current_twelfths: None,
        }
    }

    pub fn insert_expense(conn: &Connection, date: &str, category_id: i64, amount: Cents) -> i64 {
        conn.execute(
            "INSERT INTO transactions (date, description, display_name, amount_cents, type, category_id)
             VALUES (?1, 'TEST', 'Test', ?2, 'expense', ?3)",
            params![date, amount, category_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    pub fn insert_income(conn: &Connection, date: &str, source_id: i64, amount: Cents) {
        conn.execute(
            "INSERT INTO transactions (date, description, display_name, amount_cents, type, income_source_id)
             VALUES (?1, 'TEST INCOME', 'Test income', ?2, 'income', ?3)",
            params![date, amount, source_id],
        )
        .unwrap();
    }

    #[test]
    fn new_plan_has_builtins() {
        let conn = db::open_in_memory().unwrap();
        let p = create_plan(&conn, 2026, None).unwrap();
        let cats = categories(&conn, p.id).unwrap();
        assert_eq!(cats.len(), 1);
        assert!(cats[0].is_misc);
        let sources = income_sources(&conn, p.id).unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].is_other);
        assert!(create_plan(&conn, 2026, None).is_err());
    }

    #[test]
    fn plan_balances() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 1, 1);
        let p = create_plan(&conn, 2026, None).unwrap();
        add_income_source(&conn, p.id, &steady("Salary"), 7_800_000, today).unwrap();
        let bonus = IncomeSourceInput {
            name: "Bonus".into(),
            timing: "lumpy".into(),
            expected_months: vec![5],
            is_estimate: true,
            split_current_twelfths: Some(default_split_twelfths(5)),
        };
        add_income_source(&conn, p.id, &bonus, 1_200_000, today).unwrap();
        let p = set_savings_plan(&conn, p.id, 1_000_000, 0, "", today).unwrap();
        add_category(&conn, p.id, "Groceries", 600_000, false, today).unwrap();
        let t = totals(&conn, &p).unwrap();
        assert_eq!(t.projected_income_cents, 9_000_000);
        assert_eq!(t.auto_holdback_cents, 400_000); // 4/12 of the bonus
        assert_eq!(t.total_holdback_cents, 1_400_000);
        assert_eq!(t.available_cents, 7_600_000);
        assert_eq!(t.assigned_cents, 600_000);
        assert_eq!(t.unassigned_cents, 7_000_000);
    }

    #[test]
    fn split_income_carries_into_next_year() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 1, 1);
        let p26 = create_plan(&conn, 2026, None).unwrap();
        let bonus = IncomeSourceInput {
            name: "Bonus".into(),
            timing: "lumpy".into(),
            expected_months: vec![5],
            is_estimate: true,
            split_current_twelfths: Some(8),
        };
        let b = add_income_source(&conn, p26.id, &bonus, 1_200_000, today).unwrap();
        let p27 = create_plan(&conn, 2027, Some(2026)).unwrap();
        // Not yet received: carry-in uses the estimate
        assert_eq!(carried_in(&conn, 2027).unwrap(), 400_000);
        // Received a different amount: carry-in follows the actual
        insert_income(&conn, "2026-05-15", b.id, 1_500_000);
        assert_eq!(carried_in(&conn, 2027).unwrap(), 500_000);
        let t = totals(&conn, &p27).unwrap();
        assert_eq!(t.carried_in_cents, 500_000);
        assert_eq!(t.total_draw_cents, 500_000);
        // The copied bonus starts as an estimate
        let copied = income_sources(&conn, p27.id).unwrap();
        assert!(copied.iter().any(|s| s.name == "Bonus" && s.is_estimate));
    }

    #[test]
    fn changes_are_logged_only_once_tracking() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 3, 1);
        let p = create_plan(&conn, 2026, None).unwrap();
        let dining = add_category(&conn, p.id, "Dining", 300_000, false, today).unwrap();
        set_category_limit(&conn, dining.id, 320_000, "setup", today).unwrap();
        assert!(adjustments(&conn, p.id).unwrap().is_empty());

        insert_expense(&conn, "2026-02-01", dining.id, -5_000);
        set_category_limit(&conn, dining.id, 360_000, "eating out more", today).unwrap();
        let log = adjustments(&conn, p.id).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(
            (log[0].old_cents, log[0].new_cents),
            (Some(320_000), Some(360_000))
        );
        assert_eq!(log[0].reason, "eating out more");
        assert_eq!(
            last_limit_change(&conn, dining.id).unwrap().as_deref(),
            Some("2026-03-01")
        );
    }

    #[test]
    fn reallocation_is_one_entry_and_keeps_total() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 7, 1);
        let p = create_plan(&conn, 2026, None).unwrap();
        let transport = add_category(&conn, p.id, "Transport", 250_000, false, today).unwrap();
        let dining = add_category(&conn, p.id, "Dining", 300_000, false, today).unwrap();
        insert_expense(&conn, "2026-06-01", dining.id, -1_000);
        let before = totals(&conn, &plan(&conn, p.id).unwrap())
            .unwrap()
            .assigned_cents;
        reallocate(
            &conn,
            transport.id,
            dining.id,
            100_000,
            "more dining",
            today,
        )
        .unwrap();
        assert!(reallocate(&conn, transport.id, dining.id, 500_000, "", today).is_err());
        let after = totals(&conn, &plan(&conn, p.id).unwrap())
            .unwrap()
            .assigned_cents;
        assert_eq!(before, after);
        assert_eq!(
            category(&conn, dining.id).unwrap().annual_limit_cents,
            400_000
        );
        let log = adjustments(&conn, p.id).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].kind, "reallocation");
        assert_eq!(log[0].category_name.as_deref(), Some("Transport"));
        assert_eq!(log[0].to_category_name.as_deref(), Some("Dining"));
    }

    #[test]
    fn removing_a_category_merges_it() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 7, 1);
        let p = create_plan(&conn, 2026, None).unwrap();
        let coffee = add_category(&conn, p.id, "Coffee", 60_000, false, today).unwrap();
        let dining = add_category(&conn, p.id, "Dining", 300_000, false, today).unwrap();
        let t1 = insert_expense(&conn, "2026-06-01", coffee.id, -525);
        insert_expense(&conn, "2026-06-02", coffee.id, -525);
        conn.execute(
            "INSERT INTO tag_rules (merchant_key, type, category_name) VALUES ('example coffee', 'expense', 'Coffee')",
            [],
        )
        .unwrap();

        assert!(remove_category(&conn, coffee.id, None, true, "", today).is_err());
        let misc = misc_category(&conn, p.id).unwrap();
        assert!(remove_category(&conn, misc.id, Some(dining.id), true, "", today).is_err());

        let moved = remove_category(
            &conn,
            coffee.id,
            Some(dining.id),
            true,
            "fold into dining",
            today,
        )
        .unwrap();
        assert_eq!(moved, 2);
        assert_eq!(
            category(&conn, dining.id).unwrap().annual_limit_cents,
            360_000
        );
        let cat_of_t1: i64 = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE id = ?1",
                [t1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cat_of_t1, dining.id);
        let rule: String = conn
            .query_row("SELECT category_name FROM tag_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rule, "Dining");
        assert!(
            categories(&conn, p.id)
                .unwrap()
                .iter()
                .all(|c| c.name != "Coffee")
        );
        // Name can be reused after removal
        add_category(&conn, p.id, "Coffee", 0, false, today).unwrap();
        let log = adjustments(&conn, p.id).unwrap();
        assert!(
            log.iter()
                .any(|a| a.kind == "category_removed" && a.amount_cents == Some(60_000))
        );
    }

    #[test]
    fn confirming_income_clears_estimate() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 5, 20);
        let p = create_plan(&conn, 2026, None).unwrap();
        let mut input = steady("Bonus");
        input.is_estimate = true;
        let b = add_income_source(&conn, p.id, &input, 1_200_000, today).unwrap();
        insert_income(&conn, "2026-05-15", b.id, 1_120_000);
        let b = set_income_projection(&conn, b.id, 1_120_000, true, "", today).unwrap();
        assert!(!b.is_estimate);
        let log = adjustments(&conn, p.id).unwrap();
        assert_eq!(log[0].reason, "Confirmed actual amount");
    }

    #[test]
    fn default_split() {
        assert_eq!(default_split_twelfths(3), 10);
        assert_eq!(default_split_twelfths(5), 8);
        assert_eq!(default_split_twelfths(1), 12);
    }
}
