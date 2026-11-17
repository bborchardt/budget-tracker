//! Matching incoming rows against what's already in the database for the same source:
//! duplicates (count-aware), pending -> posted replacement, and uncertain matches to ask about.

use super::clean::similarity;
use crate::money::Cents;
use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Incoming {
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub merchant_key: String,
    pub description: String,
    pub pending: bool,
}

#[derive(Debug, Clone)]
pub struct Existing {
    pub id: i64,
    pub date: NaiveDate,
    pub amount_cents: Cents,
    pub merchant_key: String,
    pub description: String,
    pub pending: bool,
    /// Deleted by the user: still blocks re-importing the same transaction
    pub user_deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchStatus {
    New,
    /// Already imported: skip
    Duplicate {
        existing_id: i64,
    },
    /// The posted version of a pending transaction: replace it, keeping its tag
    ReplacesPending {
        existing_id: i64,
        amount_changed: bool,
    },
    /// Might be the same as an existing transaction: ask
    PossibleDuplicate {
        existing_id: i64,
    },
}

/// Same merchant if the keys (or, without keys, the raw descriptions) are similar enough.
fn desc_similarity(a: &Incoming, b: &Existing) -> f64 {
    if !a.merchant_key.is_empty() && !b.merchant_key.is_empty() {
        similarity(&a.merchant_key, &b.merchant_key)
    } else {
        similarity(&a.description, &b.description)
    }
}

const SAME_MERCHANT: f64 = 0.6;

/// How far a posted amount may differ from its pending amount (tips, fuel holds).
fn pending_amount_close(pending: Cents, posted: Cents) -> bool {
    let (p, q) = (pending.abs(), posted.abs());
    p <= 100 || (p - q).abs() <= (p * 3 / 10).max(200)
}

pub fn classify(incoming: &[Incoming], existing: &[Existing]) -> Vec<MatchStatus> {
    let mut status = vec![MatchStatus::New; incoming.len()];
    let mut used = vec![false; existing.len()];

    // 1. Exact: same date and amount, same merchant. Prefer an existing row with the same
    //    pending state, so re-pasting the same pending item is a plain duplicate.
    for (i, inc) in incoming.iter().enumerate() {
        let candidates = || {
            existing.iter().enumerate().filter(|(j, e)| {
                !used[*j]
                    && e.date == inc.date
                    && e.amount_cents == inc.amount_cents
                    && desc_similarity(inc, e) >= SAME_MERCHANT
            })
        };
        let pick = candidates()
            .find(|(_, e)| e.pending == inc.pending)
            .or_else(|| candidates().next());
        if let Some((j, e)) = pick {
            used[j] = true;
            status[i] = if e.pending && !inc.pending && !e.user_deleted {
                MatchStatus::ReplacesPending {
                    existing_id: e.id,
                    amount_changed: false,
                }
            } else {
                MatchStatus::Duplicate { existing_id: e.id }
            };
        }
    }

    // 2. Pending -> posted with a changed amount or date (posted up to 5 days later)
    for (i, inc) in incoming.iter().enumerate() {
        if status[i] != MatchStatus::New || inc.pending {
            continue;
        }
        let best = existing
            .iter()
            .enumerate()
            .filter(|(j, e)| {
                let days = (inc.date - e.date).num_days();
                !used[*j]
                    && e.pending
                    && (-1..=5).contains(&days)
                    && e.amount_cents.signum() == inc.amount_cents.signum()
                    && pending_amount_close(e.amount_cents, inc.amount_cents)
                    && desc_similarity(inc, e) >= SAME_MERCHANT
            })
            .max_by(|(_, a), (_, b)| {
                desc_similarity(inc, a)
                    .total_cmp(&desc_similarity(inc, b))
                    .then(
                        (b.amount_cents - inc.amount_cents)
                            .abs()
                            .cmp(&(a.amount_cents - inc.amount_cents).abs()),
                    )
            });
        if let Some((j, e)) = best {
            used[j] = true;
            status[i] = if e.user_deleted {
                MatchStatus::Duplicate { existing_id: e.id }
            } else {
                MatchStatus::ReplacesPending {
                    existing_id: e.id,
                    amount_changed: e.amount_cents != inc.amount_cents,
                }
            };
        }
    }

    // 3. Uncertain: same amount within a day, but the merchant or date doesn't quite match
    for (i, inc) in incoming.iter().enumerate() {
        if status[i] != MatchStatus::New {
            continue;
        }
        let pick = existing.iter().enumerate().find(|(j, e)| {
            let days = (inc.date - e.date).num_days().abs();
            let sim = desc_similarity(inc, e);
            !used[*j]
                && e.amount_cents == inc.amount_cents
                && days <= 1
                && (if days == 0 {
                    sim < SAME_MERCHANT
                } else {
                    sim >= 0.3
                })
        });
        if let Some((j, e)) = pick {
            used[j] = true;
            status[i] = MatchStatus::PossibleDuplicate { existing_id: e.id };
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }
    fn inc(day: u32, amount: Cents, key: &str, pending: bool) -> Incoming {
        Incoming {
            date: d(day),
            amount_cents: amount,
            merchant_key: key.into(),
            description: key.to_uppercase(),
            pending,
        }
    }
    fn ex(id: i64, day: u32, amount: Cents, key: &str, pending: bool) -> Existing {
        Existing {
            id,
            date: d(day),
            amount_cents: amount,
            merchant_key: key.into(),
            description: key.to_uppercase(),
            pending,
            user_deleted: false,
        }
    }

    #[test]
    fn identical_purchases_are_counted_not_collapsed() {
        let incoming = vec![
            inc(14, -525, "example coffee", false),
            inc(14, -525, "example coffee", false),
        ];
        let existing = vec![ex(1, 14, -525, "example coffee", false)];
        let s = classify(&incoming, &existing);
        assert_eq!(
            s,
            vec![MatchStatus::Duplicate { existing_id: 1 }, MatchStatus::New]
        );
    }

    #[test]
    fn pending_replaced_by_posted() {
        let existing = vec![
            ex(1, 10, -4_200, "test bistro", true),
            ex(2, 11, -100, "example fuel", true),
        ];
        let incoming = vec![
            inc(12, -5_040, "test bistro", false),  // tip added
            inc(12, -4_512, "example fuel", false), // fuel hold
        ];
        assert_eq!(
            classify(&incoming, &existing),
            vec![
                MatchStatus::ReplacesPending {
                    existing_id: 1,
                    amount_changed: true
                },
                MatchStatus::ReplacesPending {
                    existing_id: 2,
                    amount_changed: true
                },
            ]
        );
    }

    #[test]
    fn same_pending_pasted_again_is_a_duplicate() {
        let existing = vec![ex(1, 13, -1_840, "test bistro", true)];
        let incoming = vec![inc(13, -1_840, "test bistro", true)];
        assert_eq!(
            classify(&incoming, &existing),
            vec![MatchStatus::Duplicate { existing_id: 1 }]
        );
    }

    #[test]
    fn exact_posted_match_for_pending_replaces_it() {
        let existing = vec![ex(1, 13, -1_840, "test bistro", true)];
        let incoming = vec![inc(13, -1_840, "test bistro", false)];
        assert_eq!(
            classify(&incoming, &existing),
            vec![MatchStatus::ReplacesPending {
                existing_id: 1,
                amount_changed: false
            }]
        );
    }

    #[test]
    fn uncertain_matches_are_flagged() {
        let existing = vec![
            ex(1, 12, -525, "example coffee", false),
            ex(2, 12, -1_999, "example streaming", false),
        ];
        let incoming = vec![
            inc(13, -525, "example coffee shop", false), // a day later
            inc(12, -1_999, "xyz media services", false), // same day, different-looking name
        ];
        assert_eq!(
            classify(&incoming, &existing),
            vec![
                MatchStatus::PossibleDuplicate { existing_id: 1 },
                MatchStatus::PossibleDuplicate { existing_id: 2 }
            ]
        );
    }

    #[test]
    fn different_purchases_are_new() {
        let existing = vec![ex(1, 5, -525, "example coffee", false)];
        let incoming = vec![
            inc(12, -525, "example coffee", false),
            inc(12, -9_000, "example grocery", false),
        ];
        assert_eq!(
            classify(&incoming, &existing),
            vec![MatchStatus::New, MatchStatus::New]
        );
    }

    #[test]
    fn deleted_rows_stay_deleted() {
        let mut gone = ex(1, 12, -525, "example coffee", false);
        gone.user_deleted = true;
        let s = classify(&[inc(12, -525, "example coffee", false)], &[gone]);
        assert_eq!(s, vec![MatchStatus::Duplicate { existing_id: 1 }]);
    }
}
