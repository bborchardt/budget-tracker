//! Liquid savings accounts: starting balance + transfers, corrected by occasional snapshots.
//!
//! A transfer's effect on savings is the opposite of its amount: -$500 out of checking
//! "to savings" is +$500 in savings.

use crate::error::{AppError, Result};
use crate::money::Cents;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SavingsAccount {
    pub id: i64,
    pub name: String,
    pub starting_balance_cents: Cents,
    pub starting_date: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub account_id: i64,
    pub date: String,
    pub balance_cents: Cents,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountBalance {
    pub account: SavingsAccount,
    /// Starting balance + all transfers through the as-of date
    pub calculated_cents: Cents,
    pub latest_snapshot: Option<Snapshot>,
    /// Best estimate: latest snapshot + transfers since it (or the calculated balance)
    pub balance_cents: Cents,
    /// Latest snapshot minus the calculated balance on that date: missed transfers and interest
    pub drift_cents: Option<Cents>,
}

fn account_from_row(r: &Row) -> rusqlite::Result<SavingsAccount> {
    Ok(SavingsAccount {
        id: r.get(0)?,
        name: r.get(1)?,
        starting_balance_cents: r.get(2)?,
        starting_date: r.get(3)?,
        archived: r.get(4)?,
    })
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<SavingsAccount>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, starting_balance_cents, starting_date, archived FROM savings_accounts
         ORDER BY archived, name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], account_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn account(conn: &Connection, id: i64) -> Result<SavingsAccount> {
    conn.query_row(
        "SELECT id, name, starting_balance_cents, starting_date, archived FROM savings_accounts WHERE id = ?1",
        [id],
        account_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("savings account"))
}

fn clean_name(name: &str) -> Result<String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err(AppError::invalid("name can't be empty"));
    }
    Ok(n)
}

pub fn add_account(
    conn: &Connection,
    name: &str,
    starting_balance: Cents,
    starting_date: NaiveDate,
) -> Result<SavingsAccount> {
    let name = clean_name(name)?;
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM savings_accounts WHERE name = ?1 COLLATE NOCASE)",
        [&name],
        |r| r.get(0),
    )?;
    if taken {
        return Err(AppError::invalid(format!(
            "a savings account named \"{name}\" already exists"
        )));
    }
    conn.execute(
        "INSERT INTO savings_accounts (name, starting_balance_cents, starting_date) VALUES (?1, ?2, ?3)",
        params![name, starting_balance, starting_date.to_string()],
    )?;
    account(conn, conn.last_insert_rowid())
}

pub fn update_account(
    conn: &Connection,
    id: i64,
    name: &str,
    starting_balance: Cents,
    starting_date: NaiveDate,
    archived: bool,
) -> Result<SavingsAccount> {
    let name = clean_name(name)?;
    account(conn, id)?;
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM savings_accounts WHERE name = ?1 COLLATE NOCASE AND id <> ?2)",
        params![name, id],
        |r| r.get(0),
    )?;
    if taken {
        return Err(AppError::invalid(format!(
            "a savings account named \"{name}\" already exists"
        )));
    }
    conn.execute(
        "UPDATE savings_accounts SET name = ?2, starting_balance_cents = ?3, starting_date = ?4, archived = ?5
         WHERE id = ?1",
        params![id, name, starting_balance, starting_date.to_string(), archived],
    )?;
    account(conn, id)
}

pub fn add_snapshot(
    conn: &Connection,
    account_id: i64,
    date: NaiveDate,
    balance: Cents,
    note: &str,
) -> Result<Snapshot> {
    account(conn, account_id)?;
    conn.execute(
        "INSERT INTO balance_snapshots (account_id, date, balance_cents, note) VALUES (?1, ?2, ?3, ?4)",
        params![account_id, date.to_string(), balance, note.trim()],
    )?;
    let id = conn.last_insert_rowid();
    Ok(conn.query_row(
        "SELECT id, account_id, date, balance_cents, note FROM balance_snapshots WHERE id = ?1",
        [id],
        snapshot_from_row,
    )?)
}

pub fn delete_snapshot(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM balance_snapshots WHERE id = ?1", [id])?;
    Ok(())
}

fn snapshot_from_row(r: &Row) -> rusqlite::Result<Snapshot> {
    Ok(Snapshot {
        id: r.get(0)?,
        account_id: r.get(1)?,
        date: r.get(2)?,
        balance_cents: r.get(3)?,
        note: r.get(4)?,
    })
}

pub fn snapshots(conn: &Connection, account_id: i64) -> Result<Vec<Snapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, date, balance_cents, note FROM balance_snapshots WHERE account_id = ?1
         ORDER BY date DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([account_id], snapshot_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Net change to the account from transfers dated after `after` (exclusive) through `through`.
fn transfers_between(
    conn: &Connection,
    account_id: i64,
    after: &str,
    through: &str,
) -> Result<Cents> {
    Ok(conn.query_row(
        "SELECT COALESCE(-SUM(amount_cents), 0) FROM transactions
         WHERE savings_account_id = ?1 AND type = 'transfer' AND deleted_at IS NULL
           AND date > ?2 AND date <= ?3",
        params![account_id, after, through],
        |r| r.get(0),
    )?)
}

pub fn balance(
    conn: &Connection,
    account: &SavingsAccount,
    as_of: NaiveDate,
) -> Result<AccountBalance> {
    let as_of_s = as_of.to_string();
    let calculated = account.starting_balance_cents
        + transfers_between(conn, account.id, &account.starting_date, &as_of_s)?;
    let latest: Option<Snapshot> = conn
        .query_row(
            "SELECT id, account_id, date, balance_cents, note FROM balance_snapshots
             WHERE account_id = ?1 AND date <= ?2 ORDER BY date DESC, id DESC LIMIT 1",
            params![account.id, as_of_s],
            snapshot_from_row,
        )
        .optional()?;
    let (balance, drift) = match &latest {
        Some(s) => {
            let since = transfers_between(conn, account.id, &s.date, &as_of_s)?;
            let calc_then = account.starting_balance_cents
                + transfers_between(conn, account.id, &account.starting_date, &s.date)?;
            (s.balance_cents + since, Some(s.balance_cents - calc_then))
        }
        None => (calculated, None),
    };
    Ok(AccountBalance {
        account: account.clone(),
        calculated_cents: calculated,
        latest_snapshot: latest,
        balance_cents: balance,
        drift_cents: drift,
    })
}

/// (moved into savings, moved out of savings) during `year` through `as_of`, both positive.
pub fn year_flows(conn: &Connection, year: i32, as_of: NaiveDate) -> Result<(Cents, Cents)> {
    let start = format!("{year}-01-01");
    let end = as_of.min(crate::calendar::year_end(year)).to_string();
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN amount_cents < 0 THEN -amount_cents ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN amount_cents > 0 THEN amount_cents ELSE 0 END), 0)
         FROM transactions
         WHERE savings_account_id IS NOT NULL AND type = 'transfer' AND deleted_at IS NULL
           AND date >= ?1 AND date <= ?2",
        params![start, end],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::plan::tests::d;

    fn transfer(conn: &Connection, date: &str, account_id: i64, amount: Cents) {
        conn.execute(
            "INSERT INTO transactions (date, description, display_name, amount_cents, type, savings_account_id)
             VALUES (?1, 'TRANSFER', 'Transfer', ?2, 'transfer', ?3)",
            params![date, amount, account_id],
        )
        .unwrap();
    }

    #[test]
    fn balance_from_transfers_and_snapshots() {
        let conn = db::open_in_memory().unwrap();
        let a = add_account(&conn, "Savings 1", 1_000_000, d(2026, 1, 1)).unwrap();
        assert!(add_account(&conn, "savings 1", 0, d(2026, 1, 1)).is_err());
        transfer(&conn, "2026-02-01", a.id, -50_000); // into savings
        transfer(&conn, "2026-03-01", a.id, -50_000);
        transfer(&conn, "2026-04-01", a.id, 20_000); // drawn out

        let b = balance(&conn, &a, d(2026, 3, 15)).unwrap();
        assert_eq!(b.calculated_cents, 1_100_000);
        assert_eq!(b.balance_cents, 1_100_000);
        assert!(b.drift_cents.is_none());

        // Snapshot shows interest the transfers don't
        add_snapshot(&conn, a.id, d(2026, 3, 15), 1_102_500, "").unwrap();
        let b = balance(&conn, &a, d(2026, 4, 30)).unwrap();
        assert_eq!(b.calculated_cents, 1_080_000);
        assert_eq!(b.drift_cents, Some(2_500));
        assert_eq!(b.balance_cents, 1_082_500);

        assert_eq!(
            year_flows(&conn, 2026, d(2026, 12, 31)).unwrap(),
            (100_000, 20_000)
        );
        assert_eq!(
            year_flows(&conn, 2026, d(2026, 2, 15)).unwrap(),
            (50_000, 0)
        );
    }
}
