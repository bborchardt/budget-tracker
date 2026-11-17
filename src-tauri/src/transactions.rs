//! Transactions: listing, manual entry, editing, tagging, and learned tag rules.

use crate::error::{AppError, Result};
use crate::import::clean;
use crate::money::Cents;
use crate::plan;
use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};

/// What a transaction is tagged as. Exactly one target applies per type:
/// expense -> category, income -> income source, transfer -> optional savings account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub category_id: Option<i64>,
    #[serde(default)]
    pub income_source_id: Option<i64>,
    #[serde(default)]
    pub savings_account_id: Option<i64>,
}

impl Tag {
    pub fn expense(category_id: i64) -> Tag {
        Tag {
            kind: "expense".into(),
            category_id: Some(category_id),
            income_source_id: None,
            savings_account_id: None,
        }
    }
    pub fn income(source_id: i64) -> Tag {
        Tag {
            kind: "income".into(),
            category_id: None,
            income_source_id: Some(source_id),
            savings_account_id: None,
        }
    }
    pub fn transfer(savings_account_id: Option<i64>) -> Tag {
        Tag {
            kind: "transfer".into(),
            category_id: None,
            income_source_id: None,
            savings_account_id,
        }
    }
}

/// Check that a tag is complete and that its category / income source belongs to the plan
/// for the transaction's year. Returns the normalized tag.
pub fn validate_tag(conn: &Connection, tag: &Tag, date: NaiveDate) -> Result<Tag> {
    match tag.kind.as_str() {
        "expense" => {
            let id = tag
                .category_id
                .ok_or_else(|| AppError::invalid("choose a category"))?;
            let c = plan::category(conn, id)?;
            let p = plan::plan(conn, c.plan_id)?;
            if p.year != date.year() {
                return Err(AppError::invalid(format!(
                    "that category belongs to the {} plan",
                    p.year
                )));
            }
            Ok(Tag::expense(id))
        }
        "income" => {
            let id = tag
                .income_source_id
                .ok_or_else(|| AppError::invalid("choose an income source"))?;
            let s = plan::income_source(conn, id)?;
            let p = plan::plan(conn, s.plan_id)?;
            if p.year != date.year() {
                return Err(AppError::invalid(format!(
                    "that income source belongs to the {} plan",
                    p.year
                )));
            }
            Ok(Tag::income(id))
        }
        "transfer" => {
            if let Some(id) = tag.savings_account_id {
                crate::savings::account(conn, id)?;
            }
            Ok(Tag::transfer(tag.savings_account_id))
        }
        _ => Err(AppError::invalid(
            "type must be expense, income, or transfer",
        )),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionView {
    pub id: i64,
    pub date: String,
    pub description: String,
    pub display_name: String,
    pub amount_cents: Cents,
    #[serde(rename = "type")]
    pub kind: String,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub income_source_id: Option<i64>,
    pub income_source_name: Option<String>,
    pub savings_account_id: Option<i64>,
    pub savings_account_name: Option<String>,
    pub source_id: Option<i64>,
    pub source_name: Option<String>,
    pub is_pending: bool,
    pub is_auto: bool,
    pub notes: String,
    pub import_batch_id: Option<i64>,
}

const VIEW_SELECT: &str = "
    SELECT t.id, t.date, t.description, t.display_name, t.amount_cents, t.type,
           t.category_id, c.name, t.income_source_id, s.name, t.savings_account_id, a.name,
           t.source_id, src.name, t.is_pending, t.is_auto, t.notes, t.import_batch_id
    FROM transactions t
    LEFT JOIN categories c ON c.id = t.category_id
    LEFT JOIN income_sources s ON s.id = t.income_source_id
    LEFT JOIN savings_accounts a ON a.id = t.savings_account_id
    LEFT JOIN import_sources src ON src.id = t.source_id";

fn view_from_row(r: &rusqlite::Row) -> rusqlite::Result<TransactionView> {
    Ok(TransactionView {
        id: r.get(0)?,
        date: r.get(1)?,
        description: r.get(2)?,
        display_name: r.get(3)?,
        amount_cents: r.get(4)?,
        kind: r.get(5)?,
        category_id: r.get(6)?,
        category_name: r.get(7)?,
        income_source_id: r.get(8)?,
        income_source_name: r.get(9)?,
        savings_account_id: r.get(10)?,
        savings_account_name: r.get(11)?,
        source_id: r.get(12)?,
        source_name: r.get(13)?,
        is_pending: r.get(14)?,
        is_auto: r.get(15)?,
        notes: r.get(16)?,
        import_batch_id: r.get(17)?,
    })
}

pub fn get(conn: &Connection, id: i64) -> Result<TransactionView> {
    conn.query_row(
        &format!("{VIEW_SELECT} WHERE t.id = ?1 AND t.deleted_at IS NULL"),
        [id],
        view_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("transaction"))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub year: Option<i32>,
    pub category_id: Option<i64>,
    pub income_source_id: Option<i64>,
    pub source_id: Option<i64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub keyword: Option<String>,
    pub auto_only: bool,
    pub pending_only: bool,
    /// "date" (default), "amount", "category", "type"
    pub sort: Option<String>,
    pub ascending: bool,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub rows: Vec<TransactionView>,
    pub total: i64,
}

pub fn list(conn: &Connection, f: &Filter) -> Result<Page> {
    let mut clauses = vec!["t.deleted_at IS NULL".to_string()];
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    let mut push = |clause: &str, arg: Box<dyn ToSql>| {
        args.push(arg);
        clauses.push(clause.replace('?', &format!("?{}", args.len())));
    };
    if let Some(y) = f.year {
        push("t.date >= ?", Box::new(format!("{y}-01-01")));
        push("t.date <= ?", Box::new(format!("{y}-12-31")));
    }
    if let Some(v) = &f.from {
        push("t.date >= ?", Box::new(v.clone()));
    }
    if let Some(v) = &f.to {
        push("t.date <= ?", Box::new(v.clone()));
    }
    if let Some(v) = f.category_id {
        push("t.category_id = ?", Box::new(v));
    }
    if let Some(v) = f.income_source_id {
        push("t.income_source_id = ?", Box::new(v));
    }
    if let Some(v) = f.source_id {
        push("t.source_id = ?", Box::new(v));
    }
    if let Some(v) = &f.kind {
        push("t.type = ?", Box::new(v.clone()));
    }
    if let Some(k) = f
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        let pattern = format!(
            "%{}%",
            k.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        args.push(Box::new(pattern));
        let n = args.len();
        clauses.push(format!(
            "(t.display_name LIKE ?{n} ESCAPE '\\' OR t.description LIKE ?{n} ESCAPE '\\' OR t.notes LIKE ?{n} ESCAPE '\\')"
        ));
    }
    if f.auto_only {
        clauses.push("t.is_auto = 1".into());
    }
    if f.pending_only {
        clauses.push("t.is_pending = 1".into());
    }
    let where_sql = clauses.join(" AND ");
    let dir = if f.ascending { "ASC" } else { "DESC" };
    let order = match f.sort.as_deref() {
        Some("amount") => format!("t.amount_cents {dir}, t.date DESC"),
        Some("category") => {
            format!("COALESCE(c.name, s.name, a.name, '') COLLATE NOCASE {dir}, t.date DESC")
        }
        Some("type") => format!("t.type {dir}, t.date DESC"),
        _ => format!("t.date {dir}, t.id {dir}"),
    };
    let params_ref: Vec<&dyn ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let total: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM transactions t
             LEFT JOIN categories c ON c.id = t.category_id WHERE {where_sql}"
        ),
        params_ref.as_slice(),
        |r| r.get(0),
    )?;
    let limit = f.limit.unwrap_or(500).clamp(1, 10_000);
    let offset = f.offset.unwrap_or(0).max(0);
    let mut stmt = conn.prepare(&format!(
        "{VIEW_SELECT} WHERE {where_sql} ORDER BY {order} LIMIT {limit} OFFSET {offset}"
    ))?;
    let rows = stmt
        .query_map(params_ref.as_slice(), view_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Page { rows, total })
}

/// A row to insert (used by manual entry and the importer).
pub struct NewRow<'a> {
    pub date: NaiveDate,
    pub description: &'a str,
    pub amount_cents: Cents,
    pub tag: &'a Tag,
    pub source_id: Option<i64>,
    pub is_pending: bool,
    pub is_auto: bool,
    pub notes: &'a str,
    pub import_batch_id: Option<i64>,
}

pub fn insert(conn: &Connection, row: &NewRow) -> Result<i64> {
    let description = row.description.trim();
    if description.is_empty() {
        return Err(AppError::invalid("description can't be empty"));
    }
    conn.execute(
        "INSERT INTO transactions (date, description, display_name, merchant_key, amount_cents, type,
                                   category_id, income_source_id, savings_account_id, source_id,
                                   is_pending, is_auto, notes, import_batch_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            row.date.to_string(),
            description,
            clean::display_name(description),
            clean::merchant_key(description),
            row.amount_cents,
            row.tag.kind,
            row.tag.category_id,
            row.tag.income_source_id,
            row.tag.savings_account_id,
            row.source_id,
            row.is_pending,
            row.is_auto,
            row.notes.trim(),
            row.import_batch_id
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewTransaction {
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: Cents,
    pub tag: Tag,
    #[serde(default)]
    pub notes: String,
}

pub fn create(conn: &Connection, t: &NewTransaction) -> Result<TransactionView> {
    let tag = validate_tag(conn, &t.tag, t.date)?;
    let id = insert(
        conn,
        &NewRow {
            date: t.date,
            description: &t.description,
            amount_cents: t.amount_cents,
            tag: &tag,
            source_id: None,
            is_pending: false,
            is_auto: false,
            notes: &t.notes,
            import_batch_id: None,
        },
    )?;
    get(conn, id)
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionUpdate {
    pub date: NaiveDate,
    pub display_name: String,
    pub amount_cents: Cents,
    pub tag: Tag,
    #[serde(default)]
    pub notes: String,
    /// Remember this tag for the merchant next time
    #[serde(default)]
    pub learn: bool,
}

pub fn update(conn: &Connection, id: i64, u: &TransactionUpdate) -> Result<TransactionView> {
    let current = get(conn, id)?;
    let tag = validate_tag(conn, &u.tag, u.date)?;
    let display = u.display_name.trim();
    let display = if display.is_empty() {
        current.display_name.as_str()
    } else {
        display
    };
    let tag_changed = tag.kind != current.kind
        || tag.category_id != current.category_id
        || tag.income_source_id != current.income_source_id
        || tag.savings_account_id != current.savings_account_id;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE transactions SET date = ?2, display_name = ?3, amount_cents = ?4, type = ?5, category_id = ?6,
                                 income_source_id = ?7, savings_account_id = ?8, notes = ?9,
                                 is_auto = CASE WHEN ?10 THEN 0 ELSE is_auto END
         WHERE id = ?1",
        params![
            id,
            u.date.to_string(),
            display,
            u.amount_cents,
            tag.kind,
            tag.category_id,
            tag.income_source_id,
            tag.savings_account_id,
            u.notes.trim(),
            tag_changed
        ],
    )?;
    if u.learn && tag_changed {
        let key: String = tx.query_row(
            "SELECT merchant_key FROM transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?;
        learn(&tx, &key, &tag)?;
    }
    tx.commit()?;
    get(conn, id)
}

/// Tag several transactions at once (each must be in the year its tag belongs to).
pub fn set_tag_bulk(conn: &Connection, ids: &[i64], tag: &Tag, learn_rule: bool) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    for &id in ids {
        let t = get(&tx, id)?;
        let date =
            crate::calendar::parse_iso(&t.date).ok_or_else(|| AppError::invalid("bad date"))?;
        let tag = validate_tag(&tx, tag, date)?;
        tx.execute(
            "UPDATE transactions SET type = ?2, category_id = ?3, income_source_id = ?4, savings_account_id = ?5,
                                     is_auto = 0
             WHERE id = ?1",
            params![id, tag.kind, tag.category_id, tag.income_source_id, tag.savings_account_id],
        )?;
        if learn_rule {
            let key: String = tx.query_row(
                "SELECT merchant_key FROM transactions WHERE id = ?1",
                [id],
                |r| r.get(0),
            )?;
            learn(&tx, &key, &tag)?;
        }
    }
    tx.commit()?;
    Ok(ids.len())
}

/// Delete transactions. They're kept (hidden) so a later import doesn't bring them back.
pub fn delete(conn: &Connection, ids: &[i64]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let now = crate::db::now_timestamp();
    let mut n = 0;
    for &id in ids {
        n += tx.execute(
            "UPDATE transactions SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, now],
        )?;
    }
    tx.commit()?;
    Ok(n)
}

/// Remember `tag` for a merchant. Categories and income sources are stored by name so the
/// rule applies in other years' plans too.
pub fn learn(conn: &Connection, merchant_key: &str, tag: &Tag) -> Result<()> {
    if merchant_key.trim().is_empty() {
        return Ok(());
    }
    let category_name = match tag.category_id {
        Some(id) => Some(plan::category(conn, id)?.name),
        None => None,
    };
    let source_name = match tag.income_source_id {
        Some(id) => Some(plan::income_source(conn, id)?.name),
        None => None,
    };
    conn.execute(
        "INSERT INTO tag_rules (merchant_key, type, category_name, income_source_name, savings_account_id, times_used)
         VALUES (?1, ?2, ?3, ?4, ?5, 1)
         ON CONFLICT(merchant_key) DO UPDATE SET
             type = excluded.type, category_name = excluded.category_name,
             income_source_name = excluded.income_source_name, savings_account_id = excluded.savings_account_id,
             times_used = times_used + 1, updated_at = datetime('now')",
        params![merchant_key, tag.kind, category_name, source_name, tag.savings_account_id],
    )?;
    Ok(())
}

/// Net spending per category (purchases minus refunds) for a plan's year, positive = spent.
pub fn spending_by_category(
    conn: &Connection,
    plan_id: i64,
) -> Result<std::collections::HashMap<i64, Cents>> {
    let mut stmt = conn.prepare(
        "SELECT t.category_id, -SUM(t.amount_cents) FROM transactions t
         JOIN categories c ON c.id = t.category_id
         WHERE c.plan_id = ?1 AND t.type = 'expense' AND t.deleted_at IS NULL
         GROUP BY t.category_id",
    )?;
    let rows = stmt
        .query_map([plan_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::plan::tests::d;

    #[test]
    fn tags_must_match_the_plan_year() {
        let conn = db::open_in_memory().unwrap();
        let p26 = plan::create_plan(&conn, 2026, None).unwrap();
        plan::create_plan(&conn, 2027, None).unwrap();
        let misc26 = plan::misc_category(&conn, p26.id).unwrap();
        let t = NewTransaction {
            date: d(2027, 1, 5),
            description: "EXAMPLE GROCERY #1234".into(),
            amount_cents: -4_210,
            tag: Tag::expense(misc26.id),
            notes: String::new(),
        };
        assert!(create(&conn, &t).is_err());
        let ok = create(
            &conn,
            &NewTransaction {
                date: d(2026, 6, 5),
                ..t
            },
        )
        .unwrap();
        assert_eq!(ok.display_name, "Example Grocery");
        assert_eq!(ok.category_name.as_deref(), Some("Misc"));
    }

    #[test]
    fn editing_a_tag_learns_and_clears_auto() {
        let conn = db::open_in_memory().unwrap();
        let today = d(2026, 6, 1);
        let p = plan::create_plan(&conn, 2026, None).unwrap();
        let dining = plan::add_category(&conn, p.id, "Dining", 300_000, false, today).unwrap();
        let misc = plan::misc_category(&conn, p.id).unwrap();
        let id = insert(
            &conn,
            &NewRow {
                date: d(2026, 6, 1),
                description: "SQ *EXAMPLE COFFEE 0423",
                amount_cents: -525,
                tag: &Tag::expense(misc.id),
                source_id: None,
                is_pending: false,
                is_auto: true,
                notes: "",
                import_batch_id: None,
            },
        )
        .unwrap();
        let t = update(
            &conn,
            id,
            &TransactionUpdate {
                date: d(2026, 6, 1),
                display_name: String::new(),
                amount_cents: -525,
                tag: Tag::expense(dining.id),
                notes: String::new(),
                learn: true,
            },
        )
        .unwrap();
        assert!(!t.is_auto);
        let (key, cat): (String, String) = conn
            .query_row(
                "SELECT merchant_key, category_name FROM tag_rules",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((key.as_str(), cat.as_str()), ("example coffee", "Dining"));
    }

    #[test]
    fn list_filters_and_sorts() {
        let conn = db::open_in_memory().unwrap();
        let p = plan::create_plan(&conn, 2026, None).unwrap();
        let misc = plan::misc_category(&conn, p.id).unwrap();
        for (day, desc, amt) in [
            (1, "EXAMPLE GROCERY", -4_000),
            (2, "TEST HARDWARE 50%_OFF", -9_000),
            (3, "EXAMPLE GROCERY", 1_000),
        ] {
            create(
                &conn,
                &NewTransaction {
                    date: d(2026, 3, day),
                    description: desc.into(),
                    amount_cents: amt,
                    tag: Tag::expense(misc.id),
                    notes: String::new(),
                },
            )
            .unwrap();
        }
        let page = list(
            &conn,
            &Filter {
                keyword: Some("grocery".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0].date, "2026-03-03");
        let page = list(
            &conn,
            &Filter {
                keyword: Some("50%_".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        let page = list(
            &conn,
            &Filter {
                sort: Some("amount".into()),
                ascending: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.rows[0].amount_cents, -9_000);
        delete(&conn, &[page.rows[0].id]).unwrap();
        assert_eq!(list(&conn, &Filter::default()).unwrap().total, 2);
        assert_eq!(spending_by_category(&conn, p.id).unwrap()[&misc.id], 3_000);
    }
}
