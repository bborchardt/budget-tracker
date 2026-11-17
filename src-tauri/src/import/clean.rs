//! Cleaning bank descriptions into display names and merchant keys.
//!
//! The merchant key is what learned tagging and duplicate matching compare, so it needs to be
//! stable for the same merchant more than it needs to be pretty.

use regex::Regex;
use std::sync::LazyLock;

/// Payment-processor and bank prefixes that hide the merchant name.
const PREFIXES: &[&str] = &[
    "SQ *",
    "SQ*",
    "TST* ",
    "TST*",
    "SP * ",
    "SP *",
    "PAYPAL *",
    "PP*",
    "POS PURCHASE ",
    "POS DEBIT ",
    "POS ",
    "DEBIT CARD PURCHASE ",
    "DEBIT PURCHASE ",
    "CARD PURCHASE ",
    "PURCHASE ",
    "RECURRING PAYMENT ",
    "PENDING ",
    "PENDING - ",
    "DDA ",
];

static LEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:PURCHASE AUTHORIZED ON \d{1,2}/\d{1,2}\s+|CHECKCARD \d{4}\s+|POS \d{4}\s+)",
    )
    .unwrap()
});
static PHONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}[-.]\d{3}[-.]\d{4}\b").unwrap());
static REF_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*[A-Za-z0-9]{3,}\b").unwrap());
static STORE_NO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\s*\d+").unwrap());

const STATES: &[&str] = &[
    "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "DC", "FL", "GA", "HI", "ID", "IL", "IN", "IA",
    "KS", "KY", "LA", "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ", "NM",
    "NY", "NC", "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VT", "VA", "WA",
    "WV", "WI", "WY",
];

fn strip_prefixes(s: &str) -> String {
    let mut out = s.trim().to_string();
    loop {
        let before = out.clone();
        out = LEADING_RE.replace(&out, "").trim().to_string();
        let upper = out.to_ascii_uppercase();
        if let Some(p) = PREFIXES.iter().find(|p| upper.starts_with(*p)) {
            out = out[p.len()..].trim().to_string();
        }
        if out == before {
            return out;
        }
    }
}

fn title_case(word: &str) -> String {
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "SQ *EXAMPLE COFFEE 0423 ANYTOWN CA" -> "Example Coffee"
pub fn display_name(description: &str) -> String {
    let collapsed = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = strip_prefixes(&collapsed);
    s = PHONE_RE.replace_all(&s, " ").to_string();
    s = REF_RE.replace_all(&s, " ").to_string();
    s = STORE_NO_RE.replace_all(&s, " ").to_string();

    let mut tokens: Vec<String> = s
        .split_whitespace()
        .map(|t| t.split('/').next().unwrap_or(t).to_string())
        .filter(|t| {
            let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
            digits < 2 && t.chars().any(|c| c.is_alphanumeric())
        })
        .collect();

    // Trailing "CITY ST" (only when there's clearly a name before it)
    if tokens.len() >= 4 && tokens.last().is_some_and(|t| STATES.contains(&t.as_str())) {
        tokens.truncate(tokens.len() - 2);
    }
    let name = tokens
        .iter()
        .map(|t| t.trim_matches(|c: char| matches!(c, '*' | '-' | ',' | '.' | ':')))
        .filter(|t| !t.is_empty())
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        collapsed
            .split_whitespace()
            .map(title_case)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        name
    }
}

/// Lowercase letters and spaces only: "Example Grocery" for any store number or location.
pub fn merchant_key(description: &str) -> String {
    let name = display_name(description).to_lowercase();
    let letters: String = name
        .chars()
        .map(|c| if c.is_alphabetic() { c } else { ' ' })
        .collect();
    letters.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn bigrams(s: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Dice similarity of character bigrams, 0.0-1.0.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (a.trim(), b.trim());
    if a.eq_ignore_ascii_case(b) {
        return 1.0;
    }
    let (ba, mut bb) = (bigrams(a), bigrams(b));
    if ba.is_empty() || bb.is_empty() {
        return 0.0;
    }
    let total = ba.len() + bb.len();
    let mut matches = 0;
    for g in &ba {
        if let Some(pos) = bb.iter().position(|x| x == g) {
            bb.swap_remove(pos);
            matches += 1;
        }
    }
    2.0 * matches as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_common_descriptions() {
        assert_eq!(
            display_name("SQ *EXAMPLE COFFEE 0423 ANYTOWN CA"),
            "Example Coffee"
        );
        assert_eq!(display_name("EXAMPLE GROCERY #1234"), "Example Grocery");
        assert_eq!(display_name("TST* TEST BISTRO"), "Test Bistro");
        assert_eq!(
            display_name("PURCHASE AUTHORIZED ON 11/12 EXAMPLE HARDWARE"),
            "Example Hardware"
        );
        assert_eq!(
            display_name("EXAMPLE MKTP US*2K4AB12 EXAMPLE.COM/BILL WA"),
            "Example Mktp Us"
        );
        assert_eq!(display_name("EXAMPLE PIZZA CO"), "Example Pizza Co");
        assert_eq!(display_name("PAYMENT THANK YOU"), "Payment Thank You");
        assert_eq!(display_name("12345"), "12345");
    }

    #[test]
    fn merchant_keys_ignore_store_numbers() {
        assert_eq!(merchant_key("EXAMPLE GROCERY #1234"), "example grocery");
        assert_eq!(merchant_key("EXAMPLE GROCERY #5678"), "example grocery");
        assert_eq!(
            merchant_key("SQ *EXAMPLE COFFEE 0423 ANYTOWN CA"),
            merchant_key("Example Coffee")
        );
    }

    #[test]
    fn similarity_scores() {
        assert_eq!(similarity("example coffee", "EXAMPLE COFFEE"), 1.0);
        assert!(similarity("example coffee", "example coffee shop") > 0.8);
        assert!(similarity("example coffee", "test hardware") < 0.3);
        assert_eq!(similarity("", "x"), 0.0);
    }
}
