/**
 * UI-side interval formatting (recall `scheduler/convert.ts` `formatInterval`). The scheduler
 * itself is the Rust engine; the UI only turns seconds into labels like '<1m', '3d', '1.2mo'.
 */
const MINUTE = 60;
const HOUR = 3600;
const DAY = 86_400;
const MONTH = 30.417 * DAY;
const YEAR = 365 * DAY;

/**
 * Interval label for answer buttons, the study screen's announcement (which parses this label)
 * and card info: '<1m' under a minute, whole minutes ('6m', '10m'), hours to one decimal
 * ('1.5h'), whole days ('10d'), months and years to one decimal ('1.2mo', '1.5y'). Unlike
 * Anki's `answer_button_time` there is no '<' prefix for learn-ahead and no fractional minutes
 * ('<5.5m' read as "less than five and a half minutes" for a step that is simply six minutes).
 * `_collapseSeconds` is accepted for compatibility and ignored.
 */
export function formatInterval(seconds: number, _collapseSeconds = 0): string {
  const secs = Math.abs(seconds);
  if (secs < MINUTE) return '<1m';
  const minutes = Math.round(secs / MINUTE);
  if (minutes < 60) return `${minutes}m`;
  let unit: string, amount: number;
  if (secs < DAY) { unit = 'h'; amount = Math.round((secs / HOUR) * 10) / 10; if (amount >= 24) { unit = 'd'; amount = 1; } }
  else if (secs < MONTH) { unit = 'd'; amount = Math.round(secs / DAY); }
  else if (secs < YEAR) { unit = 'mo'; amount = Math.round((secs / MONTH) * 10) / 10; }
  else { unit = 'y'; amount = Math.round((secs / YEAR) * 10) / 10; }
  return `${amount}${unit}`;
}
