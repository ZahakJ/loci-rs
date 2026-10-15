//! Statistics and insights: the Stats DTO computed with SQL over
//! `revlog` and `cards` (`stats.rs`), the insight rules (`insights.rs`) and the sentence helpers
//! (`messages.rs`, thin wrappers over `i18n`).

pub mod insights;
pub mod messages;
#[allow(clippy::module_inception)]
pub mod stats;

pub use insights::{compute_insights, session_insight, STRUGGLING_QUERY};
pub use messages::{msg, pct};
pub use stats::{
    desired_retention_for, iso_date, momentum, revlog_scope, stats, studied_days, true_retention, RevlogScope, FORECAST_DAYS,
    HISTORY_DAYS, INTERVAL_BUCKETS, STABILITY_BUCKETS,
};
