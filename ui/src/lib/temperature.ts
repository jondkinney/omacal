// Printing a temperature — the one place in the app that rounds a Celsius
// reading and decides whether it prints as `72°` or `22°`.
//
// The backend (`weather::DayWeather`) carries the forecast unrounded and
// always in Celsius, deliberately: rounding once, here, in whichever unit
// this prints, is the only way `31.6°C` doesn't become `90°F` when the true
// converted value is `89°F`. Converting an already-rounded `32°C` gets the
// wrong answer about one time in two.

/** The stored preference. The same two spellings the settings table holds and
 *  `settings::TemperatureUnit` serialises, so no layer translates. */
export type TemperatureUnit = 'celsius' | 'fahrenheit';

/** A raw Celsius reading, rounded to the whole degree the header prints —
 *  a day header saying `31.6°` is a header showing off, in either unit. */
export function formatTemp(celsius: number, unit: TemperatureUnit): string {
  const value = unit === 'fahrenheit' ? celsius * (9 / 5) + 32 : celsius;
  return `${Math.round(value)}`;
}

/** A wind speed the way the card prints it beside the chosen temperature
 *  unit: km/h with Celsius, mph with Fahrenheit, rounded once here from the
 *  km/h the forecast carries. */
export function formatWind(kmh: number, unit: TemperatureUnit): string {
  return unit === 'fahrenheit' ? `${Math.round(kmh / 1.609344)} mph` : `${Math.round(kmh)} km/h`;
}
