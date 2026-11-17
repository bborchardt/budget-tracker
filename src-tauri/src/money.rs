//! Money is integer cents everywhere. Parsing accepts the formats banks use.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

pub type Cents = i64;

/// A credit/debit marker written next to an amount (e.g. `42.10 CR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Marker {
    Credit,
    Debit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedAmount {
    /// Signed as written (a leading `-`, trailing `-`, or parentheses make it negative).
    pub cents: Cents,
    pub marker: Option<Marker>,
}

static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(\d{1,3}(?:,\d{3})+)|(\d+))?(?:\.(\d{1,2}))?$").unwrap());

/// Parse an amount like `-42.10`, `(42.10)`, `$1,234.56`, `42.10 CR`, `−$5.00`, `42.10-`.
/// Returns `None` for anything that isn't clearly an amount.
pub fn parse_amount(input: &str) -> Option<ParsedAmount> {
    let mut s: String = input
        .trim()
        .replace(['\u{2212}', '\u{2013}', '\u{2014}'], "-")
        .replace('\u{a0}', " ");
    if s.is_empty() {
        return None;
    }
    let mut negative = false;
    let mut marker = None;

    let upper = s.to_ascii_uppercase();
    for (suffix, m) in [("CR", Marker::Credit), ("DR", Marker::Debit)] {
        if upper.ends_with(suffix) && s.len() > 2 {
            s.truncate(s.len() - 2);
            s = s.trim_end().to_string();
            marker = Some(m);
            break;
        }
    }
    if let Some(rest) = s.strip_suffix("USD") {
        s = rest.trim_end().to_string();
    }
    if s.starts_with('(') && s.ends_with(')') && s.len() > 2 {
        negative = true;
        s = s[1..s.len() - 1].trim().to_string();
    }
    // Signs and the currency symbol may come in either order: -$5, $-5, +$5
    for _ in 0..2 {
        if let Some(rest) = s.strip_prefix('-') {
            negative = !negative;
            s = rest.trim_start().to_string();
        } else if let Some(rest) = s.strip_prefix('+') {
            s = rest.trim_start().to_string();
        }
        if let Some(rest) = s.strip_prefix('$') {
            s = rest.trim_start().to_string();
        }
    }
    if let Some(rest) = s.strip_suffix('-') {
        negative = !negative;
        s = rest.trim_end().to_string();
    }
    if s.is_empty() {
        return None;
    }

    let caps = NUMBER.captures(&s)?;
    let whole = caps
        .get(1)
        .or_else(|| caps.get(2))
        .map(|m| m.as_str().replace(',', ""));
    let frac = caps.get(3).map(|m| m.as_str());
    if whole.is_none() && frac.is_none() {
        return None;
    }
    let whole: i64 = whole.as_deref().unwrap_or("0").parse().ok()?;
    let frac_cents: i64 = match frac {
        None => 0,
        Some(f) if f.len() == 1 => f.parse::<i64>().ok()? * 10,
        Some(f) => f.parse().ok()?,
    };
    let magnitude = whole.checked_mul(100)?.checked_add(frac_cents)?;
    Some(ParsedAmount {
        cents: if negative { -magnitude } else { magnitude },
        marker,
    })
}

/// True if the text looks like an amount rather than a plain number such as a store or
/// reference number: it has cents, a `$`, or a CR/DR marker.
pub fn looks_like_amount(input: &str) -> bool {
    let t = input.trim();
    parse_amount(t).is_some()
        && (t.contains('.') || t.contains('$') || {
            let u = t.to_ascii_uppercase();
            u.ends_with("CR") || u.ends_with("DR")
        })
}

/// `$1,234.56` / `-$1,234.56`
pub fn format_cents(cents: Cents) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}${grouped}.{:02}", abs % 100)
}

/// Split `amount` into (numerator/12, remainder), rounding the first part to the nearest
/// cent so the two always add up exactly.
pub fn split_twelfths(amount: Cents, twelfths: i64) -> (Cents, Cents) {
    let tw = twelfths.clamp(0, 12);
    let first = (amount * tw + 6).div_euclid(12);
    (first, amount - first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cents(s: &str) -> Option<Cents> {
        parse_amount(s).map(|p| p.cents)
    }

    #[test]
    fn parses_common_formats() {
        assert_eq!(cents("42.10"), Some(4210));
        assert_eq!(cents("-42.10"), Some(-4210));
        assert_eq!(cents("(42.10)"), Some(-4210));
        assert_eq!(cents("$1,234.56"), Some(123456));
        assert_eq!(cents("-$1,234.56"), Some(-123456));
        assert_eq!(cents("$-5.00"), Some(-500));
        assert_eq!(cents("\u{2212}$5.00"), Some(-500));
        assert_eq!(cents("42.10-"), Some(-4210));
        assert_eq!(cents("+12"), Some(1200));
        assert_eq!(cents("12.5"), Some(1250));
        assert_eq!(cents(".99"), Some(99));
        assert_eq!(cents("1,000"), Some(100000));
        assert_eq!(cents(" $ 7.00 USD "), Some(700));
    }

    #[test]
    fn parses_markers() {
        let p = parse_amount("42.10 CR").unwrap();
        assert_eq!((p.cents, p.marker), (4210, Some(Marker::Credit)));
        let p = parse_amount("42.10DR").unwrap();
        assert_eq!((p.cents, p.marker), (4210, Some(Marker::Debit)));
    }

    #[test]
    fn rejects_non_amounts() {
        for s in [
            "", "abc", "12.345", "1,23", "$", "-", "()", "12/14", "1.2.3", "CR",
        ] {
            assert_eq!(cents(s), None, "{s:?}");
        }
    }

    #[test]
    fn amount_like_needs_cents_or_symbol() {
        assert!(looks_like_amount("42.10"));
        assert!(looks_like_amount("$42"));
        assert!(!looks_like_amount("0423"));
        assert!(!looks_like_amount("Example"));
    }

    #[test]
    fn formats() {
        assert_eq!(format_cents(0), "$0.00");
        assert_eq!(format_cents(5), "$0.05");
        assert_eq!(format_cents(123456), "$1,234.56");
        assert_eq!(format_cents(-100000000), "-$1,000,000.00");
    }

    #[test]
    fn twelfths_add_up() {
        assert_eq!(split_twelfths(1_200_000, 10), (1_000_000, 200_000));
        assert_eq!(split_twelfths(1_000_001, 8), (666_667, 333_334));
        assert_eq!(split_twelfths(500, 12), (500, 0));
        assert_eq!(split_twelfths(500, 0), (0, 500));
        for amount in [1, 7, 99, 12_345] {
            for tw in 0..=12 {
                let (a, b) = split_twelfths(amount, tw);
                assert_eq!(a + b, amount);
            }
        }
    }
}
