//! Revlog → FSRS training/scoring items (Anki `reviews_for_fsrs`, research/fsrs-spec.md §5.3).

use super::fsrs::Grade;
use crate::types::{RevlogEntry, RevlogType};
use std::collections::BTreeMap;

/// One review inside an item: rating 1–4 and whole scheduler days since the previous review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsrsReview {
    pub rating: Grade,
    pub delta_t: i64,
}

impl FsrsReview {
    pub fn new(rating: Grade, delta_t: i64) -> Self {
        FsrsReview { rating, delta_t }
    }
}

/// An FSRS item: a review history whose last review is the one being predicted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FsrsItem {
    pub reviews: Vec<FsrsReview>,
    /// Revlog id (ms) of the last review, for chronological ordering.
    pub last_review_ms: Option<i64>,
    /// Card the item came from (optional, informational).
    pub card_id: Option<i64>,
}

impl FsrsItem {
    pub fn new(reviews: Vec<FsrsReview>) -> Self {
        FsrsItem { reviews, last_review_ms: None, card_id: None }
    }
    /// Shorthand for tests and synthetic data: `(rating, delta_t)` pairs.
    pub fn from_pairs(pairs: &[(Grade, i64)]) -> Self {
        FsrsItem::new(pairs.iter().map(|&(r, t)| FsrsReview::new(r, t)).collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewsForFsrs {
    /// Kept revlog entries (graded, not cramming), oldest first.
    pub entries: Vec<RevlogEntry>,
    /// Same length as entries. `reviews[0].delta_t == 0`.
    pub reviews: Vec<FsrsReview>,
    /// True when the history starts at a learning step (replay from scratch is exact).
    pub revlogs_complete: bool,
    /// Training: one item per kept review with idx ≥ 1 and delta_t > 0; scoring: one item, the full history.
    pub items: Vec<FsrsItem>,
}

/// Anki "Reset": manual row with factor 0.
pub fn is_reset(e: &RevlogEntry) -> bool {
    e.rtype == RevlogType::Manual && e.factor == 0
}
/// Anki "Cramming": filtered row with factor 0.
pub fn is_cramming(e: &RevlogEntry) -> bool {
    e.rtype == RevlogType::Filtered && e.factor == 0
}
pub fn has_rating(e: &RevlogEntry) -> bool {
    (1..=4).contains(&e.ease)
}
pub fn affects_scheduling(e: &RevlogEntry) -> bool {
    has_rating(e) && !is_cramming(e)
}

/// Builds FSRS reviews for one card's revlog (`input` oldest first; it is sorted by id
/// defensively). `next_day_at_ms` is Anki's `next_day_at`; days elapsed for an entry are
/// floor((next_day_at − entry)/86400 s). `None` when the card has no usable history.
pub fn reviews_for_fsrs(
    input: &[RevlogEntry],
    next_day_at_ms: i64,
    training: bool,
    ignore_before_ms: i64,
) -> Option<ReviewsForFsrs> {
    let mut entries = input.to_vec();
    entries.sort_by_key(|e| e.id);
    let mut first_of_last_learn: Option<usize> = None;
    let mut first_user_grade: Option<usize> = None;
    let mut revlogs_complete = false;
    for i in (0..entries.len()).rev() {
        let e = &entries[i];
        if is_cramming(e) {
            continue;
        }
        let within_cutoff = e.id > ignore_before_ms;
        let graded = has_rating(e);
        let interday = e.ivl >= 1 || e.ivl <= -86400;
        if graded && within_cutoff && interday {
            first_user_grade = Some(i);
        }
        if graded && e.rtype == RevlogType::Learn {
            first_of_last_learn = Some(i);
            revlogs_complete = true;
        } else if is_reset(e) {
            if first_of_last_learn.is_some() {
                revlogs_complete = true;
                break;
            }
            if first_user_grade.is_some() {
                revlogs_complete = false;
                break;
            }
            return None;
        } else if first_of_last_learn.is_some() {
            break;
        }
    }
    if training {
        if let Some(f) = first_of_last_learn {
            if entries[f].id < ignore_before_ms {
                return None;
            }
        }
    } else if let Some(f) = first_of_last_learn {
        if entries[f].id < ignore_before_ms && f < entries.len() - 1 {
            revlogs_complete = false;
            first_of_last_learn = first_user_grade;
        }
    }
    let start = first_of_last_learn.or(if training { None } else { first_user_grade })?;
    let kept: Vec<RevlogEntry> = entries[start..].iter().filter(|e| affects_scheduling(e)).cloned().collect();
    if kept.is_empty() {
        return None;
    }
    let days_elapsed = |e: &RevlogEntry| ((next_day_at_ms - e.id).div_euclid(86_400_000)).max(0);
    let reviews: Vec<FsrsReview> = kept
        .iter()
        .enumerate()
        .map(|(i, e)| FsrsReview {
            rating: e.ease as Grade,
            delta_t: if i == 0 { 0 } else { days_elapsed(&kept[i - 1]) - days_elapsed(e) },
        })
        .collect();
    let mut items = Vec::new();
    if training {
        for i in 1..reviews.len() {
            if reviews[i].delta_t > 0 {
                items.push(FsrsItem {
                    reviews: reviews[..=i].to_vec(),
                    last_review_ms: Some(kept[i].id),
                    card_id: Some(kept[i].cid),
                });
            }
        }
    } else {
        items.push(FsrsItem {
            reviews: reviews.clone(),
            last_review_ms: Some(kept[kept.len() - 1].id),
            card_id: Some(kept[0].cid),
        });
    }
    Some(ReviewsForFsrs { entries: kept, reviews, revlogs_complete, items })
}

/// Training items for a whole preset: groups `revlog` by card, builds items per card and
/// sorts all items chronologically by their last review (fsrs-spec §6.1 step 1).
pub fn training_items_from_revlog(revlog: &[RevlogEntry], next_day_at_ms: i64, ignore_before_ms: i64) -> Vec<FsrsItem> {
    let mut by_card: BTreeMap<i64, Vec<RevlogEntry>> = BTreeMap::new();
    for e in revlog {
        by_card.entry(e.cid).or_default().push(e.clone());
    }
    let mut out = Vec::new();
    for list in by_card.values() {
        if let Some(r) = reviews_for_fsrs(list, next_day_at_ms, true, ignore_before_ms) {
            out.extend(r.items);
        }
    }
    out.sort_by_key(|it| it.last_review_ms.unwrap_or(0));
    out
}

/// Number of long-term (delta_t > 0) reviews in an item.
pub fn long_term_review_count(item: &FsrsItem) -> usize {
    item.reviews.iter().filter(|r| r.delta_t > 0).count()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const DAY: i64 = 86_400_000;
    pub(crate) const NEXT_DAY_AT: i64 = 86_400 * 1000 * 1000;

    pub(crate) fn entry(days_ago: i64, rtype: RevlogType, ease: i64) -> RevlogEntry {
        RevlogEntry {
            id: NEXT_DAY_AT - days_ago * DAY - DAY / 2,
            cid: 1,
            usn: 0,
            ease,
            ivl: if rtype == RevlogType::Learn { -600 } else { 1 },
            last_ivl: 0,
            factor: 2500,
            time: 1000,
            rtype,
        }
    }
    fn deltas(items: &[FsrsItem]) -> Vec<Vec<i64>> {
        items.iter().map(|it| it.reviews.iter().map(|r| r.delta_t).collect()).collect()
    }

    #[test]
    fn learning_plus_reviews() {
        let r = reviews_for_fsrs(
            &[
                entry(15, RevlogType::Learn, 3),
                entry(13, RevlogType::Learn, 3),
                entry(10, RevlogType::Review, 3),
                entry(5, RevlogType::Review, 3),
            ],
            NEXT_DAY_AT,
            true,
            0,
        )
        .unwrap();
        assert_eq!(deltas(&r.items), vec![vec![0, 2], vec![0, 2, 3], vec![0, 2, 3, 5]]);
        assert!(r.revlogs_complete);
    }

    #[test]
    fn cramming_entries_are_skipped() {
        let mut cram = entry(7, RevlogType::Filtered, 3);
        cram.factor = 0;
        let r = reviews_for_fsrs(
            &[entry(10, RevlogType::Learn, 3), entry(9, RevlogType::Review, 3), cram, entry(4, RevlogType::Review, 3)],
            NEXT_DAY_AT,
            true,
            0,
        )
        .unwrap();
        assert_eq!(deltas(&r.items), vec![vec![0, 1], vec![0, 1, 5]]);
    }

    #[test]
    fn single_learning_entry_none_when_training_single_item_when_scoring() {
        let r = reviews_for_fsrs(&[entry(1, RevlogType::Learn, 3)], NEXT_DAY_AT, true, 0);
        assert!(r.map(|r| r.items).unwrap_or_default().is_empty());
        let s = reviews_for_fsrs(&[entry(1, RevlogType::Learn, 3)], NEXT_DAY_AT, false, 0).unwrap();
        assert_eq!(deltas(&s.items), vec![vec![0]]);
    }

    #[test]
    fn a_reset_cuts_history_and_manual_rows_without_rating_are_dropped() {
        let mut reset = entry(15, RevlogType::Manual, 0);
        reset.factor = 0;
        let list = vec![
            entry(30, RevlogType::Learn, 3),
            entry(20, RevlogType::Review, 3),
            reset.clone(),
            entry(10, RevlogType::Learn, 3),
            entry(3, RevlogType::Review, 3),
            entry(2, RevlogType::Rescheduled, 0),
        ];
        let r = reviews_for_fsrs(&list, NEXT_DAY_AT, true, 0).unwrap();
        assert_eq!(deltas(&r.items), vec![vec![0, 7]]);
        assert!(reviews_for_fsrs(&[entry(30, RevlogType::Learn, 3), reset], NEXT_DAY_AT, false, 0).is_none());
    }

    #[test]
    fn training_needs_a_learning_block_scoring_falls_back_to_first_interday_grade() {
        let mut a = entry(20, RevlogType::Review, 3);
        a.ivl = 10;
        let mut b = entry(5, RevlogType::Review, 3);
        b.ivl = 30;
        let list = vec![a, b];
        assert!(reviews_for_fsrs(&list, NEXT_DAY_AT, true, 0).is_none());
        let s = reviews_for_fsrs(&list, NEXT_DAY_AT, false, 0).unwrap();
        assert!(!s.revlogs_complete);
        assert_eq!(deltas(&s.items), vec![vec![0, 15]]);
    }

    #[test]
    fn items_for_many_cards_are_chronological() {
        let a = vec![entry(15, RevlogType::Learn, 3), entry(5, RevlogType::Review, 3)];
        let b: Vec<RevlogEntry> = vec![entry(12, RevlogType::Learn, 3), entry(8, RevlogType::Review, 3)]
            .into_iter()
            .map(|mut e| {
                e.cid = 2;
                e
            })
            .collect();
        let all: Vec<RevlogEntry> = a.into_iter().chain(b).collect();
        let items = training_items_from_revlog(&all, NEXT_DAY_AT, 0);
        assert_eq!(items.iter().map(|i| i.card_id.unwrap()).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(long_term_review_count(&items[0]), 1);
    }
}
