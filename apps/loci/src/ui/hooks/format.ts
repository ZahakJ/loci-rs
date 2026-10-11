/** Pure formatting helpers shared by shell views (tested). Strings go through t(). */
import { getLocale, t } from '@core/i18n/i18n';

/** "about 14 min" style duration from minutes. */
export function formatMinutes(min: number): string {
  const m = Math.max(0, Math.round(min));
  if (m < 1) return t('time.lessThanMinute');
  if (m < 60) return t('time.minutes', { count: m });
  const h = Math.floor(m / 60), r = m % 60;
  return r === 0 ? t('time.hours', { count: h }) : t('time.hoursMinutes', { h, m: r });
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB'];
  let v = bytes / 1024, i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toLocaleString(getLocale(), { maximumFractionDigits: v < 10 ? 1 : 0 })} ${units[i]}`;
}

/** "today", "yesterday", "3 days ago", "2 weeks ago"… relative to `now` (both in seconds). */
export function formatRelative(at: number, now: number = Date.now() / 1000): string {
  const rtf = new Intl.RelativeTimeFormat(getLocale(), { numeric: 'auto' });
  const diff = at - now;
  const abs = Math.abs(diff);
  if (abs < 60) return rtf.format(0, 'second');
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour');
  if (abs < 86400 * 14) return rtf.format(Math.round(diff / 86400), 'day');
  if (abs < 86400 * 60) return rtf.format(Math.round(diff / (86400 * 7)), 'week');
  if (abs < 86400 * 365) return rtf.format(Math.round(diff / (86400 * 30)), 'month');
  return rtf.format(Math.round(diff / (86400 * 365)), 'year');
}

export function formatDateTime(at: number): string {
  return new Intl.DateTimeFormat(getLocale(), { dateStyle: 'medium', timeStyle: 'short' }).format(at * 1000);
}

export function formatHour(h: number): string {
  return new Intl.DateTimeFormat(getLocale(), { hour: 'numeric', timeZone: 'UTC' }).format(Date.UTC(2024, 0, 1, h));
}

export function formatPercent(x: number, digits = 0): string {
  return new Intl.NumberFormat(getLocale(), { style: 'percent', maximumFractionDigits: digits, minimumFractionDigits: digits }).format(x);
}

export function formatCount(n: number): string { return n.toLocaleString(getLocale()); }

/** Parses Anki-style step text ("1m 10m 1h 1d") into minutes; returns null when invalid. */
export function parseSteps(text: string): number[] | null {
  const parts = text.trim().split(/[\s,]+/).filter(Boolean);
  const out: number[] = [];
  for (const p of parts) {
    const m = /^(\d+(?:\.\d+)?)(s|m|h|d)?$/i.exec(p);
    if (!m) return null;
    const v = Number(m[1]);
    const unit = (m[2] ?? 'm').toLowerCase();
    const mins = unit === 's' ? v / 60 : unit === 'h' ? v * 60 : unit === 'd' ? v * 1440 : v;
    if (!(mins > 0)) return null;
    out.push(Math.round(mins * 1000) / 1000);
  }
  return out;
}

/** Inverse of parseSteps: minutes → "1m 10m 1h 1d". */
export function formatSteps(steps: number[]): string {
  return steps.map((m) => {
    if (m < 1) return `${Math.round(m * 60)}s`;
    if (m >= 1440 && m % 1440 === 0) return `${m / 1440}d`;
    if (m >= 60 && m % 60 === 0) return `${m / 60}h`;
    return `${m}m`;
  }).join(' ');
}

/** YYYY-MM-DD in the user's own time zone (for file names: a backup made at 1am is today's). */
export function localDateStamp(d: Date = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Today's date as a heading ("Tuesday 6 October"). */
export function formatToday(d: Date = new Date()): string {
  return new Intl.DateTimeFormat(getLocale(), { weekday: 'long', day: 'numeric', month: 'long' }).format(d);
}
