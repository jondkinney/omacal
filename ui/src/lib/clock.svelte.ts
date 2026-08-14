// The clock format as the rest of the UI reads it.
//
// A module-level rune rather than a prop, and the reason is the shape of the
// tree: `EventBlock` is rendered by `WeekGrid`, `MonthGrid` and `Filmstrip`,
// and `EventPopover` is opened from four places. Threading a display
// preference through every one of those means six components that do not care
// about it acquiring a prop, forwarding it, and being the place it can be
// forgotten. `dismiss.svelte.ts` is here for the same reason: a concern every
// component has and no component owns.
//
// Seeded and kept fresh by `App`, which is the only writer — the same shape as
// `defaultCalendarId`, and for the same reason. A failure to read it leaves
// the 24-hour clock the app has always drawn, silently: refusing to print a
// time because a preference could not be recalled is worse than printing it
// the way it was printed yesterday.
import type { TimeFormat } from './timefmt';

const state = $state<{ format: TimeFormat }>({ format: '24h' });

/** What to print. Read at render time, so a change repaints every clock in
 *  the app without any of them subscribing to anything. */
export const clockFormat = () => state.format;

/** `App` only. Called on startup and after every settings change. */
export const setClockFormat = (f: TimeFormat) => (state.format = f);
