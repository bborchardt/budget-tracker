//! Turning messy input into rows: CSV exports, tab-separated copies from bank websites, and
//! "stacked" copies where each transaction spans several lines.
//!
//! Amounts come out signed as written; the caller normalizes sign per source.

use crate::money::{self, Cents, Marker};
use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedRow {
    /// 1-based line in the input
    pub line: usize,
    pub date: NaiveDate,
    pub description: String,
    /// As written in the input (sign not yet normalized)
    pub amount_cents: Cents,
    pub marker: Option<Marker>,
    /// From separate debit/credit columns: already signed (debit negative)
    pub from_debit_credit: bool,
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseOutput {
    pub rows: Vec<ParsedRow>,
    /// Non-empty lines that weren't transactions (headers, balances, totals, unreadable)
    pub skipped_lines: usize,
    /// "csv", "tab", "semicolon", "pipe", "spaced", or "stacked"
    pub layout: String,
}

// ---------- dates ----------

static WEEKDAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:mon|tue|tues|wed|thu|thur|thurs|fri|sat|sun)[a-z]*\.?,?\s+").unwrap()
});
static NUMERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2})[/.-](\d{1,2})(?:[/.-](\d{4}|\d{2}))?$").unwrap());
static ISO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4})[/.-](\d{1,2})[/.-](\d{1,2})$").unwrap());
static MONTH_FIRST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([a-z]{3,9})\.?\s+(\d{1,2})(?:st|nd|rd|th)?(?:,?\s+(\d{4}))?$").unwrap()
});
static DAY_FIRST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(\d{1,2})\s+([a-z]{3,9})\.?(?:,?\s+(\d{4}))?$").unwrap());

fn month_from_name(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let idx = MONTHS.iter().position(|m| n.starts_with(m))?;
    let full = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    // Accept "Mar", "March", "Sept"; reject other words that happen to start with a month
    (n.len() == 3 || full[idx].starts_with(&n) || n == "sept").then_some(idx as u32 + 1)
}

/// A date without a year is assumed to be the most recent such date (allowing a week of
/// slack for timezone and posting differences).
fn infer_year(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if this_year > today + Duration::days(7) {
        NaiveDate::from_ymd_opt(today.year() - 1, month, day)
    } else {
        Some(this_year)
    }
}

fn ymd(year: Option<&str>, month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    match year {
        Some(y) => {
            let mut y: i32 = y.parse().ok()?;
            if y < 100 {
                y += 2000;
            }
            if !(1990..=2200).contains(&y) {
                return None;
            }
            NaiveDate::from_ymd_opt(y, month, day)
        }
        None => infer_year(month, day, today),
    }
}

/// Parse a date in any common US bank format. Numeric dates are month-first.
pub fn parse_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = input.trim().trim_end_matches(',').trim();
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "today" => return Some(today),
        "yesterday" => return Some(today - Duration::days(1)),
        _ => {}
    }
    let s = WEEKDAY_RE.replace(s, "");
    let s = s.trim();
    if let Some(c) = ISO_RE.captures(s) {
        return ymd(Some(&c[1]), c[2].parse().ok()?, c[3].parse().ok()?, today);
    }
    if let Some(c) = NUMERIC_RE.captures(s) {
        return ymd(
            c.get(3).map(|m| m.as_str()),
            c[1].parse().ok()?,
            c[2].parse().ok()?,
            today,
        );
    }
    if let Some(c) = MONTH_FIRST_RE.captures(s) {
        let month = month_from_name(&c[1])?;
        return ymd(
            c.get(3).map(|m| m.as_str()),
            month,
            c[2].parse().ok()?,
            today,
        );
    }
    if let Some(c) = DAY_FIRST_RE.captures(s) {
        let month = month_from_name(&c[2])?;
        return ymd(
            c.get(3).map(|m| m.as_str()),
            month,
            c[1].parse().ok()?,
            today,
        );
    }
    None
}

// ---------- shared helpers ----------

static JUNK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:beginning|ending|opening|closing|previous|new|available|current|daily|statement)\s+balance|balance\b|total\b|totals\b|statement\b|page \d)",
    )
    .unwrap()
});

fn is_junk_description(d: &str) -> bool {
    JUNK_RE.is_match(d.trim())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Pending,
    Posted,
}

static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(pending|posted|cleared|recent|processed)(?:\s+(?:transactions|activity|items|charges))?:?$")
        .unwrap()
});

fn section_header(s: &str) -> Option<Section> {
    let c = SECTION_RE.captures(s.trim())?;
    Some(if c[1].eq_ignore_ascii_case("pending") {
        Section::Pending
    } else {
        Section::Posted
    })
}

fn mentions_pending(s: &str) -> bool {
    s.to_ascii_lowercase().contains("pending")
}

// ---------- entry point ----------

pub fn parse(text: &str, today: NaiveDate) -> ParseOutput {
    let lines: Vec<&str> = text.lines().collect();
    let non_empty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    if non_empty == 0 {
        return ParseOutput {
            rows: vec![],
            skipped_lines: 0,
            layout: "empty".into(),
        };
    }

    type Splitter = fn(&str) -> Vec<String>;
    let candidates: [(&str, Splitter); 5] = [
        ("tab", |l| split_simple(l, '\t')),
        ("csv", |l| split_csv(l, ',')),
        ("semicolon", |l| split_csv(l, ';')),
        ("pipe", |l| split_simple(l, '|')),
        ("spaced", split_spaced),
    ];
    let needed = ((non_empty as f64) * 0.4).ceil().max(2.0) as usize;
    for (name, split) in candidates {
        let table: Vec<(usize, Vec<String>)> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| (i + 1, split(l)))
            .collect();
        let wide = table
            .iter()
            .filter(|(_, cells)| cells.iter().filter(|c| !c.is_empty()).count() >= 3)
            .count();
        if wide >= needed.min(non_empty) {
            let (rows, skipped) = parse_table(&table, today);
            if !rows.is_empty() {
                return ParseOutput {
                    rows,
                    skipped_lines: skipped,
                    layout: name.into(),
                };
            }
        }
    }
    let (rows, skipped) = parse_stacked(&lines, today);
    ParseOutput {
        rows,
        skipped_lines: skipped,
        layout: "stacked".into(),
    }
}

fn split_simple(line: &str, delim: char) -> Vec<String> {
    line.split(delim).map(|c| c.trim().to_string()).collect()
}

fn split_spaced(line: &str) -> Vec<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}|\t").unwrap());
    RE.split(line.trim())
        .map(|c| c.trim().to_string())
        .collect()
}

/// Split a CSV line, honoring double quotes ("" inside quotes is a literal quote).
fn split_csv(line: &str, delim: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            c if c == delim && !in_quotes => {
                cells.push(std::mem::take(&mut cur).trim().to_string())
            }
            c => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

// ---------- tables ----------

#[derive(Debug, Default, Clone, Copy)]
struct Columns {
    date: Option<usize>,
    description: Option<usize>,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    status: Option<usize>,
}

fn header_columns(cells: &[String]) -> Option<Columns> {
    let mut cols = Columns::default();
    let mut date_rank = 0;
    let mut desc_rank = 0;
    let mut hits = 0;
    for (i, cell) in cells.iter().enumerate() {
        let h = cell.to_ascii_lowercase();
        let h = h.trim();
        if h.is_empty() {
            continue;
        }
        if h.contains("date") {
            hits += 1;
            let rank = if h.contains("trans") {
                3
            } else if h.contains("post") {
                1
            } else {
                2
            };
            if rank > date_rank {
                date_rank = rank;
                cols.date = Some(i);
            }
        } else if h.contains("amount") {
            hits += 1;
            cols.amount.get_or_insert(i);
        } else if h.contains("debit")
            || h.contains("withdrawal")
            || h == "charge"
            || h == "charges"
            || h == "money out"
        {
            hits += 1;
            cols.debit.get_or_insert(i);
        } else if h.contains("credit")
            || h.contains("deposit")
            || h == "payment"
            || h == "payments"
            || h == "money in"
        {
            hits += 1;
            cols.credit.get_or_insert(i);
        } else if h == "status" || h == "state" {
            hits += 1;
            cols.status = Some(i);
        } else if h.contains("balance")
            || h.contains("category")
            || h == "type"
            || h.contains("reference")
            || h.contains("check")
        {
            hits += 1;
        } else {
            let rank = match h {
                _ if h.contains("description") => 4,
                _ if h.contains("merchant") || h.contains("payee") || h.contains("details") => 3,
                _ if h.contains("name") || h.contains("transaction") => 2,
                _ if h.contains("memo") => 1,
                _ => 0,
            };
            if rank > 0 {
                hits += 1;
                if rank > desc_rank {
                    desc_rank = rank;
                    cols.description = Some(i);
                }
            }
        }
    }
    let has_money = cols.amount.is_some() || cols.debit.is_some() || cols.credit.is_some();
    (hits >= 2 && cols.date.is_some() && has_money).then_some(cols)
}

fn infer_columns(rows: &[&Vec<String>], today: NaiveDate) -> Option<Columns> {
    let width = rows.iter().map(|r| r.len()).max()?;
    let n = rows.len() as f64;
    let cell =
        |r: &Vec<String>, i: usize| r.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut date_frac = vec![0.0; width];
    let mut amount_frac = vec![0.0; width];
    let mut filled = vec![0.0; width];
    let mut letters = vec![0.0; width];
    let mut status_frac = vec![0.0; width];
    for r in rows {
        for i in 0..width {
            let c = cell(r, i);
            if c.is_empty() {
                continue;
            }
            filled[i] += 1.0;
            if parse_date(&c, today).is_some() {
                date_frac[i] += 1.0;
            }
            if money::looks_like_amount(&c) {
                amount_frac[i] += 1.0;
            }
            letters[i] += c.chars().filter(|ch| ch.is_alphabetic()).count() as f64;
            if matches!(
                c.to_ascii_lowercase().as_str(),
                "pending" | "posted" | "cleared"
            ) {
                status_frac[i] += 1.0;
            }
        }
    }
    let date = (0..width).find(|&i| date_frac[i] / n >= 0.6)?;
    let amount_cols: Vec<usize> = (0..width)
        .filter(|&i| {
            i != date && filled[i] / n >= 0.3 && amount_frac[i] / filled[i].max(1.0) >= 0.8
        })
        .collect();
    if amount_cols.is_empty() {
        return None;
    }
    let mut cols = Columns {
        date: Some(date),
        ..Default::default()
    };
    if amount_cols.len() >= 2 {
        let (a, b) = (amount_cols[0], amount_cols[1]);
        let exclusive = rows
            .iter()
            .filter(|r| cell(r, a).is_empty() != cell(r, b).is_empty())
            .count() as f64;
        if exclusive / n >= 0.8 {
            cols.debit = Some(a);
            cols.credit = Some(b);
        } else {
            cols.amount = Some(a); // a later amount column is usually a running balance
        }
    } else {
        cols.amount = Some(amount_cols[0]);
    }
    cols.status = (0..width).find(|&i| status_frac[i] / n >= 0.5);
    let used = [
        Some(date),
        cols.amount,
        cols.debit,
        cols.credit,
        cols.status,
    ];
    cols.description = (0..width)
        .filter(|i| !used.contains(&Some(*i)) && filled[*i] / n >= 0.6)
        .max_by(|&a, &b| {
            (letters[a] / filled[a].max(1.0)).total_cmp(&(letters[b] / filled[b].max(1.0)))
        });
    cols.description?;
    Some(cols)
}

fn parse_table(table: &[(usize, Vec<String>)], today: NaiveDate) -> (Vec<ParsedRow>, usize) {
    // A header row, if any, is within the first few rows
    let mut header_at = None;
    let mut cols = None;
    for (idx, (_, cells)) in table.iter().take(5).enumerate() {
        if let Some(c) = header_columns(cells) {
            header_at = Some(idx);
            cols = Some(c);
            break;
        }
    }
    let data: Vec<&Vec<String>> = table
        .iter()
        .enumerate()
        .filter(|(i, (_, cells))| {
            Some(*i) != header_at && cells.iter().filter(|c| !c.is_empty()).count() >= 2
        })
        .map(|(_, (_, cells))| cells)
        .collect();
    let mut cols = match cols {
        Some(c) => c,
        None => match infer_columns(&data, today) {
            Some(c) => c,
            None => return (vec![], table.len()),
        },
    };
    if cols.description.is_none() {
        // Header named the money and date columns but not the description: use the wordiest other column
        let used = [cols.date, cols.amount, cols.debit, cols.credit, cols.status];
        let width = data.iter().map(|r| r.len()).max().unwrap_or(0);
        cols.description = (0..width)
            .filter(|i| !used.contains(&Some(*i)))
            .max_by_key(|&i| {
                data.iter()
                    .map(|r| {
                        r.get(i)
                            .map_or(0, |c| c.chars().filter(|ch| ch.is_alphabetic()).count())
                    })
                    .sum::<usize>()
            });
    }

    let mut rows = Vec::new();
    let mut skipped = 0;
    let mut section = Section::Posted;
    for (idx, (line, cells)) in table.iter().enumerate() {
        if Some(idx) == header_at {
            skipped += 1;
            continue;
        }
        let filled: Vec<&String> = cells.iter().filter(|c| !c.is_empty()).collect();
        if filled.len() == 1
            && let Some(s) = section_header(filled[0])
        {
            section = s;
            skipped += 1;
            continue;
        }
        let get = |i: Option<usize>| i.and_then(|i| cells.get(i)).map(|s| s.trim()).unwrap_or("");
        let Some(date) = parse_date(get(cols.date), today) else {
            skipped += 1;
            continue;
        };
        let description = get(cols.description).to_string();
        if description.is_empty() || is_junk_description(&description) {
            skipped += 1;
            continue;
        }
        let (amount, marker, from_dc) = if let Some(p) =
            money::parse_amount(get(cols.amount)).filter(|_| cols.amount.is_some())
        {
            (p.cents, p.marker, false)
        } else {
            let debit = money::parse_amount(get(cols.debit)).filter(|p| p.cents != 0);
            let credit = money::parse_amount(get(cols.credit)).filter(|p| p.cents != 0);
            match (debit, credit) {
                (Some(d), _) => (-d.cents.abs(), None, true),
                (None, Some(c)) => (c.cents.abs(), None, true),
                _ => {
                    skipped += 1;
                    continue;
                }
            }
        };
        let pending = section == Section::Pending
            || mentions_pending(get(cols.status))
            || description.to_ascii_lowercase().starts_with("pending");
        rows.push(ParsedRow {
            line: *line,
            date,
            description,
            amount_cents: amount,
            marker,
            from_debit_credit: from_dc,
            pending,
        });
    }
    (rows, skipped)
}

// ---------- stacked ----------

#[derive(Debug)]
struct Building {
    line: usize,
    date: NaiveDate,
    description: Option<String>,
    amount: Option<money::ParsedAmount>,
    pending: bool,
}

/// One line holding date, description, and amount separated by single spaces.
fn single_line(line: &str, today: NaiveDate) -> Option<(NaiveDate, String, money::ParsedAmount)> {
    let mut tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    // "42.10 CR" -> one amount token
    let mut amount_text = String::new();
    if let Some(last) = tokens.last()
        && (last.eq_ignore_ascii_case("cr") || last.eq_ignore_ascii_case("dr"))
    {
        let marker = tokens.pop()?;
        amount_text = format!("{} {marker}", tokens.pop()?);
    } else {
        amount_text.push_str(tokens.pop()?);
    }
    // A trailing running balance after the amount
    if !money::looks_like_amount(&amount_text) {
        return None;
    }
    if let Some(prev) = tokens.last()
        && money::looks_like_amount(prev)
    {
        amount_text = tokens.pop()?.to_string();
    }
    let amount = money::parse_amount(&amount_text)?;
    for k in (1..=3.min(tokens.len().saturating_sub(1))).rev() {
        let date_text = tokens[..k].join(" ");
        if let Some(date) = parse_date(&date_text, today) {
            let description = tokens[k..].join(" ");
            if description.chars().any(|c| c.is_alphabetic()) {
                return Some((date, description, amount));
            }
        }
    }
    None
}

fn parse_stacked(lines: &[&str], today: NaiveDate) -> (Vec<ParsedRow>, usize) {
    let mut rows = Vec::new();
    let mut skipped = 0;
    let mut section = Section::Posted;
    let mut cur: Option<Building> = None;

    let flush = |cur: &mut Option<Building>, rows: &mut Vec<ParsedRow>, skipped: &mut usize| {
        if let Some(b) = cur.take() {
            match (b.description, b.amount) {
                (Some(d), Some(a)) if !is_junk_description(&d) => rows.push(ParsedRow {
                    line: b.line,
                    date: b.date,
                    description: d,
                    amount_cents: a.cents,
                    marker: a.marker,
                    from_debit_credit: false,
                    pending: b.pending,
                }),
                _ => *skipped += 1,
            }
        }
    };

    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        // A bare "Pending" under a transaction is its status; otherwise it starts a section
        if lower == "pending" && cur.is_some() {
            if let Some(b) = cur.as_mut() {
                b.pending = true;
            }
            continue;
        }
        if (lower == "posted" || lower == "cleared")
            && cur.as_ref().is_some_and(|b| b.amount.is_none())
        {
            continue;
        }
        if let Some(s) = section_header(t) {
            flush(&mut cur, &mut rows, &mut skipped);
            section = s;
            skipped += 1;
            continue;
        }
        if let Some((date, description, amount)) = single_line(t, today) {
            flush(&mut cur, &mut rows, &mut skipped);
            if is_junk_description(&description) {
                skipped += 1;
                continue;
            }
            rows.push(ParsedRow {
                line: i + 1,
                date,
                pending: section == Section::Pending || lower.contains("pending"),
                description,
                amount_cents: amount.cents,
                marker: amount.marker,
                from_debit_credit: false,
            });
            continue;
        }
        if let Some(date) = parse_date(t, today) {
            flush(&mut cur, &mut rows, &mut skipped);
            cur = Some(Building {
                line: i + 1,
                date,
                description: None,
                amount: None,
                pending: section == Section::Pending,
            });
            continue;
        }
        if money::looks_like_amount(t) {
            match cur.as_mut() {
                Some(b) if b.amount.is_none() => b.amount = money::parse_amount(t),
                Some(_) => {} // a running balance after the amount
                None => skipped += 1,
            }
            continue;
        }
        match cur.as_mut() {
            // The first text line is the description; later ones (bank categories etc.) are ignored
            Some(b) if b.description.is_none() => {
                if lower.starts_with("pending") {
                    b.pending = true;
                }
                b.description = Some(t.to_string());
            }
            Some(_) => {}
            None => skipped += 1,
        }
    }
    flush(&mut cur, &mut rows, &mut skipped);
    (rows, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()
    }
    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }
    fn summary(out: &ParseOutput) -> Vec<(NaiveDate, &str, Cents, bool)> {
        out.rows
            .iter()
            .map(|r| (r.date, r.description.as_str(), r.amount_cents, r.pending))
            .collect()
    }

    #[test]
    fn dates() {
        let t = today();
        for (s, want) in [
            ("11/12/2026", d(11, 12)),
            ("11/12/26", d(11, 12)),
            ("2026-11-12", d(11, 12)),
            ("Nov 12, 2026", d(11, 12)),
            ("November 12 2026", d(11, 12)),
            ("12 Nov 2026", d(11, 12)),
            ("Thu, Nov 12, 2026", d(11, 12)),
            ("Nov 12", d(11, 12)),
            ("11/12", d(11, 12)),
            ("Sept 3", d(9, 3)),
            ("Today", d(11, 14)),
            ("Yesterday", d(11, 13)),
        ] {
            assert_eq!(parse_date(s, t), Some(want), "{s}");
        }
        // A month/day later than today belongs to last year
        assert_eq!(
            parse_date("Dec 30", t),
            NaiveDate::from_ymd_opt(2025, 12, 30)
        );
        for s in ["", "13/40/2026", "Example", "Mayhem 3", "0423", "1/5/1880"] {
            assert_eq!(parse_date(s, t), None, "{s}");
        }
    }

    #[test]
    fn csv_with_header_and_quotes() {
        let text = "Transaction Date,Post Date,Description,Category,Type,Amount\n\
                    11/10/2026,11/11/2026,\"EXAMPLE GROCERY #1234\",Groceries,Sale,-42.10\n\
                    11/11/2026,11/12/2026,\"TEST BISTRO, LLC\",Food,Sale,-18.40\n\
                    11/12/2026,11/12/2026,PAYMENT THANK YOU,,Payment,500.00\n";
        let out = parse(text, today());
        assert_eq!(out.layout, "csv");
        assert_eq!(
            summary(&out),
            vec![
                (d(11, 10), "EXAMPLE GROCERY #1234", -4210, false),
                (d(11, 11), "TEST BISTRO, LLC", -1840, false),
                (d(11, 12), "PAYMENT THANK YOU", 50000, false),
            ]
        );
        assert_eq!(out.skipped_lines, 1);
    }

    #[test]
    fn debit_credit_columns() {
        let text = "Date,Description,Debit,Credit,Balance\n\
                    11/02/2026,EXAMPLE HARDWARE,25.00,,975.00\n\
                    11/03/2026,TEST EMPLOYER PAYROLL,,2000.00,2975.00\n\
                    11/03/2026,Ending balance,,,2975.00\n";
        let out = parse(text, today());
        assert_eq!(
            summary(&out),
            vec![
                (d(11, 2), "EXAMPLE HARDWARE", -2500, false),
                (d(11, 3), "TEST EMPLOYER PAYROLL", 200000, false)
            ]
        );
        assert!(out.rows.iter().all(|r| r.from_debit_credit));
    }

    #[test]
    fn tab_separated_website_copy_with_sections() {
        let text = "Pending\n\
                    Nov 13, 2026\tEXAMPLE FUEL\t$1.00\n\
                    Posted\n\
                    Nov 12, 2026\tSQ *EXAMPLE COFFEE\t$5.25\t$1,200.00\n\
                    Nov 11, 2026\tEXAMPLE GROCERY\t$86.12\t$1,205.25\n";
        let out = parse(text, today());
        assert_eq!(out.layout, "tab");
        assert_eq!(
            summary(&out),
            vec![
                (d(11, 13), "EXAMPLE FUEL", 100, true),
                (d(11, 12), "SQ *EXAMPLE COFFEE", 525, false),
                (d(11, 11), "EXAMPLE GROCERY", 8612, false),
            ]
        );
    }

    #[test]
    fn headerless_columns_are_inferred() {
        let text = "11/10/2026\tSale\tEXAMPLE GROCERY #1234\t-42.10\n\
                    11/11/2026\tSale\tTEST BISTRO\t-18.40\n\
                    11/12/2026\tPayment\tPAYMENT THANK YOU\t500.00\n";
        let out = parse(text, today());
        assert_eq!(out.rows.len(), 3);
        assert_eq!(out.rows[0].description, "EXAMPLE GROCERY #1234");
        assert_eq!(out.rows[2].amount_cents, 50000);
    }

    #[test]
    fn stacked_website_copy() {
        let text = "Pending transactions\n\
                    Nov 13, 2026\n\
                    EXAMPLE FUEL\n\
                    -$1.00\n\
                    \n\
                    Posted transactions\n\
                    Nov 12, 2026\n\
                    SQ *EXAMPLE COFFEE\n\
                    Dining\n\
                    -$5.25\n\
                    $1,200.00\n\
                    Nov 11, 2026\n\
                    EXAMPLE GROCERY\n\
                    Pending\n\
                    -$86.12\n\
                    Nov 10, 2026\n\
                    Beginning balance\n\
                    $1,291.37\n";
        let out = parse(text, today());
        assert_eq!(out.layout, "stacked");
        assert_eq!(
            summary(&out),
            vec![
                (d(11, 13), "EXAMPLE FUEL", -100, true),
                (d(11, 12), "SQ *EXAMPLE COFFEE", -525, false),
                (d(11, 11), "EXAMPLE GROCERY", -8612, true),
            ]
        );
    }

    #[test]
    fn single_line_rows() {
        let text = "11/12 EXAMPLE GROCERY 86.12\n\
                    11/13 TEST REFUND STORE 20.00 CR\n\
                    Nov 14 EXAMPLE COFFEE -5.25 1,194.75\n";
        let out = parse(text, today());
        assert_eq!(
            summary(&out),
            vec![
                (d(11, 12), "EXAMPLE GROCERY", 8612, false),
                (d(11, 13), "TEST REFUND STORE", 2000, false),
                (d(11, 14), "EXAMPLE COFFEE", -525, false),
            ]
        );
        assert_eq!(out.rows[1].marker, Some(Marker::Credit));
    }

    #[test]
    fn junk_and_empty_input() {
        assert!(parse("", today()).rows.is_empty());
        let out = parse("Account summary\nNothing to see here\n", today());
        assert!(out.rows.is_empty());
        assert_eq!(out.skipped_lines, 2);
    }
}
