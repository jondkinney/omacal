// The second time zone as the rest of the UI reads it.
//
// A module-level rune for `clock.svelte.ts`'s reason exactly: the readers —
// the Week and Day gutter, the event form's convenience line — sit in
// different subtrees, and none of them owns the preference. `null` is the
// feature off, which is the fresh-install state.
//
// Seeded and kept fresh by `App`, the only writer. A failure to read it
// leaves the second clock off, silently: a convenience that could not be
// recalled is a convenience absent, not an error worth a banner.

const state = $state<{ zone: string | null }>({ zone: null });

/** The IANA zone to show beside times, or `null` for off. Read at render
 *  time, so a settings change repaints every second clock without any of
 *  them subscribing to anything. */
export const secondZone = () => state.zone;

/** `App` only. Called on startup and after every settings change. */
export const setSecondZone = (z: string | null) => (state.zone = z);

/** The hour ruler's column width — which is also the all-day band's left
 *  offset and the header row's first column, so it lives here, once, where
 *  all three read it and none can drift. Wider when the second clock needs
 *  a lane of its own. */
export const gutterWidth = () => (state.zone ? '104px' : '44px');
