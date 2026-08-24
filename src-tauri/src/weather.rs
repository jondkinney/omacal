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
    pub tmax: i32,
    pub tmin: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, Default)]
pub struct WeatherReport {
    pub days: Vec<DayWeather>,
    /// Where this forecast is for, when known — the settings hint names it.
    pub place: Option<String>,
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

/// Open-Meteo's daily forecast → our report. Temperatures round to whole
/// degrees — a day header saying `31.6°` is a header showing off.
pub(crate) fn parse_open_meteo(raw: &str, place: Option<String>) -> Option<WeatherReport> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let daily = v.get("daily")?;
    let dates = daily.get("time")?.as_array()?;
    let codes = daily.get("weather_code")?.as_array()?;
    let tmax = daily.get("temperature_2m_max")?.as_array()?;
    let tmin = daily.get("temperature_2m_min")?.as_array()?;

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
            tmax: hi.round() as i32,
            tmin: lo.round() as i32,
        });
    }
    (!days.is_empty()).then_some(WeatherReport { days, place })
}

/// Whether a cache written at `at_ms` still answers at `now_ms`.
pub(crate) fn cache_is_fresh(now_ms: i64, at_ms: i64, ttl_ms: i64) -> bool {
    at_ms <= now_ms && now_ms - at_ms < ttl_ms
}

/// Demo's forecast: a fixed cycle through every bucket the UI can draw,
/// dated from today. Deterministic — a demo screenshot taken twice shows the
/// same sky — and offline, which is demo's whole promise.
pub(crate) fn synthetic_report(today: jiff::civil::Date) -> WeatherReport {
    const CYCLE: &[(u16, i32, i32)] = &[
        (0, 31, 24), (2, 29, 23), (3, 27, 22), (61, 26, 22),
        (95, 25, 21), (71, 2, -3), (1, 28, 22), (0, 30, 23),
    ];
    let days = CYCLE
        .iter()
        .enumerate()
        .map(|(i, (code, hi, lo))| DayWeather {
            date: today.saturating_add(jiff::Span::new().days(i as i64)).to_string(),
            bucket: bucket_for_code(*code).to_string(),
            tmax: *hi,
            tmin: *lo,
        })
        .collect();
    WeatherReport { days, place: Some("Demo".into()) }
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
/// module auto-detected recently, then one wttr.in call to learn them.
async fn resolve_location(pool: &SqlitePool, now_ms: i64) -> Option<(f64, f64, Option<String>)> {
    if let Some(path) = omarchy_location_path() {
        if let Some((coords, name)) = tokio::fs::read_to_string(&path)
            .await
            .ok()
            .and_then(|raw| parse_omarchy_location(&raw))
        {
            if let Some((lat, lon)) = coords {
                return Some((lat, lon, Some(name)));
            }
            let url = format!(
                "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                urlencoding_encode(&name)
            );
            if let Some((lat, lon)) = http_get(&url).await.ok().and_then(|r| parse_geocoding(&r)) {
                return Some((lat, lon, Some(name)));
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
                    return Some((lat, lon, name));
                }
            }
        }
    }

    let raw = http_get("https://wttr.in/?format=j1").await.ok()?;
    let (lat, lon, name) = parse_wttr_coords(&raw)?;
    let _ = crate::settings::write(pool, COORDS_KEY, &format!("{lat},{lon}|{name}")).await;
    let _ = crate::settings::write(pool, COORDS_AT_KEY, &now_ms.to_string()).await;
    Some((lat, lon, (!name.is_empty()).then_some(name)))
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
    let Some((lat, lon, place)) = resolve_location(pool, now_ms).await else {
        tracing::debug!("weather: no location; skipping");
        return;
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &daily=weather_code,temperature_2m_max,temperature_2m_min\
         &forecast_days={FORECAST_DAYS}&timezone=auto"
    );
    match http_get(&url).await.ok().and_then(|raw| parse_open_meteo(&raw, place)) {
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
        return Ok(synthetic_report(jiff::Zoned::now().date()));
    }
    let report = crate::settings::read(&state.pool, CACHE_KEY)
        .await
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(report)
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

    /// A real Open-Meteo daily answer, cut to two days. Rounding is part of
    /// the contract: a header saying `31.6°` is a header showing off.
    #[test]
    fn an_open_meteo_answer_becomes_rounded_bucketed_days() {
        let raw = r#"{"daily":{
            "time":["2026-08-24","2026-08-25"],
            "weather_code":[3,95],
            "temperature_2m_max":[31.6,29.4],
            "temperature_2m_min":[25.5,24.1]}}"#;
        let r = parse_open_meteo(raw, Some("Gurugram".into())).unwrap();
        assert_eq!(r.place.as_deref(), Some("Gurugram"));
        assert_eq!(r.days.len(), 2);
        assert_eq!(r.days[0].date, "2026-08-24");
        assert_eq!(r.days[0].bucket, "overcast");
        assert_eq!(r.days[0].tmax, 32);
        assert_eq!(r.days[0].tmin, 26);
        assert_eq!(r.days[1].bucket, "thunder");
    }

    /// Garbage, an empty daily block, and a shape drift all answer `None` —
    /// decoration never invents data.
    #[test]
    fn a_bad_forecast_answer_is_none_not_a_guess() {
        assert!(parse_open_meteo("not json", None).is_none());
        assert!(parse_open_meteo(r#"{"daily":{"time":[]}}"#, None).is_none());
        assert!(parse_open_meteo(r#"{"hourly":{}}"#, None).is_none());
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
        let r = synthetic_report(today);
        assert_eq!(r.days.len(), 8);
        assert_eq!(r.days[0].date, "2026-08-24");
        assert_eq!(r.days[7].date, "2026-08-31");
        assert_eq!(r, synthetic_report(today), "two runs disagreed");
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
