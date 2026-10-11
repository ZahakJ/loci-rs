//! Ports of recall `tests/unit/scheduler/*.test.ts` that need the shared helpers
//! (`helpers.ts`): `make_config` / `make_card`. Smaller unit vectors live next to their code.

mod optimizer;
mod queue;
mod states;

use crate::collection::presets::standard_deck_config;
use crate::types::*;

/// `helpers.ts` `makeConfig`: the Standard preset with 200 reviews/day and no sibling burying.
pub(crate) fn make_config() -> DeckConfig {
    DeckConfig {
        reviews_per_day: 200,
        bury_siblings_new: false,
        bury_siblings_review: false,
        bury_siblings_interday: false,
        ..standard_deck_config()
    }
}

/// `helpers.ts` `makeCard`: a new card with id/nid 1_700_000_000_000 in deck 1, due 1.
pub(crate) fn make_card() -> Card {
    Card {
        id: 1_700_000_000_000,
        nid: 1_700_000_000_000,
        did: 1,
        ord: 0,
        mtime: 0,
        usn: 0,
        ctype: CardType::New,
        queue: CardQueue::New,
        due: 1,
        ivl: 0,
        factor: 0,
        reps: 0,
        lapses: 0,
        left: 0,
        odue: 0,
        odid: 0,
        flags: 0,
        data: CardData::default(),
    }
}

/// Local-time instant in ms (the TS tests built `new Date(y, m, d, h, mi)`).
pub(crate) fn local_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
    use chrono::TimeZone;
    chrono::Local.with_ymd_and_hms(y, mo, d, h, mi, 0).single().expect("valid local time").timestamp_millis()
}
