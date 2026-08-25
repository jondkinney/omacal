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

/**
 * A stored `HH:MM` as the clock the user chose — what the event form's time
 * fields *show*.
 *
 * The storage form never changes: `EventFormValue.start`/`.end` are `HH:MM`
 * everywhere below the form, and in 24h mode this is the identity. It exists
 * because the native `<input type="time">` this replaced rendered per the
 * *system locale* — an AM/PM form on a grid drawn in 24h (reported
 * 2026-08-25), with no attribute to say otherwise; the engine's clock is not
 * the app's clock.
 */
export function displayClock(hhmm: string, format: TimeFormat): string {
  const m = /^(\d{2}):(\d{2})$/.exec(hhmm);
  if (!m || format === '24h') return hhmm;
  const h = Number(m[1]);
  return `${dial(h)}:${m[2]} ${h < 12 ? 'AM' : 'PM'}`;
}

/**
 * Typed time → stored `HH:MM`, or `null` for anything that is not a time.
 *
 * Deliberately generous on the way in — `9`, `9:30`, `21:30`, `9:30 pm`,
 * `9.30pm`, `12am` all land — because a form field is where people type,
 * not where they conform. The rules are a clock's own: a suffix makes the
 * hour a dial hour (1–12, `12am` = midnight, `12pm` = noon); no suffix
 * makes it a 24-hour reading (0–23). Minutes are two digits or absent —
 * `9:5` is a typo, not five past nine, and guessing would store the guess.
 */
export function parseClock(text: string): string | null {
  const m = /^\s*(\d{1,2})(?:[:.](\d{2}))?\s*(?:([ap])\.?\s*m?\.?)?\s*$/i.exec(text);
  if (!m) return null;
  let h = Number(m[1]);
  const min = m[2] === undefined ? 0 : Number(m[2]);
  const suffix = m[3]?.toLowerCase();
  if (min > 59) return null;
  if (suffix) {
    if (h < 1 || h > 12) return null;
    if (suffix === 'a') h = h === 12 ? 0 : h;
    else h = h === 12 ? 12 : h + 12;
  } else if (h > 23) {
    return null;
  }
  return `${pad2(h)}:${pad2(min)}`;
}
