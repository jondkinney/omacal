//! Weather in the day headers — Fantastical's idea, the Omarchy widget's
//! sources, omacal's ink.
//!
//! Two endpoints, the exact pair the Omarchy bar widget uses, in the same
//! roles: **Open-Meteo** (`api.open-meteo.com`) is the data — daily weather
//! code and min/max, keyless and free — and **wttr.in** is only the answer
//! to "where am I" when nobody has said. Location resolves in the widget's
//! own order, and deliberately *through* the widget: a user who set their
//! city in the bar has set it for the calendar too, because the first stop
//! is the widget's own state file. After that: coordinates cached from a
//! previous auto-detect, then one wttr.in call to learn them from the IP.
//!
//! Weather is decoration. Every failure here is a `debug!` and an absent
//! icon, never a banner — a calendar that nags about a forecast it could
//! not fetch has its priorities exactly backwards. The forecast refreshes
//! on a three-hour ticker (a sky changes slower than a calendar), and the
//! cache is served stale rather than blank while a refresh is in flight.
//!
//! Split as everywhere: parsing the three JSON shapes, the code→bucket
//! mapping (ported group-for-group from the widget's `Model.js`, so the bar
//! and the calendar never tell two stories about one sky), and the may-fetch
//! gates are pure and tested; the ticker and the HTTP are the untested half.

use serde::Serialize;
use sqlx::SqlitePool;
use std::time::Duration;

/// Cached forecast JSON (our own compact shape, not Open-Meteo's).
const CACHE_KEY: &str = "weather_cache";
/// When the cache was written, ms epoch.
const CACHE_AT_KEY: &str = "weather_cache_at";
/// Auto-detected coordinates, as `lat,lon|Name`, and when they were learned.
const COORDS_KEY: &str = "weather_coords";
const COORDS_AT_KEY: &str = "weather_coords_at";

/// A sky changes slower than a calendar; Open-Meteo asks heavy users to stay
/// polite. Three hours is eight calls a day.
const REFRESH_EVERY: Duration = Duration::from_secs(3 * 3600);
/// The forecast horizon: today plus a week — one full Week view whichever
/// day it starts on.
const FORECAST_DAYS: u8 = 8;
/// How long an IP-derived location is trusted before asking again. Machines
/// move; they mostly don't move daily.
const COORDS_TTL_MS: i64 = 24 * 3600 * 1000;

/// One day of forecast, as the UI draws it: a bucket for the icon, the two
/// temperatures Fantastical taught everyone to expect.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct DayWeather {
    /// ISO date, local to the *location* (`timezone=auto`), which for the
    /// only case that matters — you, where you are — is the display zone.
    pub date: String,
    /// The icon family, not the raw code: `clear` | `partly` | `overcast` |
    /// `fog` | `drizzle` | `rain` | `snow` | `thunder`. Decided here so the
    /// grouping is tested once, beside the table it was ported from.
    pub bucket: String,
    /// **Celsius, unrounded** — the unit Open-Meteo answers in and the one
    /// this side stays in, whichever unit the header ends up printing. The
    /// rounding lives at the display end (`temperature.ts`) because it can
    /// only happen once: rounding here and converting there turns 31.6°C
    /// into 90°F, and 31.6°C is 89°F.
    pub tmax: f64,
    pub tmin: f64,
    /// The day card's extras (2026-09-07), each absent where Open-Meteo did
    /// not say — and absent in every cache written before they existed,
    /// which `serde(default)` reads as the same thing. The card prints a
    /// line for what is there and nothing for what is not.
    /// Chance of rain, per cent (`precipitation_probability_max`).
    #[serde(default)]
    pub rain_chance: Option<u8>,
    /// The day's strongest wind, km/h — Open-Meteo's own unit; the card
    /// prints mph beside Fahrenheit (`temperature.ts`).
    #[serde(default)]
    pub wind_max_kmh: Option<f64>,
    /// `HH:MM`, local to the location.
    #[serde(default)]
    pub sunrise: Option<String>,
    #[serde(default)]
    pub sunset: Option<String>,
}

/// Conditions right now, for today's card — Open-Meteo's `current` block,
/// which arrives in the same call as the days.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct CurrentWeather {
    pub bucket: String,
    /// Celsius, unrounded, like every temperature here.
    pub temp: f64,
    /// Apparent temperature, the "feels like".
    pub feels: f64,
    /// Relative humidity, per cent.
    pub humidity: u8,
    /// km/h.
    pub wind_kmh: f64,
    /// When these held, ISO local to the location (`2026-09-07T07:15`) —
    /// a reading can be up to a refresh interval old, and the card says so.
    pub at: String,
}

/// Where the forecast's place came from. **The card always says it**,
/// because the place may not be where the user is: a detected one is a
/// guess from the connection's IP, which can be a city off, or a country
/// off through a VPN, and a forecast for the wrong place with no way to
/// tell is worse than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationSource {
    /// The Omarchy bar's weather setting — the user chose it.
    Configured,
    /// Learned from the connection's IP.
    Detected,
    /// Demo mode's fixed sky.
    Demo,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, Default)]
pub struct WeatherReport {
    pub days: Vec<DayWeather>,
    /// Where this forecast is for, when known — the settings hint names it,
    /// and the card leads with it.
    pub place: Option<String>,
    /// Right now, when the answer carried it; absent in older caches.
    #[serde(default)]
    pub current: Option<CurrentWeather>,
    /// How `place` was decided; absent in older caches, which the card
    /// treats as detected — the honest default.
    #[serde(default)]
    pub source: Option<LocationSource>,
    /// When this forecast was fetched, ms epoch — **stamped on read**, from
    /// the row the cache writer has always kept, so a cache written before
    /// this field existed still reports its true age. `None` only where
    /// there is no cache at all.
    ///
    /// It exists because a stale forecast is otherwise indistinguishable
    /// from a fresh one: `refresh` keeps the last good answer when the
    /// endpoint cannot be reached, deliberately, and a laptop that spent
    /// two days asleep comes back to a two-day-old sky. The surfaces say
    /// the age, so a wrong number gets reported rather than believed.
    #[serde(default)]
    pub fetched_at: Option<i64>,
}

/// Past this, a forecast is old enough to say so loudly: two missed
/// refresh cycles, so an ordinary late tick never cries wolf.
pub const STALE_AFTER_MS: i64 = 2 * REFRESH_EVERY.as_millis() as i64;

/// The age of a forecast in the words every surface uses, and whether it is
/// stale enough to warn about.
///
/// A cache with no timestamp counts as stale, and so does one stamped in
/// the future: not knowing when a reading was taken is not a reason to
/// present it as current.
pub(crate) fn freshness(fetched_at: Option<i64>, now_ms: i64) -> (String, bool) {
    let Some(at) = fetched_at else {
        return ("Updated at an unknown time".into(), true);
    };
    if at > now_ms {
        return ("Updated at an unknown time".into(), true);
    }
    let age = now_ms - at;
    let stale = age >= STALE_AFTER_MS;
    let minutes = age / 60_000;
    let label = if minutes < 2 {
        "Updated just now".to_string()
    } else if minutes < 60 {
        format!("Updated {minutes} minutes ago")
    } else if minutes < 24 * 60 {
        let h = minutes / 60;
        format!("Updated {h} hour{} ago", if h == 1 { "" } else { "s" })
    } else {
        let d = minutes / (24 * 60);
        format!("Updated {d} day{} ago", if d == 1 { "" } else { "s" })
    };
    (label, stale)
}

/// Same gate, same shape, same reason as [`crate::update::may_check`]: demo
/// promises no network traffic, and a disabled setting means disabled.
pub(crate) fn may_fetch(demo: bool, enabled: bool) -> bool {
    !demo && enabled
}

/// Open-Meteo WMO code → icon bucket. **Ported group-for-group from the
/// Omarchy widget's `iconForOpenMeteoCode`** (`Model.js`), so the bar and
/// the calendar always agree on what kind of day it is. The fallback is
/// `overcast` there and stays `overcast` here.
pub(crate) fn bucket_for_code(code: u16) -> &'static str {
    match code {
        0 => "clear",
        1 | 2 => "partly",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 | 56 | 57 | 61 => "drizzle",
        63 | 65 | 66 | 67 | 80 | 81 | 82 => "rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snow",
        95 | 96 | 99 => "thunder",
        _ => "overcast",
    }
}

/// The Omarchy widget's location file, when this is an Omarchy machine and
/// the user has set one: `{name, latitude, longitude}` — coordinates
/// optional, a bare name geocodes below.
pub(crate) fn parse_omarchy_location(raw: &str) -> Option<(Option<(f64, f64)>, String)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let name = v.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let coords = match (
        v.get("latitude").and_then(|x| x.as_f64()),
        v.get("longitude").and_then(|x| x.as_f64()),
    ) {
        (Some(lat), Some(lon)) => Some((lat, lon)),
        _ => None,
    };
    Some((coords, name))
}

/// Coordinates and a place name out of a wttr.in `j1` answer — the widget's
/// auto-detect, reading the same fields (`nearest_area`). wttr sends numbers
/// as strings.
pub(crate) fn parse_wttr_coords(raw: &str) -> Option<(f64, f64, String)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let area = v.get("nearest_area")?.get(0)?;
    let num = |k: &str| area.get(k)?.get(0)?.get("value")?.as_str()?.parse::<f64>().ok();
    let lat = area.get("latitude")?.as_str()?.parse::<f64>().ok().or_else(|| num("latitude"))?;
    let lon = area.get("longitude")?.as_str()?.parse::<f64>().ok().or_else(|| num("longitude"))?;
    let name = area
        .get("areaName")
        .and_then(|a| a.get(0))
        .and_then(|a| a.get("value"))
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .to_string();
    Some((lat, lon, name))
}

/// First hit of Open-Meteo's geocoder, for a widget location that has a name
/// but no coordinates.
pub(crate) fn parse_geocoding(raw: &str) -> Option<(f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let hit = v.get("results")?.get(0)?;
    Some((hit.get("latitude")?.as_f64()?, hit.get("longitude")?.as_f64()?))
}

/// Open-Meteo's forecast → our report. Temperatures pass through unrounded
/// — the rounding happens once, at display, in whichever unit the header
/// prints (see the note on `DayWeather::tmax`).
///
/// The four daily fields the headers always needed are required; the
/// card's extras and the `current` block are each optional, so an answer
/// missing one of them still draws the headers it always drew.
pub(crate) fn parse_open_meteo(
    raw: &str,
    place: Option<String>,
    source: LocationSource,
) -> Option<WeatherReport> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let daily = v.get("daily")?;
    let dates = daily.get("time")?.as_array()?;
    let codes = daily.get("weather_code")?.as_array()?;
    let tmax = daily.get("temperature_2m_max")?.as_array()?;
    let tmin = daily.get("temperature_2m_min")?.as_array()?;
    let extra = |key: &str| daily.get(key).and_then(|a| a.as_array());
    let rain = extra("precipitation_probability_max");
    let wind = extra("wind_speed_10m_max");
    let sunrise = extra("sunrise");
    let sunset = extra("sunset");

    let mut days = Vec::with_capacity(dates.len());
    for i in 0..dates.len() {
        let (Some(date), Some(code), Some(hi), Some(lo)) = (
            dates.get(i).and_then(|d| d.as_str()),
            codes.get(i).and_then(|c| c.as_u64()),
            tmax.get(i).and_then(|t| t.as_f64()),
            tmin.get(i).and_then(|t| t.as_f64()),
        ) else {
            continue; // One null row is Open-Meteo's problem, not the header's.
        };
        days.push(DayWeather {
            date: date.to_string(),
            bucket: bucket_for_code(code as u16).to_string(),
            tmax: hi,
            tmin: lo,
            rain_chance: rain
                .and_then(|a| a.get(i))
                .and_then(|x| x.as_u64())
                .map(|x| x.min(100) as u8),
            wind_max_kmh: wind.and_then(|a| a.get(i)).and_then(|x| x.as_f64()),
            sunrise: sunrise.and_then(|a| a.get(i)).and_then(|x| x.as_str()).map(clock_of),
            sunset: sunset.and_then(|a| a.get(i)).and_then(|x| x.as_str()).map(clock_of),
        });
    }
    if days.is_empty() {
        return None;
    }
    let current = v.get("current").and_then(|c| {
        Some(CurrentWeather {
            bucket: bucket_for_code(c.get("weather_code")?.as_u64()? as u16).to_string(),
            temp: c.get("temperature_2m")?.as_f64()?,
            feels: c.get("apparent_temperature")?.as_f64()?,
            humidity: c.get("relative_humidity_2m")?.as_u64()?.min(100) as u8,
            wind_kmh: c.get("wind_speed_10m")?.as_f64()?,
            at: c.get("time")?.as_str()?.to_string(),
        })
    });
    // `fetched_at` is stamped by `cached_report` on the way out, from the
    // row the writer keeps, rather than here: that way it is right for a
    // cache written before the field existed too.
    Some(WeatherReport { days, place, current, source: Some(source), fetched_at: None })
}

/// The `HH:MM` of an Open-Meteo local timestamp (`2026-09-07T06:05`), or
/// the string as it came if it is not one — the card prints it either way.
fn clock_of(iso: &str) -> String {
    iso.split_once('T').map_or(iso, |(_, t)| t).chars().take(5).collect()
}

/// The place out of wttr.in's own location line (`?format=%l`), which is
/// what the Omarchy bar prints — `Gurugram, Haryana, India` — where the
/// `j1` area name still says `Gurgaon`. The first part, so both sides call
/// the same place by the same name. Blank for a blank answer.
pub(crate) fn wttr_place_name(line: &str) -> Option<String> {
    let first = line.split(',').next()?.trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// Whether a cache written at `at_ms` still answers at `now_ms`.
pub(crate) fn cache_is_fresh(now_ms: i64, at_ms: i64, ttl_ms: i64) -> bool {
    at_ms <= now_ms && now_ms - at_ms < ttl_ms
}

/// Demo's forecast: a fixed cycle through every bucket the UI can draw,
/// dated from today. Deterministic — a demo screenshot taken twice shows the
/// same sky — and offline, which is demo's whole promise.
pub(crate) fn synthetic_report(today: jiff::civil::Date, fetched_at_ms: i64) -> WeatherReport {
    const CYCLE: &[(u16, f64, f64)] = &[
        (0, 31.6, 24.0), (2, 29.0, 23.0), (3, 27.0, 22.0), (61, 26.0, 22.0),
        (95, 25.0, 21.0), (71, 2.0, -3.0), (1, 28.0, 22.0), (0, 30.0, 23.0),
    ];
    let days = CYCLE
        .iter()
        .enumerate()
        .map(|(i, (code, hi, lo))| DayWeather {
            date: today.saturating_add(jiff::Span::new().days(i as i64)).to_string(),
            bucket: bucket_for_code(*code).to_string(),
            tmax: *hi,
            tmin: *lo,
            rain_chance: Some(((i * 15) % 100) as u8),
            wind_max_kmh: Some(8.0 + i as f64 * 3.0),
            sunrise: Some("06:05".into()),
            sunset: Some("18:30".into()),
        })
        .collect();
    WeatherReport {
        days,
        place: Some("Demo".into()),
        current: Some(CurrentWeather {
            bucket: "clear".into(),
            temp: 26.4,
            feels: 29.1,
            humidity: 73,
            wind_kmh: 4.0,
            at: format!("{today}T09:00"),
        }),
        source: Some(LocationSource::Demo),
        // Demo fetches nothing, so its sky is as fresh as the moment it is
        // asked for — a demo screenshot must not carry a staleness warning.
        //
        // **The caller owns the clock**, as `patch_todo_status` and the task
        // date helpers do. Reading `Timestamp::now()` here made a function
        // whose whole point is determinism disagree with itself between two
        // calls, and the test that asserted otherwise passed only while both
        // landed in the same millisecond. It lost that coin toss on main
        // (2026-09-09) having been unsound since it was written.
        fetched_at: Some(fetched_at_ms),
    }
}

/// Where the Omarchy widget keeps its configured location. `None` off
/// Omarchy — same posture as `omarchy_plugin::plugin_dir`: on any other
/// machine this module reads nothing of Omarchy's.
fn omarchy_location_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = std::path::Path::new(&home).join(".local/state/omarchy/settings/weather.json");
    p.exists().then_some(p)
}

async fn http_get(url: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let resp = reqwest::Client::builder()
        .user_agent(concat!("omacal/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()?
        .get(url)
        .send()
        .await
        .context("weather endpoint unreachable")?;
    if !resp.status().is_success() {
        anyhow::bail!("weather endpoint answered {}", resp.status());
    }
    Ok(resp.text().await?)
}

/// The location to forecast for, resolved in the widget's order: its file
/// (coordinates as given, a bare name geocoded), then coordinates this
/// module auto-detected recently, then one wttr.in call to learn them —
/// and, with them, where the answer came from, which the card says.
async fn resolve_location(
    pool: &SqlitePool,
    now_ms: i64,
) -> Option<(f64, f64, Option<String>, LocationSource)> {
    if let Some(path) = omarchy_location_path() {
        if let Some((coords, name)) = tokio::fs::read_to_string(&path)
            .await
            .ok()
            .and_then(|raw| parse_omarchy_location(&raw))
        {
            if let Some((lat, lon)) = coords {
                return Some((lat, lon, Some(name), LocationSource::Configured));
            }
            let url = format!(
                "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                urlencoding_encode(&name)
            );
            if let Some((lat, lon)) = http_get(&url).await.ok().and_then(|r| parse_geocoding(&r)) {
                return Some((lat, lon, Some(name), LocationSource::Configured));
            }
            // A name that will not geocode falls through to auto-detect
            // rather than to nothing: a misspelled city should not turn the
            // feature off.
        }
    }

    let cached_at: i64 = crate::settings::read(pool, COORDS_AT_KEY)
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if cache_is_fresh(now_ms, cached_at, COORDS_TTL_MS) {
        if let Some(v) = crate::settings::read(pool, COORDS_KEY).await {
            if let Some((coords, name)) = v.split_once('|') {
                if let Some((lat, lon)) = coords
                    .split_once(',')
                    .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                {
                    let name = (!name.is_empty()).then(|| name.to_string());
                    return Some((lat, lon, name, LocationSource::Detected));
                }
            }
        }
    }

    let raw = http_get("https://wttr.in/?format=j1").await.ok()?;
    let (lat, lon, area) = parse_wttr_coords(&raw)?;
    // The bar's spelling of the same place, when wttr.in will say it; the
    // area name from the answer above otherwise.
    let name = http_get("https://wttr.in/?format=%l")
        .await
        .ok()
        .and_then(|line| wttr_place_name(&line))
        .unwrap_or(area);
    let _ = crate::settings::write(pool, COORDS_KEY, &format!("{lat},{lon}|{name}")).await;
    let _ = crate::settings::write(pool, COORDS_AT_KEY, &now_ms.to_string()).await;
    Some((lat, lon, (!name.is_empty()).then_some(name), LocationSource::Detected))
}

/// Minimal percent-encoding for the one geocoding query parameter — a city
/// name, not arbitrary data; a dependency for this would be a dependency
/// for spaces.
fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c.to_string()]
            } else {
                c.to_string().bytes().map(|b| format!("%{b:02X}")).collect()
            }
        })
        .collect()
}

/// One fetch: resolve where, ask Open-Meteo, cache the report — and say so.
/// Quiet on every failure — the header just keeps whatever it last knew.
///
/// The `weather-changed` emit is the race the first field run lost: the UI
/// reads the cache once at mount, and the first fetch — wttr.in can take
/// most of a minute — lands after that read, leaving the headers empty
/// until the hourly re-poll. Same fix as `update-notice`: the backend
/// learned something; it tells the webview instead of waiting to be asked.
async fn refresh(app: Option<&tauri::AppHandle>, pool: &SqlitePool) {
    let now_ms = jiff::Timestamp::now().as_millisecond();
    let Some((lat, lon, place, source)) = resolve_location(pool, now_ms).await else {
        tracing::debug!("weather: no location; skipping");
        return;
    };
    // One call: the days the headers draw, the extras the day card prints,
    // and the `current` block today's card leads with.
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,\
         precipitation_probability_max,wind_speed_10m_max,sunrise,sunset\
         &current=weather_code,temperature_2m,apparent_temperature,\
         relative_humidity_2m,wind_speed_10m\
         &forecast_days={FORECAST_DAYS}&timezone=auto"
    );
    match http_get(&url).await.ok().and_then(|raw| parse_open_meteo(&raw, place, source)) {
        Some(report) => {
            if let Ok(json) = serde_json::to_string(&report) {
                let _ = crate::settings::write(pool, CACHE_KEY, &json).await;
                let _ = crate::settings::write(pool, CACHE_AT_KEY, &now_ms.to_string()).await;
                tracing::debug!(days = report.days.len(), "weather: forecast cached");
                if let Some(app) = app {
                    use tauri::Emitter;
                    let _ = app.emit("weather-changed", ());
                }
            }
        }
        None => tracing::debug!("weather: fetch or parse failed; keeping the stale cache"),
    }
}

/// [`refresh`] for a caller holding only clones — the settings toggle, so
/// switching weather on shows a sky now rather than at the next tick. Gated
/// again here: the toggle handing this a `false` is a programming error, but
/// demo handing it anything must still fetch nothing.
pub fn refresh_soon(app: tauri::AppHandle, pool: SqlitePool, demo: bool, enabled: bool) {
    if !may_fetch(demo, enabled) {
        return;
    }
    tauri::async_runtime::spawn(async move { refresh(Some(&app), &pool).await });
}

/// The three-hour loop. The enabled setting is re-read every tick, so a
/// toggle off stops the traffic at the next tick and a toggle back on is
/// carried by [`refresh_soon`] in the meantime.
pub(crate) fn spawn(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let (pool, demo) = {
            let state = app.state::<crate::AppState>();
            (state.pool.clone(), state.demo)
        };
        if demo {
            return;
        }
        let mut ticker = tokio::time::interval(REFRESH_EVERY);
        loop {
            ticker.tick().await; // the first tick resolves immediately
            let enabled = crate::settings::weather_enabled(&pool).await;
            if may_fetch(demo, enabled) {
                refresh(Some(&app), &pool).await;
            }
        }
    });
}

/// What the headers draw. Disabled answers empty rather than erroring — the
/// UI's rule is simply "no days, no icons". Demo answers the synthetic week,
/// so the feature is visible (and deterministic) with zero network.
#[tauri::command]
pub(crate) async fn get_weather(
    state: tauri::State<'_, crate::AppState>,
) -> Result<WeatherReport, String> {
    if !crate::settings::weather_enabled(&state.pool).await {
        return Ok(WeatherReport::default());
    }
    if state.demo {
        return Ok(synthetic_report(jiff::Zoned::now().date(), jiff::Timestamp::now().as_millisecond()));
    }
    Ok(cached_report(&state.pool).await.unwrap_or_default())
}

/// The last forecast fetched, as cached — the one thing the CLI and the
/// window both read, so `omacal weather` can never show a different sky
/// from the header.
pub(crate) async fn cached_report(pool: &SqlitePool) -> Option<WeatherReport> {
    let mut report: WeatherReport = crate::settings::read(pool, CACHE_KEY)
        .await
        .and_then(|json| serde_json::from_str(&json).ok())?;
    report.fetched_at = crate::settings::read(pool, CACHE_AT_KEY)
        .await
        .and_then(|v| v.parse().ok());
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The widget's own grouping, pinned code group by code group — this
    /// table *is* the agreement between the bar and the calendar, so a
    /// regrouping on either side must fail a test, not drift silently.
    #[test]
    fn the_code_buckets_match_the_omarchy_widgets_grouping() {
        assert_eq!(bucket_for_code(0), "clear");
        for c in [1, 2] {
            assert_eq!(bucket_for_code(c), "partly", "code {c}");
        }
        assert_eq!(bucket_for_code(3), "overcast");
        for c in [45, 48] {
            assert_eq!(bucket_for_code(c), "fog", "code {c}");
        }
        for c in [51, 53, 55, 56, 57, 61] {
            assert_eq!(bucket_for_code(c), "drizzle", "code {c}");
        }
        for c in [63, 65, 66, 67, 80, 81, 82] {
            assert_eq!(bucket_for_code(c), "rain", "code {c}");
        }
        for c in [71, 73, 75, 77, 85, 86] {
            assert_eq!(bucket_for_code(c), "snow", "code {c}");
        }
        for c in [95, 96, 99] {
            assert_eq!(bucket_for_code(c), "thunder", "code {c}");
        }
        // The widget's own fallback for a code neither table names.
        assert_eq!(bucket_for_code(42), "overcast");
    }

    /// A real Open-Meteo daily answer, cut to two days. Unrounded is part of
    /// the contract: the caller (`temperature.ts`) is the one that rounds,
    /// once, in whichever unit it is printing — pinning `32` here would have
    /// hidden the very bug (rounding twice, in two units) this shape exists
    /// to avoid.
    #[test]
    fn an_open_meteo_answer_becomes_unrounded_bucketed_days() {
        let raw = r#"{"daily":{
            "time":["2026-08-24","2026-08-25"],
            "weather_code":[3,95],
            "temperature_2m_max":[31.6,29.4],
            "temperature_2m_min":[25.5,24.1]}}"#;
        let r = parse_open_meteo(raw, Some("Gurugram".into()), LocationSource::Detected).unwrap();
        assert_eq!(r.place.as_deref(), Some("Gurugram"));
        assert_eq!(r.source, Some(LocationSource::Detected));
        assert_eq!(r.days.len(), 2);
        assert_eq!(r.days[0].date, "2026-08-24");
        assert_eq!(r.days[0].bucket, "overcast");
        assert_eq!(r.days[0].tmax, 31.6);
        assert_eq!(r.days[0].tmin, 25.5);
        assert_eq!(r.days[1].bucket, "thunder");
        // The extras are each absent when the answer lacks them, never
        // invented; and no `current` block is no current.
        assert_eq!(r.days[0].rain_chance, None);
        assert_eq!(r.days[0].sunrise, None);
        assert_eq!(r.current, None);
    }

    /// The age a surface prints, and the line past which it warns.
    #[test]
    fn a_forecasts_age_is_said_in_words_and_warns_past_two_missed_cycles() {
        let now = 1_700_000_000_000_i64;
        let ago = |ms: i64| freshness(Some(now - ms), now);
        assert_eq!(ago(30_000), ("Updated just now".into(), false));
        assert_eq!(ago(20 * 60_000), ("Updated 20 minutes ago".into(), false));
        assert_eq!(ago(60 * 60_000), ("Updated 1 hour ago".into(), false));
        assert_eq!(ago(5 * 3_600_000), ("Updated 5 hours ago".into(), false));
        assert!(!ago(STALE_AFTER_MS - 60_000).1, "one late tick is not a warning");
        assert!(ago(STALE_AFTER_MS).1, "two missed cycles is");
        assert_eq!(ago(26 * 3_600_000), ("Updated 1 day ago".into(), true));
        assert_eq!(ago(3 * 24 * 3_600_000), ("Updated 3 days ago".into(), true));
        assert_eq!(freshness(None, now), ("Updated at an unknown time".into(), true));
        assert!(freshness(Some(now + 60_000), now).1, "a future stamp is not fresh");
    }

    /// The whole answer the card needs, in Open-Meteo's own shapes: the
    /// daily extras and the `current` block, temperatures unrounded, the
    /// sun times cut to the clock, the wind in the km/h it arrives in.
    #[test]
    fn the_cards_extras_and_the_current_block_parse_from_the_same_answer() {
        let raw = r#"{"current":{"time":"2026-09-07T07:15","weather_code":0,
            "temperature_2m":26.4,"apparent_temperature":29.1,
            "relative_humidity_2m":73,"wind_speed_10m":4.3},
          "daily":{"time":["2026-09-07","2026-09-08"],
            "weather_code":[0,61],"temperature_2m_max":[33.2,31.0],
            "temperature_2m_min":[25.1,24.6],
            "precipitation_probability_max":[10,85],
            "wind_speed_10m_max":[12.4,18.0],
            "sunrise":["2026-09-07T06:05","2026-09-08T06:06"],
            "sunset":["2026-09-07T18:30","2026-09-08T18:29"]}}"#;
        let r = parse_open_meteo(raw, Some("Gurugram".into()), LocationSource::Configured).unwrap();
        let now = r.current.expect("current");
        assert_eq!(now.bucket, "clear");
        assert_eq!((now.temp, now.feels, now.humidity, now.wind_kmh), (26.4, 29.1, 73, 4.3));
        assert_eq!(now.at, "2026-09-07T07:15");
        let d = &r.days[1];
        assert_eq!(d.bucket, "drizzle");
        assert_eq!(d.rain_chance, Some(85));
        assert_eq!(d.wind_max_kmh, Some(18.0));
        assert_eq!(d.sunrise.as_deref(), Some("06:06"));
        assert_eq!(d.sunset.as_deref(), Some("18:29"));
        assert_eq!(r.source, Some(LocationSource::Configured));
    }

    /// A cache written before the card existed still reads, as days with
    /// no extras and no current — the header keeps drawing across the
    /// update, and the card says what it can.
    #[test]
    fn a_cache_from_before_the_card_still_reads() {
        let old = r#"{"days":[{"date":"2026-09-06","bucket":"clear","tmax":32.0,"tmin":25.0}],"place":"Gurgaon"}"#;
        let r: WeatherReport = serde_json::from_str(old).unwrap();
        assert_eq!(r.days[0].rain_chance, None);
        assert_eq!(r.current, None);
        assert_eq!(r.source, None);
        assert_eq!(r.fetched_at, None);
        assert_eq!(r.place.as_deref(), Some("Gurgaon"));
    }

    /// The age comes off the row the cache writer has always kept, stamped
    /// on the way out — so a cache written before the field existed still
    /// reports its true age rather than "unknown". That is the whole reason
    /// it is read here rather than stored in the JSON.
    #[tokio::test]
    async fn a_cached_forecast_is_stamped_with_when_it_was_fetched() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let json = r#"{"days":[{"date":"2026-09-06","bucket":"clear","tmax":32.0,"tmin":25.0}],"place":"Gurgaon"}"#;
        crate::settings::write(&pool, CACHE_KEY, json).await.unwrap();
        crate::settings::write(&pool, CACHE_AT_KEY, "1700000000000").await.unwrap();

        let r = cached_report(&pool).await.unwrap();
        assert_eq!(r.fetched_at, Some(1_700_000_000_000));
        assert_eq!(freshness(r.fetched_at, 1_700_000_000_000 + 25 * 3_600_000).0, "Updated 1 day ago");

        let empty = omacal_store::connect_memory().await.unwrap();
        assert!(cached_report(&empty).await.is_none(), "no cache is no report, not an ageless one");
    }

    /// wttr.in's location line is what the bar prints; the first part is
    /// the place, and a blank line is no name rather than an empty one.
    #[test]
    fn the_bars_place_name_is_the_first_part_of_wttrs_location_line() {
        assert_eq!(wttr_place_name("Gurugram, Haryana, India\n").as_deref(), Some("Gurugram"));
        assert_eq!(wttr_place_name("Sofia").as_deref(), Some("Sofia"));
        assert_eq!(wttr_place_name("  \n"), None);
        assert_eq!(wttr_place_name(""), None);
    }

    /// Garbage, an empty daily block, and a shape drift all answer `None` —
    /// decoration never invents data.
    #[test]
    fn a_bad_forecast_answer_is_none_not_a_guess() {
        assert!(parse_open_meteo("not json", None, LocationSource::Detected).is_none());
        assert!(parse_open_meteo(r#"{"daily":{"time":[]}}"#, None, LocationSource::Detected).is_none());
        assert!(parse_open_meteo(r#"{"hourly":{}}"#, None, LocationSource::Detected).is_none());
    }

    /// The widget's location file, in its three real shapes: coordinates,
    /// name-only (geocoded later), and absent/blank name meaning auto.
    #[test]
    fn the_widgets_location_file_parses_in_all_three_shapes() {
        let full = r#"{"name":"Malibu","latitude":34.03,"longitude":-118.68}"#;
        assert_eq!(
            parse_omarchy_location(full),
            Some((Some((34.03, -118.68)), "Malibu".to_string()))
        );
        assert_eq!(
            parse_omarchy_location(r#"{"name":"Malibu"}"#),
            Some((None, "Malibu".to_string()))
        );
        assert_eq!(parse_omarchy_location(r#"{"name":""}"#), None);
        assert_eq!(parse_omarchy_location("junk"), None);
    }

    /// wttr.in's `j1`, cut to what auto-detect reads. Its numbers arrive as
    /// strings, which is exactly the trap this parser exists to absorb.
    #[test]
    fn wttr_coordinates_parse_from_their_string_shaped_numbers() {
        let raw = r#"{"nearest_area":[{
            "areaName":[{"value":"Gurugram"}],
            "latitude":"28.450","longitude":"77.033"}]}"#;
        let (lat, lon, name) = parse_wttr_coords(raw).unwrap();
        assert_eq!((lat, lon), (28.45, 77.033));
        assert_eq!(name, "Gurugram");
        assert!(parse_wttr_coords(r#"{"nearest_area":[]}"#).is_none());
    }

    /// The gates, all four corners — demo's no-network promise outranks the
    /// setting in both directions.
    #[test]
    fn only_an_enabled_non_demo_build_may_fetch_weather() {
        assert!(may_fetch(false, true));
        assert!(!may_fetch(false, false), "off means off");
        assert!(!may_fetch(true, true), "demo made network traffic");
        assert!(!may_fetch(true, false));
    }

    /// Freshness is a window, not a comparison: a cache from the future —
    /// a clock that jumped back — is stale, not eternally fresh.
    #[test]
    fn cache_freshness_is_a_window_and_a_future_stamp_is_stale() {
        assert!(cache_is_fresh(1_000, 900, 200));
        assert!(!cache_is_fresh(1_000, 700, 200), "expired read as fresh");
        assert!(!cache_is_fresh(1_000, 1_100, 200), "a future stamp read as fresh");
    }

    /// Demo's week is deterministic, dated from the given today, and cycles
    /// through every bucket the UI can draw — that is what makes it a demo.
    #[test]
    fn the_demo_forecast_is_deterministic_and_shows_every_kind_of_sky() {
        let today = jiff::civil::date(2026, 8, 24);
        // A fixed stamp, because the clock is the caller's now. With
        // `Timestamp::now()` inside, this equality held only when both calls
        // landed in the same millisecond — which is how it passed for weeks
        // and then reddened main.
        let stamp = 1_756_000_000_000;
        let r = synthetic_report(today, stamp);
        assert_eq!(r.days.len(), 8);
        assert_eq!(r.days[0].date, "2026-08-24");
        assert_eq!(r.days[7].date, "2026-08-31");
        assert_eq!(r, synthetic_report(today, stamp), "two runs disagreed");
        assert_eq!(r.fetched_at, Some(stamp), "the stamp is the caller's, not the clock's");
        let buckets: std::collections::HashSet<_> =
            r.days.iter().map(|d| d.bucket.as_str()).collect();
        for b in ["clear", "partly", "overcast", "drizzle", "thunder", "snow"] {
            assert!(buckets.contains(b), "demo never shows {b}");
        }
    }

    /// The one query parameter that carries user text. Spaces and unicode,
    /// since city names have both.
    #[test]
    fn the_geocoding_query_percent_encodes() {
        assert_eq!(urlencoding_encode("New Delhi"), "New%20Delhi");
        assert_eq!(urlencoding_encode("Zürich"), "Z%C3%BCrich");
        assert_eq!(urlencoding_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    /// Geocoder's first hit, and an empty result set refusing quietly.
    #[test]
    fn geocoding_takes_the_first_hit_or_nothing() {
        let raw = r#"{"results":[{"latitude":28.46,"longitude":77.03,"name":"Gurugram"}]}"#;
        assert_eq!(parse_geocoding(raw), Some((28.46, 77.03)));
        assert!(parse_geocoding(r#"{"generationtime_ms":0.5}"#).is_none());
    }
}
