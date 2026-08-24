import { invoke } from '@tauri-apps/api/core';

/** One day of forecast, exactly as `weather::DayWeather` serializes it. The
 *  `bucket` is the icon family — the backend ports the Omarchy widget's
 *  code grouping, so the bar and the calendar never tell two stories about
 *  one sky; this side only picks a drawing. */
export type DayWeather = {
  /** ISO date, local to the forecast's location. */
  date: string;
  bucket: string;
  tmax: number;
  tmin: number;
};

export type WeatherReport = {
  days: DayWeather[];
  place: string | null;
};

/** Empty `days` is the whole failure/off contract: no days, no icons, and
 *  never an error surface — weather is decoration. */
export const getWeather = () => invoke<WeatherReport>('get_weather');

/** The report as the headers look it up: by the day's own ISO date. */
export const weatherByDate = (report: WeatherReport | null): Map<string, DayWeather> =>
  new Map((report?.days ?? []).map((d) => [d.date, d]));

/** A day-start instant's ISO date in the *display* zone — the webview's own,
 *  fixed at launch, which is the zone the grid's columns are built in. The
 *  forecast's dates are local to the forecast's location; for the case this
 *  feature exists for — you, where you are — those are the same calendar
 *  dates, and where they differ (viewing from a far zone) a missed lookup
 *  draws nothing rather than the wrong sky. */
export const dateKey = (ms: number): string => {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};
