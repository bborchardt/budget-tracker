//! Suggesting tags: learned rules first, then built-in patterns, then Misc / Other / Gifts.

use crate::error::Result;
use crate::money::Cents;
use crate::plan;
use crate::transactions::Tag;
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub tag: Tag,
    /// Guessed (fallback): shown with the quiet "auto" marker
    pub auto: bool,
    /// "learned", "pattern", "fallback", or "pending" (copied from the pending row it replaces)
    pub reason: String,
}

#[derive(Debug, Default)]
struct PlanLookup {
    categories: HashMap<String, i64>,
    misc_id: i64,
    sources: HashMap<String, i64>,
    other_id: i64,
    /// The largest steady source (usually salary), for payroll deposits
    main_source_id: Option<i64>,
}

#[derive(Debug)]
struct Rule {
    kind: String,
    category_name: Option<String>,
    income_source_name: Option<String>,
    savings_account_id: Option<i64>,
}

#[derive(Debug, Default)]
pub struct Context {
    plans: HashMap<i32, PlanLookup>,
    rules: HashMap<String, Rule>,
    savings: Vec<(i64, String)>,
}

impl Context {
    pub fn load(conn: &Connection, years: impl IntoIterator<Item = i32>) -> Result<Context> {
        let mut ctx = Context::default();
        for year in years {
            if ctx.plans.contains_key(&year) {
                continue;
            }
            let Some(p) = plan::plan_for_year(conn, year)? else {
                continue;
            };
            let mut lookup = PlanLookup::default();
            for c in plan::categories(conn, p.id)? {
                if c.is_misc {
                    lookup.misc_id = c.id;
                }
                lookup.categories.insert(c.name.to_lowercase(), c.id);
            }
            let sources = plan::income_sources(conn, p.id)?;
            for s in &sources {
                if s.is_other {
                    lookup.other_id = s.id;
                }
                lookup.sources.insert(s.name.to_lowercase(), s.id);
            }
            lookup.main_source_id = sources
                .iter()
                .filter(|s| {
                    !s.is_other && s.timing == "steady" && s.split_current_twelfths.is_none()
                })
                .max_by_key(|s| s.projected_cents)
                .map(|s| s.id);
            ctx.plans.insert(year, lookup);
        }
        let mut stmt = conn.prepare(
            "SELECT merchant_key, type, category_name, income_source_name, savings_account_id FROM tag_rules",
        )?;
        let rules = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Rule {
                    kind: r.get(1)?,
                    category_name: r.get(2)?,
                    income_source_name: r.get(3)?,
                    savings_account_id: r.get(4)?,
                },
            ))
        })?;
        for r in rules {
            let (k, rule) = r?;
            ctx.rules.insert(k, rule);
        }
        ctx.savings = crate::savings::list_accounts(conn)?
            .into_iter()
            .filter(|a| !a.archived)
            .map(|a| (a.id, a.name.to_lowercase()))
            .collect();
        Ok(ctx)
    }

    pub fn has_plan(&self, year: i32) -> bool {
        self.plans.contains_key(&year)
    }
}

const TRANSFER_PATTERNS: &[&str] = &[
    "payment thank you",
    "payment - thank you",
    "payment received",
    "autopay",
    "auto pay",
    "auto-pay",
    "online payment",
    "mobile payment",
    "credit card payment",
    "card payment",
    "epay",
    "online transfer",
    "transfer to",
    "transfer from",
    "internal transfer",
    "xfer",
];

const PAYROLL_PATTERNS: &[&str] = &["payroll", "direct dep", "dir dep", "salary"];

fn transfer_account(ctx: &Context, lower: &str) -> Option<i64> {
    ctx.savings
        .iter()
        .find(|(_, name)| !name.is_empty() && lower.contains(name.as_str()))
        .map(|(id, _)| *id)
}

/// Suggest a tag, or None when the row's year has no plan (and it isn't a transfer).
pub fn suggest(
    ctx: &Context,
    date: NaiveDate,
    description: &str,
    merchant_key: &str,
    amount_cents: Cents,
    source_kind: &str,
) -> Option<Suggestion> {
    let lower = description.to_lowercase();
    let lookup = ctx.plans.get(&date.year());

    // 1. Learned from the user's own tagging
    if let Some(rule) = ctx.rules.get(merchant_key) {
        let tag = match rule.kind.as_str() {
            "transfer" => Some(Tag::transfer(rule.savings_account_id)),
            "expense" => lookup.and_then(|l| {
                rule.category_name
                    .as_ref()
                    .and_then(|n| l.categories.get(&n.to_lowercase()))
                    .map(|&id| Tag::expense(id))
            }),
            "income" => lookup.and_then(|l| {
                rule.income_source_name
                    .as_ref()
                    .and_then(|n| l.sources.get(&n.to_lowercase()))
                    .map(|&id| Tag::income(id))
            }),
            _ => None,
        };
        if let Some(tag) = tag {
            return Some(Suggestion {
                tag,
                auto: false,
                reason: "learned".into(),
            });
        }
    }

    // 2. Built-in patterns
    if TRANSFER_PATTERNS.iter().any(|p| lower.contains(p)) {
        return Some(Suggestion {
            tag: Tag::transfer(transfer_account(ctx, &lower)),
            auto: false,
            reason: "pattern".into(),
        });
    }
    let lookup = lookup?;
    if amount_cents > 0 && PAYROLL_PATTERNS.iter().any(|p| lower.contains(p)) {
        let id = lookup.main_source_id.unwrap_or(lookup.other_id);
        return Some(Suggestion {
            tag: Tag::income(id),
            auto: true,
            reason: "pattern".into(),
        });
    }

    // 3. Fallback: spending (and card refunds) to Misc, other money in to Other / Gifts
    let tag = if amount_cents < 0 || source_kind == "card" {
        Tag::expense(lookup.misc_id)
    } else {
        Tag::income(lookup.other_id)
    };
    Some(Suggestion {
        tag,
        auto: true,
        reason: "fallback".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::plan::tests::{d, steady};

    #[test]
    fn learned_then_patterns_then_fallback() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 1, 1);
        let p = plan::create_plan(&conn, 2026, None).unwrap();
        let dining = plan::add_category(&conn, p.id, "Dining", 300_000, false, today).unwrap();
        let salary =
            plan::add_income_source(&conn, p.id, &steady("Salary"), 7_000_000, today).unwrap();
        let sav = crate::savings::add_account(&conn, "Savings 1", 0, today).unwrap();
        conn.execute(
            "INSERT INTO tag_rules (merchant_key, type, category_name) VALUES ('example coffee', 'expense', 'dining')",
            [],
        )
        .unwrap();
        let ctx = Context::load(&conn, [2026]).unwrap();
        let misc = plan::misc_category(&conn, p.id).unwrap();
        let other = plan::other_income_source(&conn, p.id).unwrap();
        let date = d(2026, 6, 1);

        let s = suggest(
            &ctx,
            date,
            "SQ *EXAMPLE COFFEE",
            "example coffee",
            -525,
            "card",
        )
        .unwrap();
        assert_eq!((s.tag, s.auto), (Tag::expense(dining.id), false));
        // Refund at a learned merchant goes back to the same category
        let s = suggest(
            &ctx,
            date,
            "SQ *EXAMPLE COFFEE",
            "example coffee",
            525,
            "card",
        )
        .unwrap();
        assert_eq!(s.tag, Tag::expense(dining.id));

        let s = suggest(
            &ctx,
            date,
            "PAYMENT THANK YOU",
            "payment thank you",
            50_000,
            "card",
        )
        .unwrap();
        assert_eq!(s.tag, Tag::transfer(None));
        let s = suggest(
            &ctx,
            date,
            "ONLINE TRANSFER TO SAVINGS 1",
            "online transfer to savings",
            -50_000,
            "checking",
        )
        .unwrap();
        assert_eq!(s.tag, Tag::transfer(Some(sav.id)));
        let s = suggest(
            &ctx,
            date,
            "TEST EMPLOYER PAYROLL",
            "test employer payroll",
            250_000,
            "checking",
        )
        .unwrap();
        assert_eq!((s.tag, s.auto), (Tag::income(salary.id), true));

        let s = suggest(&ctx, date, "NEW PLACE", "new place", -1_000, "card").unwrap();
        assert_eq!((s.tag, s.auto), (Tag::expense(misc.id), true));
        let s = suggest(
            &ctx,
            date,
            "TEST STORE RETURN",
            "test store return",
            1_000,
            "card",
        )
        .unwrap();
        assert_eq!(s.tag, Tag::expense(misc.id));
        let s = suggest(
            &ctx,
            date,
            "MOBILE DEPOSIT",
            "mobile deposit",
            10_000,
            "checking",
        )
        .unwrap();
        assert_eq!(s.tag, Tag::income(other.id));

        // No plan for the year: only transfers can be suggested
        assert!(
            suggest(
                &ctx,
                d(2025, 6, 1),
                "NEW PLACE",
                "new place",
                -1_000,
                "card"
            )
            .is_none()
        );
        assert!(
            suggest(
                &ctx,
                d(2025, 6, 1),
                "AUTOPAY",
                "autopay",
                -1_000,
                "checking"
            )
            .is_some()
        );
    }
}
