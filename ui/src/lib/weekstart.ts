// Which day a week begins on, and the three things the UI derives from it.
//
// A plain module rather than part of `settings.ts`, for the reason `views.ts`
// exists: `tsconfig.test.json` compiles no `.svelte`, so anything a spec wants
// to test directly has to live in a `.ts` of its own. The arithmetic here is
// worth testing directly — it is the half that decides where Saturday lands.
//
// The Rust side owns the same three rules for the grids it assembles
// (`settings::WeekStart`); these are the ones the browser needs, and
// `the_ribbons_weekend_stripes_stay_straight_under_every_start` pins the two
// against each other so they cannot drift.

/** The stored preference — the same three spellings `settings::WeekStart`
 *  serialises, so no layer translates. */
export type WeekStartDay = 'monday' | 'sunday' | 'saturday';

/** As `Date.prototype.getDay()` numbers them: Sunday is 0. */
const JS_DAY: Record<WeekStartDay, number> = { sunday: 0, monday: 1, saturday: 6 };

/** Midnight local on the first day of the week containing `d`. */
export function startOfWeek(d: Date, start: WeekStartDay): number {
  const m = new Date(d);
  m.setHours(0, 0, 0, 0);
  // `+ 7` before the modulo because `getDay() - JS_DAY` is negative for more
  // than half the week under a Saturday start, and JavaScript's `%` keeps the
  // sign of its left operand — the classic way this lands a week late.
  m.setDate(m.getDate() - ((m.getDay() - JS_DAY[start] + 7) % 7));
  return m.getTime();
}

/** The seven weekday labels in the order this week draws them, given the
 *  labels in **Monday-first** order — which is how both grids already spell
 *  them, so a caller rotates rather than rewrites. */
export function rotate<T>(mondayFirst: readonly T[], start: WeekStartDay): T[] {
  // Monday-zero, matching the arrays: Sunday sits at index 6, not 0.
  const offset = (JS_DAY[start] + 6) % 7;
  return [...mondayFirst.slice(offset), ...mondayFirst.slice(0, offset)];
}

/**
 * Whether the column at `index` of a week-aligned row is a weekend day.
 *
 * Read off the index and never off the date the column carries — the property
 * Big Year's 28-day rows exist to guarantee. Mirrors
 * `settings::WeekStart::is_weekend_column`, and the Rust suite asserts the two
 * agree for all three starts.
 */
export function isWeekendColumn(index: number, start: WeekStartDay): boolean {
  const weekday = (JS_DAY[start] + index) % 7;
  return weekday === 0 || weekday === 6;
}
