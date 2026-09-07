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
  /** The day card's extras, each absent where the forecast did not say
   *  and in every cache from before the card; the card prints what is
   *  there. Chance of rain in per cent, the day's strongest wind in km/h,
   *  sunrise and sunset as `HH:MM` local to the location. */
  rain_chance?: number | null;
  wind_max_kmh?: number | null;
  sunrise?: string | null;
  sunset?: string | null;
};

/** Right now, for today's card: Celsius unrounded like every temperature
 *  here, wind in km/h, and when it held (`2026-09-07T07:15`, local to the
 *  location) — a reading can be a refresh interval old. */
export type CurrentWeather = {
  bucket: string;
  temp: number;
  feels: number;
  humidity: number;
  wind_kmh: number;
  at: string;
};

/** How the forecast's place was decided. The card always says it: a
 *  detected place is a guess from the connection's IP and can be a city
 *  off, and a forecast for the wrong place with no way to tell is worse
 *  than none. */
export type LocationSource = 'configured' | 'detected' | 'demo';

export type WeatherReport = {
  days: DayWeather[];
  place: string | null;
  current?: CurrentWeather | null;
  /** Absent in caches from before the card, which read as detected — the
   *  honest default for a place nobody chose. */
  source?: LocationSource | null;
  /** When the backend last fetched this, ms epoch. It is stamped on read
   *  from a row the cache writer has always kept, so it is right even for
   *  a cache written before the field existed; `null` only where there is
   *  no cache at all. */
  fetched_at?: number | null;
};

/** Past this a forecast is old enough to warn about: two missed refresh
 *  cycles (the backend fetches every three hours), so one late tick never
 *  cries wolf. Mirrors `weather::STALE_AFTER_MS`. */
export const STALE_AFTER_MS = 6 * 3600_000;

/** The age of a forecast in words, and whether to warn about it.
 *
 *  The warning is the point of the whole field: a forecast the app could
 *  not refresh is kept and shown rather than blanked — deliberately, since
 *  a sky an hour out of date beats no sky — and without an age on it a
 *  two-day-old reading looks exactly like a fresh one. No timestamp, or one
 *  from the future, counts as stale: not knowing when a reading was taken
 *  is not a reason to present it as current. */
export const freshness = (
  fetchedAt: number | null | undefined,
  nowMs: number,
): { label: string; stale: boolean } => {
  if (fetchedAt == null || fetchedAt > nowMs) {
    return { label: 'Updated at an unknown time', stale: true };
  }
  const age = nowMs - fetchedAt;
  const stale = age >= STALE_AFTER_MS;
  const minutes = Math.floor(age / 60_000);
  if (minutes < 2) return { label: 'Updated just now', stale };
  if (minutes < 60) return { label: `Updated ${minutes} minutes ago`, stale };
  if (minutes < 24 * 60) {
    const h = Math.floor(minutes / 60);
    return { label: `Updated ${h} hour${h === 1 ? '' : 's'} ago`, stale };
  }
  const d = Math.floor(minutes / (24 * 60));
  return { label: `Updated ${d} day${d === 1 ? '' : 's'} ago`, stale };
};

/** The line under the place, in the card's own words. */
export const sourceLine = (source: LocationSource | null | undefined): string => {
  switch (source) {
    case 'configured': return "set in the bar's weather panel";
    case 'demo': return 'demo data';
    default: return "from your connection's location, which may be a city off";
  }
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
