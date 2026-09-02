// The chosen temperature unit as the rest of the UI reads it.
//
// A module-level rune for `clock.svelte.ts`'s reason: `WeekGrid` and
// `Filmstrip` both print a forecast high and neither owns the preference —
// threading it as a prop means two components forwarding a value that is
// really Settings'.
//
// Seeded and kept fresh by `App`, the only writer. A failure to read it
// leaves the Celsius omacal has always drawn.
import type { TemperatureUnit } from './temperature';

const state = $state<{ unit: TemperatureUnit }>({ unit: 'celsius' });

/** What to print. Read at render time, so a change repaints every forecast
 *  high in the app without any of them subscribing to anything. */
export const temperatureUnit = () => state.unit;

/** `App` only. Called on startup and after every settings change. */
export const setTemperatureUnit = (u: TemperatureUnit) => (state.unit = u);
