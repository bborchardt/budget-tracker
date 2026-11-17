//! The weekly import: sources, preview, commit, and undo.
//!
//! Preview never writes. Commit writes one batch (any number of sources) that can be undone
//! as a unit: inserted rows are deleted and anything the batch replaced or removed comes back.

pub mod clean;
pub mod matching;
pub mod parse;
pub mod tagging;

use crate::calendar;
use crate::db::now_timestamp;
use crate::error::{AppError, Result};
use crate::money::{Cents, Marker};
use crate::transactions::{self, NewRow, Tag};
use chrono::{Datelike, Duration, NaiveDate};
use matching::{Existing, Incoming, MatchStatus};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tagging::Suggestion;

// ---------- sources ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceFormat {
    /// The source shows purchases as positive numbers (common for credit cards)
    #[serde(default)]
    pub purchases_positive: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportSource {
    pub id: i64,
    pub name: String,
    /// "card" or "checking"
    pub kind: String,
    pub format: SourceFormat,
}

fn source_from_row(r: &rusqlite::Row) -> rusqlite::Result<ImportSource> {
    let json: String = r.get(3)?;
    Ok(ImportSource {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        format: serde_json::from_str(&json).unwrap_or_default(),
    })
}

pub fn list_sources(conn: &Connection) -> Result<Vec<ImportSource>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, format_json FROM import_sources ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], source_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn source(conn: &Connection, id: i64) -> Result<ImportSource> {
    conn.query_row(
        "SELECT id, name, kind, format_json FROM import_sources WHERE id = ?1",
        [id],
        source_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("import source"))
}

fn check_kind(kind: &str) -> Result<()> {
    if !matches!(kind, "card" | "checking") {
        return Err(AppError::invalid("kind must be card or checking"));
    }
    Ok(())
}

pub fn add_source(conn: &Connection, name: &str, kind: &str) -> Result<ImportSource> {
    check_kind(kind)?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AppError::invalid("name can't be empty"));
    }
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM import_sources WHERE name = ?1 COLLATE NOCASE)",
        [&name],
        |r| r.get(0),
    )?;
    if taken {
        return Err(AppError::invalid(format!(
            "a source named \"{name}\" already exists"
        )));
    }
    conn.execute(
        "INSERT INTO import_sources (name, kind) VALUES (?1, ?2)",
        params![name, kind],
    )?;
    source(conn, conn.last_insert_rowid())
}

pub fn update_source(conn: &Connection, id: i64, name: &str, kind: &str) -> Result<ImportSource> {
    check_kind(kind)?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AppError::invalid("name can't be empty"));
    }
    conn.execute(
        "UPDATE import_sources SET name = ?2, kind = ?3 WHERE id = ?1",
        params![id, name, kind],
    )?;
    source(conn, id)
}

fn save_format(conn: &Connection, id: i64, format: &SourceFormat) -> Result<()> {
    conn.execute(
        "UPDATE import_sources SET format_json = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(format)?],
    )?;
    Ok(())
}

// ---------- preview ----------

#[derive(Debug, Clone, Serialize)]
pub struct ExistingSummary {
    pub id: i64,
    pub date: String,
    pub display_name: String,
    pub amount_cents: Cents,
    pub is_pending: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewRow {
    pub line: usize,
    pub date: NaiveDate,
    pub description: String,
    pub display_name: String,
    pub merchant_key: String,
    /// Normalized: negative = money out
    pub amount_cents: Cents,
    pub pending: bool,
    pub status: MatchStatus,
    pub existing: Option<ExistingSummary>,
    pub suggestion: Option<Suggestion>,
    /// Why this row can't be imported as-is (e.g. no plan for its year)
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub source_id: i64,
    pub layout: String,
    pub skipped_lines: usize,
    /// The sign convention used. Sent back on commit so the source remembers it.
    pub purchases_positive: bool,
    /// True when the sign convention was guessed from this input rather than remembered
    pub sign_detected: bool,
    pub rows: Vec<PreviewRow>,
}

/// Decide whether this source shows purchases as positive numbers. Remembered per source;
/// otherwise guessed: if most plain amounts are positive, they're purchases.
fn purchases_positive(
    format: &SourceFormat,
    rows: &[parse::ParsedRow],
    override_: Option<bool>,
) -> (bool, bool) {
    if let Some(v) = override_ {
        return (v, false);
    }
    if let Some(v) = format.purchases_positive {
        return (v, false);
    }
    let plain: Vec<&parse::ParsedRow> = rows
        .iter()
        .filter(|r| r.marker.is_none() && !r.from_debit_credit)
        .collect();
    if plain.is_empty() {
        return (false, true);
    }
    let positive = plain.iter().filter(|r| r.amount_cents > 0).count();
    (positive as f64 / plain.len() as f64 > 0.6, true)
}

fn normalize(row: &parse::ParsedRow, purchases_positive: bool) -> Cents {
    match row.marker {
        Some(Marker::Credit) => row.amount_cents.abs(),
        Some(Marker::Debit) => -row.amount_cents.abs(),
        None if row.from_debit_credit => row.amount_cents,
        None if purchases_positive => -row.amount_cents,
        None => row.amount_cents,
    }
}

fn existing_for_source(
    conn: &Connection,
    source_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<Existing>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, amount_cents, merchant_key, description, is_pending, deleted_at IS NOT NULL
         FROM transactions
         WHERE source_id = ?1 AND date >= ?2 AND date <= ?3
           AND (deleted_at IS NULL OR (deleted_by_batch_id IS NULL AND replaced_by_id IS NULL))
         ORDER BY date, id",
    )?;
    let rows = stmt
        .query_map(params![source_id, from.to_string(), to.to_string()], |r| {
            let date: String = r.get(1)?;
            Ok(Existing {
                id: r.get(0)?,
                date: calendar::parse_iso(&date).unwrap_or_default(),
                amount_cents: r.get(2)?,
                merchant_key: r.get(3)?,
                description: r.get(4)?,
                pending: r.get(5)?,
                user_deleted: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn existing_summary(conn: &Connection, id: i64) -> Result<Option<ExistingSummary>> {
    Ok(conn
        .query_row(
            "SELECT id, date, display_name, amount_cents, is_pending FROM transactions WHERE id = ?1",
            [id],
            |r| {
                Ok(ExistingSummary {
                    id: r.get(0)?,
                    date: r.get(1)?,
                    display_name: r.get(2)?,
                    amount_cents: r.get(3)?,
                    is_pending: r.get(4)?,
                })
            },
        )
        .optional()?)
}

fn tag_of(conn: &Connection, id: i64) -> Result<(Tag, bool)> {
    Ok(conn.query_row(
        "SELECT type, category_id, income_source_id, savings_account_id, is_auto FROM transactions WHERE id = ?1",
        [id],
        |r| {
            Ok((
                Tag {
                    kind: r.get(0)?,
                    category_id: r.get(1)?,
                    income_source_id: r.get(2)?,
                    savings_account_id: r.get(3)?,
                },
                r.get(4)?,
            ))
        },
    )?)
}

/// Parse `text` for `source_id` and show what an import would do. Writes nothing.
pub fn preview(
    conn: &Connection,
    source_id: i64,
    text: &str,
    purchases_positive_override: Option<bool>,
    today: NaiveDate,
) -> Result<Preview> {
    let src = source(conn, source_id)?;
    let parsed = parse::parse(text, today);
    let (purchases_positive, sign_detected) =
        purchases_positive(&src.format, &parsed.rows, purchases_positive_override);

    let incoming: Vec<Incoming> = parsed
        .rows
        .iter()
        .map(|r| Incoming {
            date: r.date,
            amount_cents: normalize(r, purchases_positive),
            merchant_key: clean::merchant_key(&r.description),
            description: r.description.clone(),
            pending: r.pending,
        })
        .collect();

    let statuses = match (
        incoming.iter().map(|r| r.date).min(),
        incoming.iter().map(|r| r.date).max(),
    ) {
        (Some(min), Some(max)) => {
            let existing = existing_for_source(
                conn,
                source_id,
                min - Duration::days(6),
                max + Duration::days(6),
            )?;
            matching::classify(&incoming, &existing)
        }
        _ => vec![],
    };
    let ctx = tagging::Context::load(conn, incoming.iter().map(|r| r.date.year()))?;

    let mut rows = Vec::with_capacity(incoming.len());
    for ((inc, parsed_row), status) in incoming.iter().zip(&parsed.rows).zip(statuses) {
        let existing_id = match &status {
            MatchStatus::New => None,
            MatchStatus::Duplicate { existing_id }
            | MatchStatus::ReplacesPending { existing_id, .. }
            | MatchStatus::PossibleDuplicate { existing_id } => Some(*existing_id),
        };
        let mut suggestion = match &status {
            MatchStatus::ReplacesPending { existing_id, .. } => {
                let (tag, auto) = tag_of(conn, *existing_id)?;
                Some(Suggestion {
                    tag,
                    auto,
                    reason: "pending".into(),
                })
            }
            _ => None,
        };
        // A pending row's tag may belong to a different year if the posted date crossed Jan 1
        if let Some(s) = &suggestion
            && transactions::validate_tag(conn, &s.tag, inc.date).is_err()
        {
            suggestion = None;
        }
        if suggestion.is_none() {
            suggestion = tagging::suggest(
                &ctx,
                inc.date,
                &inc.description,
                &inc.merchant_key,
                inc.amount_cents,
                &src.kind,
            );
        }
        let problem = (suggestion.is_none() && !matches!(status, MatchStatus::Duplicate { .. }))
            .then(|| {
                format!(
                    "No plan for {} yet. Create one to import this row.",
                    inc.date.year()
                )
            });
        rows.push(PreviewRow {
            line: parsed_row.line,
            date: inc.date,
            description: inc.description.clone(),
            display_name: clean::display_name(&inc.description),
            merchant_key: inc.merchant_key.clone(),
            amount_cents: inc.amount_cents,
            pending: inc.pending,
            status,
            existing: match existing_id {
                Some(id) => existing_summary(conn, id)?,
                None => None,
            },
            suggestion,
            problem,
        });
    }

    Ok(Preview {
        source_id,
        layout: parsed.layout,
        skipped_lines: parsed.skipped_lines,
        purchases_positive,
        sign_detected,
        rows,
    })
}

// ---------- commit ----------

#[derive(Debug, Clone, Deserialize)]
pub struct CommitRow {
    pub source_id: i64,
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: Cents,
    #[serde(default)]
    pub pending: bool,
    /// "import", "skip", or "replace_pending" (with existing_id)
    pub action: String,
    #[serde(default)]
    pub existing_id: Option<i64>,
    pub tag: Option<Tag>,
    #[serde(default)]
    pub auto: bool,
    /// The user changed the suggested tag: remember it for this merchant
    #[serde(default)]
    pub learn: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceFormatUpdate {
    pub source_id: i64,
    pub purchases_positive: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitRequest {
    pub rows: Vec<CommitRow>,
    #[serde(default)]
    pub formats: Vec<SourceFormatUpdate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportSummary {
    pub batch_id: i64,
    pub imported: usize,
    pub skipped: usize,
    pub pending_replaced: usize,
    pub pending_amount_changed: usize,
    pub stale_pending_removed: usize,
}

pub fn commit(conn: &Connection, req: &CommitRequest, today: NaiveDate) -> Result<ImportSummary> {
    let stale_days = crate::settings::load(conn)?.pending_stale_days;
    let tx = conn.unchecked_transaction()?;
    tx.execute("INSERT INTO import_batches DEFAULT VALUES", [])?;
    let batch = tx.last_insert_rowid();
    let mut summary = ImportSummary {
        batch_id: batch,
        ..Default::default()
    };
    let now = now_timestamp();

    for f in &req.formats {
        let mut format = source(&tx, f.source_id)?.format;
        format.purchases_positive = Some(f.purchases_positive);
        save_format(&tx, f.source_id, &format)?;
    }

    for (i, row) in req.rows.iter().enumerate() {
        source(&tx, row.source_id)?;
        if row.action == "skip" {
            summary.skipped += 1;
            continue;
        }
        let tag = row
            .tag
            .as_ref()
            .ok_or_else(|| AppError::invalid(format!("row {} has no tag", i + 1)))?;
        let tag = transactions::validate_tag(&tx, tag, row.date)
            .map_err(|e| AppError::invalid(format!("row {}: {e}", i + 1)))?;
        let id = transactions::insert(
            &tx,
            &NewRow {
                date: row.date,
                description: &row.description,
                amount_cents: row.amount_cents,
                tag: &tag,
                source_id: Some(row.source_id),
                is_pending: row.pending,
                is_auto: row.auto && !row.learn,
                notes: "",
                import_batch_id: Some(batch),
            },
        )?;
        match row.action.as_str() {
            "import" => summary.imported += 1,
            "replace_pending" => {
                let old_id = row
                    .existing_id
                    .ok_or_else(|| AppError::invalid("replace_pending needs existing_id"))?;
                let (old_amount, old_notes): (Cents, String) = tx
                    .query_row(
                        "SELECT amount_cents, notes FROM transactions
                         WHERE id = ?1 AND is_pending = 1 AND deleted_at IS NULL AND source_id = ?2",
                        params![old_id, row.source_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::invalid(format!("row {}: the pending transaction it replaces is gone", i + 1)))?;
                tx.execute(
                    "UPDATE transactions SET notes = ?2 WHERE id = ?1",
                    params![id, old_notes],
                )?;
                tx.execute(
                    "UPDATE transactions SET deleted_at = ?2, deleted_by_batch_id = ?3, replaced_by_id = ?4 WHERE id = ?1",
                    params![old_id, now, batch, id],
                )?;
                summary.pending_replaced += 1;
                if old_amount != row.amount_cents {
                    summary.pending_amount_changed += 1;
                }
            }
            other => return Err(AppError::invalid(format!("unknown action {other}"))),
        }
        if row.learn {
            let key = clean::merchant_key(&row.description);
            transactions::learn(&tx, &key, &tag)?;
        }
    }

    // Pending items that never posted (canceled orders, holds)
    let cutoff = (today - Duration::days(stale_days)).to_string();
    summary.stale_pending_removed = tx.execute(
        "UPDATE transactions SET deleted_at = ?1, deleted_by_batch_id = ?2
         WHERE is_pending = 1 AND deleted_at IS NULL AND date < ?3 AND import_batch_id IS NOT ?2",
        params![now, batch, cutoff],
    )?;

    tx.execute(
        "UPDATE import_batches SET summary_json = ?2 WHERE id = ?1",
        params![batch, serde_json::to_string(&summary)?],
    )?;
    tx.commit()?;
    Ok(summary)
}

#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    pub id: i64,
    pub created_at: String,
    pub summary: ImportSummary,
    pub undone_at: Option<String>,
}

pub fn list_batches(conn: &Connection, limit: i64) -> Result<Vec<Batch>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, summary_json, undone_at FROM import_batches ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            let json: String = r.get(2)?;
            Ok(Batch {
                id: r.get(0)?,
                created_at: r.get(1)?,
                summary: serde_json::from_str(&json).unwrap_or_default(),
                undone_at: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Undo a committed import: restore what it replaced or removed, then delete what it added.
/// Refused if a later import has already replaced one of this batch's pending rows.
pub fn undo(conn: &Connection, batch_id: i64) -> Result<()> {
    let undone: Option<Option<String>> = conn
        .query_row(
            "SELECT undone_at FROM import_batches WHERE id = ?1",
            [batch_id],
            |r| r.get(0),
        )
        .optional()?;
    match undone {
        None => return Err(AppError::not_found("import")),
        Some(Some(_)) => return Err(AppError::invalid("that import was already undone")),
        Some(None) => {}
    }
    let blocked: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transactions
                        WHERE import_batch_id = ?1 AND deleted_by_batch_id IS NOT NULL AND deleted_by_batch_id <> ?1)",
        [batch_id],
        |r| r.get(0),
    )?;
    if blocked {
        return Err(AppError::invalid(
            "a later import changed rows from this one; undo the later import first",
        ));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE transactions SET deleted_at = NULL, deleted_by_batch_id = NULL, replaced_by_id = NULL
         WHERE deleted_by_batch_id = ?1",
        [batch_id],
    )?;
    tx.execute("UPDATE transactions SET replaced_by_id = NULL WHERE replaced_by_id IN (SELECT id FROM transactions WHERE import_batch_id = ?1)", [batch_id])?;
    tx.execute(
        "DELETE FROM transactions WHERE import_batch_id = ?1",
        [batch_id],
    )?;
    tx.execute(
        "UPDATE import_batches SET undone_at = ?2 WHERE id = ?1",
        params![batch_id, now_timestamp()],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::plan::{self, tests::d};

    struct Setup {
        conn: Connection,
        card: i64,
        dining: i64,
    }

    fn setup() -> Setup {
        let conn = db::open_in_memory().unwrap();
        let p = plan::create_plan(&conn, 2026, None).unwrap();
        let dining = plan::add_category(&conn, p.id, "Dining", 300_000, false, d(2026, 1, 1))
            .unwrap()
            .id;
        let card = add_source(&conn, "Card A", "card").unwrap().id;
        Setup { conn, card, dining }
    }

    /// Accept every suggestion: import new rows, replace pending, skip duplicates.
    fn accept_all(p: &Preview) -> CommitRequest {
        let rows = p
            .rows
            .iter()
            .map(|r| {
                let (action, existing_id) = match &r.status {
                    MatchStatus::New | MatchStatus::PossibleDuplicate { .. } => ("import", None),
                    MatchStatus::Duplicate { .. } => ("skip", None),
                    MatchStatus::ReplacesPending { existing_id, .. } => {
                        ("replace_pending", Some(*existing_id))
                    }
                };
                CommitRow {
                    source_id: p.source_id,
                    date: r.date,
                    description: r.description.clone(),
                    amount_cents: r.amount_cents,
                    pending: r.pending,
                    action: action.into(),
                    existing_id,
                    tag: r.suggestion.as_ref().map(|s| s.tag.clone()),
                    auto: r.suggestion.as_ref().is_some_and(|s| s.auto),
                    learn: false,
                }
            })
            .collect();
        CommitRequest {
            rows,
            formats: vec![SourceFormatUpdate {
                source_id: p.source_id,
                purchases_positive: p.purchases_positive,
            }],
        }
    }

    fn count_live(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    const WEEK_1: &str = "Date\tDescription\tAmount\n\
                          11/10/2026\tSQ *EXAMPLE COFFEE 0423\t5.25\n\
                          11/10/2026\tSQ *EXAMPLE COFFEE 0423\t5.25\n\
                          11/11/2026\tEXAMPLE GROCERY #1234\t86.12\n\
                          11/12/2026\tPAYMENT THANK YOU\t-500.00\n\
                          11/13/2026\tPENDING TEST BISTRO\t42.00\n";

    #[test]
    fn weekly_flow_with_pending_duplicates_and_undo() {
        let s = setup();
        let today = d(2026, 11, 14);

        let p = preview(&s.conn, s.card, WEEK_1, None, today).unwrap();
        assert!(p.purchases_positive && p.sign_detected);
        assert_eq!(p.rows.len(), 5);
        assert_eq!(p.rows[0].amount_cents, -525);
        assert_eq!(p.rows[3].suggestion.as_ref().unwrap().tag.kind, "transfer");
        assert!(p.rows[4].pending);
        let mut req = accept_all(&p);
        // The user tags the coffee as Dining and the app learns it
        req.rows[0].tag = Some(Tag::expense(s.dining));
        req.rows[0].learn = true;
        let sum = commit(&s.conn, &req, today).unwrap();
        assert_eq!(sum.imported, 5);
        assert_eq!(
            source(&s.conn, s.card).unwrap().format.purchases_positive,
            Some(true)
        );

        // Next week: overlapping copy, the pending bistro posted with a tip, a third coffee
        let week_2 = "Date\tDescription\tAmount\n\
                      11/10/2026\tSQ *EXAMPLE COFFEE 0423\t5.25\n\
                      11/10/2026\tSQ *EXAMPLE COFFEE 0423\t5.25\n\
                      11/11/2026\tEXAMPLE GROCERY #1234\t86.12\n\
                      11/14/2026\tTEST BISTRO\t50.40\n\
                      11/17/2026\tSQ *EXAMPLE COFFEE 0423\t5.25\n";
        let today2 = d(2026, 11, 21);
        let p2 = preview(&s.conn, s.card, week_2, None, today2).unwrap();
        assert!(!p2.sign_detected, "sign convention is remembered");
        let kinds: Vec<&str> = p2
            .rows
            .iter()
            .map(|r| match r.status {
                MatchStatus::New => "new",
                MatchStatus::Duplicate { .. } => "dup",
                MatchStatus::ReplacesPending { .. } => "replace",
                MatchStatus::PossibleDuplicate { .. } => "maybe",
            })
            .collect();
        assert_eq!(kinds, vec!["dup", "dup", "dup", "replace", "new"]);
        // Learned: the new coffee is suggested as Dining
        let coffee = p2.rows[4].suggestion.as_ref().unwrap();
        assert_eq!(
            (coffee.tag.category_id, coffee.auto),
            (Some(s.dining), false)
        );

        let before = count_live(&s.conn);
        let sum2 = commit(&s.conn, &accept_all(&p2), today2).unwrap();
        assert_eq!(
            (
                sum2.imported,
                sum2.skipped,
                sum2.pending_replaced,
                sum2.pending_amount_changed
            ),
            (1, 3, 1, 1)
        );
        assert_eq!(count_live(&s.conn), before + 1);

        // Undo the second import: the pending row comes back, the new rows go
        undo(&s.conn, sum2.batch_id).unwrap();
        assert_eq!(count_live(&s.conn), before);
        let pending: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_pending = 1 AND deleted_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
        assert!(undo(&s.conn, sum2.batch_id).is_err());
    }

    #[test]
    fn stale_pending_removed_and_restored_on_undo() {
        let s = setup();
        let p = preview(
            &s.conn,
            s.card,
            "Pending\n11/01/2026\tEXAMPLE HOTEL HOLD\t100.00\n11/02/2026\tEXAMPLE GROCERY\t10.00\n",
            Some(true),
            d(2026, 11, 2),
        )
        .unwrap();
        assert!(p.rows[0].pending);
        commit(&s.conn, &accept_all(&p), d(2026, 11, 2)).unwrap();

        let p2 = preview(
            &s.conn,
            s.card,
            "11/20/2026\tEXAMPLE GROCERY\t20.00\n11/20/2026\tTEST BISTRO\t30.00\n",
            None,
            d(2026, 11, 21),
        )
        .unwrap();
        let sum = commit(&s.conn, &accept_all(&p2), d(2026, 11, 21)).unwrap();
        assert_eq!(sum.stale_pending_removed, 2);
        undo(&s.conn, sum.batch_id).unwrap();
        assert_eq!(count_live(&s.conn), 2);
    }

    #[test]
    fn rows_without_a_plan_are_flagged() {
        let s = setup();
        let p = preview(
            &s.conn,
            s.card,
            "12/30/2025\tEXAMPLE GROCERY\t10.00\n12/31/2025\tTEST BISTRO\t30.00\n",
            Some(true),
            d(2026, 1, 3),
        )
        .unwrap();
        assert!(
            p.rows
                .iter()
                .all(|r| r.problem.is_some() && r.suggestion.is_none())
        );
    }

    #[test]
    fn user_deleted_rows_are_not_reimported() {
        let s = setup();
        let text = "11/10/2026\tEXAMPLE GROCERY\t10.00\n11/11/2026\tTEST BISTRO\t30.00\n";
        let p = preview(&s.conn, s.card, text, Some(true), d(2026, 11, 14)).unwrap();
        commit(&s.conn, &accept_all(&p), d(2026, 11, 14)).unwrap();
        let id: i64 = s
            .conn
            .query_row(
                "SELECT id FROM transactions WHERE merchant_key = 'test bistro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        transactions::delete(&s.conn, &[id]).unwrap();
        let p2 = preview(&s.conn, s.card, text, None, d(2026, 11, 15)).unwrap();
        assert!(
            p2.rows
                .iter()
                .all(|r| matches!(r.status, MatchStatus::Duplicate { .. }))
        );
    }
}
