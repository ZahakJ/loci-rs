//! Backup naming and rotation policy (recall `src/core/backup/rotation.ts`). Pure functions; no I/O.
//!
//! Names: `loci-YYYYMMDD-HHMMSS-<kind>.sqlite`, timestamp in UTC (unambiguous across DST),
//! kind ∈ auto | manual | pre-restore. Automatic backups are thinned like Anki's
//! `thin_backups`: every automatic backup from today is kept, then the newest backup of
//! each of the most recent 12 days, then of 10 further weeks, then of 9 further months.
//! Manual and pre-restore backups are never deleted.

use crate::types::{BackupKind, Millis, Seconds};
use chrono::{DateTime, Datelike, Timelike};
use regex::Regex;
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackupFileKind {
    Auto,
    Manual,
    PreRestore,
}

impl BackupFileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackupFileKind::Auto => "auto",
            BackupFileKind::Manual => "manual",
            BackupFileKind::PreRestore => "pre-restore",
        }
    }
    /// The `BackupInfo` kind a non-automatic backup is kept as.
    fn as_backup_kind(self) -> Option<BackupKind> {
        match self {
            BackupFileKind::Auto => None,
            BackupFileKind::Manual => Some(BackupKind::Manual),
            BackupFileKind::PreRestore => Some(BackupKind::PreRestore),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupLimits {
    /// distinct days kept (today included)
    pub daily: usize,
    /// further distinct weeks kept (Monday-based)
    pub weekly: usize,
    /// further distinct months kept
    pub monthly: usize,
}

pub const DEFAULT_BACKUP_LIMITS: BackupLimits = BackupLimits { daily: 12, weekly: 10, monthly: 9 };
/// Minimum time between automatic backups (seconds).
pub const BACKUP_MIN_INTERVAL_SECONDS: Seconds = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBackupName {
    pub name: String,
    /// seconds since epoch
    pub at: Seconds,
    pub file_kind: BackupFileKind,
}

/// Returns the UTC offset in minutes WEST of UTC at instant `ms` (`Date#getTimezoneOffset`
/// semantics; `collection/time.ts` `TzOffsetFn`). Injectable so tests and the app can pin a zone.
pub type TzOffsetFn<'a> = &'a dyn Fn(Millis) -> i64;

const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 of the local calendar date of `ms` at the given offset
/// (`collection/time.ts` `localDayIndex`; phase 2 has its own copy with the rest of `time.ts`).
pub(crate) fn local_day_index(ms: Millis, offset_minutes_west: i64) -> i64 {
    (ms - offset_minutes_west * 60_000).div_euclid(DAY_MS)
}

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^loci-(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})-(auto|manual|pre-restore|daily|weekly|monthly)\.sqlite$")
        .expect("valid regex")
});

pub fn backup_file_name(at: Seconds, kind: BackupFileKind) -> String {
    let d = DateTime::from_timestamp(at, 0).unwrap_or_default();
    format!(
        "loci-{:04}{:02}{:02}-{:02}{:02}{:02}-{}.sqlite",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute(),
        d.second(),
        kind.as_str()
    )
}

/// Parses a backup file name; None for anything else. Legacy daily/weekly/monthly tokens count as `Auto`.
/// (rotation.ts:48 used `Date.UTC`, which silently normalises out-of-range fields such as month
/// 13; here such a name is rejected — Loci never writes one.)
pub fn parse_backup_name(name: &str) -> Option<ParsedBackupName> {
    let m = NAME_RE.captures(name)?;
    let n = |i: usize| m[i].parse::<u32>().ok();
    let date = chrono::NaiveDate::from_ymd_opt(n(1)? as i32, n(2)?, n(3)?)?;
    let at = date.and_hms_opt(n(4)?, n(5)?, n(6)?)?.and_utc().timestamp();
    let file_kind = match &m[7] {
        "manual" => BackupFileKind::Manual,
        "pre-restore" => BackupFileKind::PreRestore,
        _ => BackupFileKind::Auto,
    };
    Some(ParsedBackupName { name: name.to_string(), at, file_kind })
}

/// Decides which backups to keep. Returns, per name, the `BackupInfo` kind it is kept as,
/// or None when it should be deleted.
///
/// `now` in seconds; `tz_offset` gives minutes west of UTC at an instant (days, weeks and months
/// are local).
pub fn plan_rotation(
    backups: &[ParsedBackupName],
    now: Seconds,
    tz_offset: TzOffsetFn<'_>,
    limits: BackupLimits,
) -> BTreeMap<String, Option<BackupKind>> {
    let mut out: BTreeMap<String, Option<BackupKind>> = BTreeMap::new();
    let today = local_day_index(now * 1000, tz_offset(now * 1000));
    let mut kept_days: HashSet<i64> = HashSet::new();
    let mut kept_weeks: HashSet<i64> = HashSet::new();
    let mut kept_months: HashSet<i64> = HashSet::new();
    let mut weekly = 0usize;
    let mut monthly = 0usize;
    for b in backups {
        if let Some(kind) = b.file_kind.as_backup_kind() {
            out.insert(b.name.clone(), Some(kind));
        }
    }
    // Newest first; equal timestamps sort by name descending (rotation.ts:70).
    let mut auto: Vec<&ParsedBackupName> = backups.iter().filter(|b| b.file_kind == BackupFileKind::Auto).collect();
    auto.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.name.cmp(&a.name)));
    for b in auto {
        let ms = b.at * 1000;
        let offset = tz_offset(ms);
        let day = local_day_index(ms, offset);
        // 1970-01-01 was a Thursday; +3 makes weeks start on Monday.
        let week = (day + 3).div_euclid(7);
        let local = DateTime::from_timestamp(day * 86_400, 0).unwrap_or_default();
        let month = local.year() as i64 * 12 + local.month0() as i64;
        // Today's backups are all kept; older days take the daily slots newest first.
        let kind = if day >= today || (!kept_days.contains(&day) && kept_days.len() < limits.daily) {
            kept_days.insert(day);
            kept_weeks.insert(week);
            kept_months.insert(month);
            Some(BackupKind::Daily)
        } else if !kept_days.contains(&day) && !kept_weeks.contains(&week) && weekly < limits.weekly {
            weekly += 1;
            kept_weeks.insert(week);
            kept_months.insert(month);
            Some(BackupKind::Weekly)
        } else if !kept_days.contains(&day)
            && !kept_weeks.contains(&week)
            && !kept_months.contains(&month)
            && monthly < limits.monthly
        {
            monthly += 1;
            kept_months.insert(month);
            Some(BackupKind::Monthly)
        } else {
            None
        };
        out.insert(b.name.clone(), kind);
    }
    out
}

/// The 30-minute rule: back up when something changed after the last backup and at least
/// 30 minutes have passed since it. All arguments in seconds; None = never.
pub fn should_backup(last_backup: Option<Seconds>, last_change: Option<Seconds>, now: Seconds) -> bool {
    let Some(change) = last_change else { return false };
    let Some(backup) = last_backup else { return true };
    change > backup && now - backup >= BACKUP_MIN_INTERVAL_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(hh, mm, 0).unwrap().and_utc().timestamp()
    }
    fn auto(at: i64) -> ParsedBackupName {
        ParsedBackupName { name: backup_file_name(at, BackupFileKind::Auto), at, file_kind: BackupFileKind::Auto }
    }
    fn iso(at: i64) -> String {
        DateTime::from_timestamp(at, 0).unwrap().format("%Y-%m-%d").to_string()
    }
    fn utc(_ms: i64) -> i64 {
        0
    }

    #[test]
    fn round_trips_names_utc_timestamp() {
        let at = sec(2026, 3, 11, 9, 5) + 7;
        let name = backup_file_name(at, BackupFileKind::PreRestore);
        assert_eq!(name, "loci-20260311-090507-pre-restore.sqlite");
        assert_eq!(
            parse_backup_name(&name),
            Some(ParsedBackupName { name: name.clone(), at, file_kind: BackupFileKind::PreRestore })
        );
        assert_eq!(parse_backup_name("loci-20260311-090507-weekly.sqlite").map(|p| p.file_kind), Some(BackupFileKind::Auto));
        assert_eq!(parse_backup_name("collection.sqlite"), None);
        assert_eq!(parse_backup_name("loci-20261311-090507-auto.sqlite"), None);
    }

    #[test]
    fn keeps_exactly_the_expected_backups_for_a_daily_history() {
        let now = sec(2026, 3, 11, 15, 0); // Wednesday
        let mut backups: Vec<ParsedBackupName> = Vec::new();
        // One backup per day at noon for 500 days, plus extra ones today and yesterday.
        for i in 0..500 {
            backups.push(auto(sec(2026, 3, 11, 12, 0) - i * 86400));
        }
        backups.push(auto(sec(2026, 3, 11, 8, 0)));
        backups.push(auto(sec(2026, 3, 11, 14, 0)));
        backups.push(auto(sec(2026, 3, 10, 8, 0)));
        let manual_old = ParsedBackupName {
            name: backup_file_name(sec(2024, 1, 1, 12, 0), BackupFileKind::Manual),
            at: sec(2024, 1, 1, 12, 0),
            file_kind: BackupFileKind::Manual,
        };
        let pre_restore = ParsedBackupName {
            name: backup_file_name(sec(2023, 6, 1, 12, 0), BackupFileKind::PreRestore),
            at: sec(2023, 6, 1, 12, 0),
            file_kind: BackupFileKind::PreRestore,
        };
        backups.push(manual_old.clone());
        backups.push(pre_restore.clone());
        let plan = plan_rotation(&backups, now, &utc, DEFAULT_BACKUP_LIMITS);

        let kept = |kind: BackupKind| {
            let mut v: Vec<String> =
                plan.iter().filter(|(_, k)| **k == Some(kind)).map(|(n, _)| iso(parse_backup_name(n).unwrap().at)).collect();
            v.sort();
            v.reverse();
            v
        };
        // Today: all three kept; other days: newest only.
        let daily = kept(BackupKind::Daily);
        assert_eq!(daily.iter().filter(|d| *d == "2026-03-11").count(), 3);
        let mut distinct = daily.clone();
        distinct.dedup();
        assert_eq!(
            distinct,
            [
                "2026-03-11",
                "2026-03-10",
                "2026-03-09",
                "2026-03-08",
                "2026-03-07",
                "2026-03-06",
                "2026-03-05",
                "2026-03-04",
                "2026-03-03",
                "2026-03-02",
                "2026-03-01",
                "2026-02-28",
            ]
        );
        assert_eq!(daily.iter().filter(|d| *d == "2026-03-10").count(), 1);
        assert_eq!(
            kept(BackupKind::Weekly),
            [
                "2026-02-22",
                "2026-02-15",
                "2026-02-08",
                "2026-02-01",
                "2026-01-25",
                "2026-01-18",
                "2026-01-11",
                "2026-01-04",
                "2025-12-28",
                "2025-12-21"
            ]
        );
        assert_eq!(
            kept(BackupKind::Monthly),
            [
                "2025-11-30",
                "2025-10-31",
                "2025-09-30",
                "2025-08-31",
                "2025-07-31",
                "2025-06-30",
                "2025-05-31",
                "2025-04-30",
                "2025-03-31"
            ]
        );
        assert_eq!(plan.get(&manual_old.name), Some(&Some(BackupKind::Manual)));
        assert_eq!(plan.get(&pre_restore.name), Some(&Some(BackupKind::PreRestore)));
        let deleted = plan.values().filter(|k| k.is_none()).count();
        assert_eq!(deleted, 503 - 3 - 11 - 10 - 9);
    }

    #[test]
    fn sparse_history_each_backup_takes_the_finest_free_slot() {
        let now = sec(2026, 3, 11, 12, 0);
        let plan = plan_rotation(
            &[auto(sec(2026, 3, 1, 12, 0)), auto(sec(2025, 12, 1, 12, 0)), auto(sec(2025, 11, 30, 12, 0))],
            now,
            &utc,
            DEFAULT_BACKUP_LIMITS,
        );
        assert_eq!(plan.values().cloned().collect::<Vec<_>>(), [Some(BackupKind::Daily); 3]);
    }

    #[test]
    fn uses_local_days_utc_plus_10() {
        // 2026-03-10 20:00 UTC is 2026-03-11 06:00 at UTC+10, i.e. "today" locally.
        let now = sec(2026, 3, 11, 2, 0);
        let a = auto(sec(2026, 3, 10, 20, 0));
        let b = auto(sec(2026, 3, 10, 13, 0)); // local 23:00 on the 10th
        let c = auto(sec(2026, 3, 10, 12, 0)); // local 22:00 on the 10th → same day as b, older → deleted
        let plan = plan_rotation(&[a.clone(), b.clone(), c.clone()], now, &|_| -600, DEFAULT_BACKUP_LIMITS);
        assert_eq!(plan.get(&a.name), Some(&Some(BackupKind::Daily)));
        assert_eq!(plan.get(&b.name), Some(&Some(BackupKind::Daily)));
        assert_eq!(plan.get(&c.name), Some(&None));
    }

    #[test]
    fn thirty_minute_rule() {
        assert!(!should_backup(None, None, 1000));
        assert!(should_backup(None, Some(500), 1000));
        assert!(!should_backup(Some(1000), Some(900), 5000));
        assert!(!should_backup(Some(1000), Some(1000), 5000));
        assert!(!should_backup(Some(1000), Some(1001), 1000 + 1799));
        assert!(should_backup(Some(1000), Some(1001), 1000 + 1800));
    }

    #[test]
    fn local_day_index_floors_before_the_epoch() {
        assert_eq!(local_day_index(0, 0), 0);
        assert_eq!(local_day_index(-1, 0), -1);
        assert_eq!(local_day_index(DAY_MS - 1, 0), 0);
        assert_eq!(local_day_index(DAY_MS, 60), 0);
    }
}
