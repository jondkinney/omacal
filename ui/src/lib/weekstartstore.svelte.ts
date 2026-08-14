// The chosen first day of the week, as the rest of the UI reads it.
//
// A module-level rune for the same reason `clock.svelte.ts` is one: `MonthGrid`,
// `YearGrid` and `BigYearRibbon` each need it to draw their headers and their
// weekend shading, and `App` needs it to decide which week `weekStart` means.
// Threading it as a prop means four components forwarding a value none of them
// owns.
//
// Seeded and kept fresh by `App`, the only writer. A failure to read it leaves
// the Monday week omacal has always drawn.
import type { WeekStartDay } from './weekstart';

const state = $state<{ day: WeekStartDay }>({ day: 'monday' });

/** What to draw. Read at render time, so a change repaints every grid. */
export const weekStartDay = () => state.day;

/** `App` only. Called on startup and after every settings change. */
export const setWeekStartDay = (d: WeekStartDay) => (state.day = d);
