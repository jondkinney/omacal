// Printing a clock — the one place in the app that decides whether an instant
// reads as `13:30` or as `1:30 PM`.
//
// This existed three times before it existed once: `EventBlock`, `Filmstrip`
// and `EventPopover` each carried the same two-line `hhmm`, and the Week and
// Day gutter carried a fourth spelling of the same idea. Three copies of a
// constant are three places to change and two places to forget, which is
// survivable while the constant is `hour12: false` forever and stops being so
// the moment it becomes a preference.
//
// **Built by hand rather than by `Intl`, deliberately.** The old `hhmm` asked
// `toLocaleTimeString` for a fixed 24-hour clock, which works because every
// locale that matters renders that the same way — but the 12-hour side has no
// such luck (`1:30 PM`, `1:30 pm`, `13:30` in a locale that ignores the flag),
// and the *screenshot goldens* would then depend on the ICU data of whichever
// machine rendered them. The testing standard has an open failure of exactly
// that shape already (`ruleInWords`, §6). Two dozen strings are cheaper than a
// second one.

/** The stored preference. The same two spellings the settings table holds and
 *  `settings::TimeFormat` serialises, so no layer translates. */
export type TimeFormat = '12h' | '24h';

const pad2 = (n: number) => String(n).padStart(2, '0');

/** The hour on a 12-hour dial: midnight and noon are both 12, never 0. The
 *  case every hand-rolled 12-hour clock gets wrong. */
const dial = (hour24: number) => (hour24 % 12 === 0 ? 12 : hour24 % 12);

/**
 * An instant as a clock time, in the *browser's* zone — the same zone
 * `toLocaleTimeString` read, and the same one the grid lays events out in.
 *
 * `1:30 PM` and not `01:30 PM`: a leading zero on a 12-hour clock is something
 * no clock face and no other calendar does, and it is what `hour: '2-digit'`
 * would have given us for free had this stayed with `Intl`.
 */
export function formatClock(ms: number, format: TimeFormat): string {
  const d = new Date(ms);
  const h = d.getHours();
  const m = pad2(d.getMinutes());
  return format === '12h' ? `${dial(h)}:${m} ${h < 12 ? 'AM' : 'PM'}` : `${pad2(h)}:${m}`;
}

/**
 * One label on the Week and Day hour ruler, given the hour 0-23.
 *
 * The 12-hour form is compact — `12a`, `1p` — rather than `12 AM`, because the
 * gutter is a fixed-width column beside the grid and the long form is nearly
 * three times the glyphs of the `00`-`23` it replaces. A ruler wide enough to
 * spell "AM" twenty-four times is a ruler taking space from the calendar.
 */
export function gutterLabel(hour24: number, format: TimeFormat): string {
  return format === '12h' ? `${dial(hour24)}${hour24 < 12 ? 'a' : 'p'}` : pad2(hour24);
}
