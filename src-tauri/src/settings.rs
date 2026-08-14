//! The preferences the settings modal edits, and the two commands behind it.
//!
//! Everything here lives in the same `settings` key/value table `sync_loop`
//! and `status` already use — one table, string values, read with a parse and
//! a fallback. That is deliberate rather than lazy: a typed column per
//! preference means a migration per preference, and these are a handful of
//! scalars that a hand-edited row must never be able to crash the app with.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::AppState;

const SYNC_INTERVAL_KEY: &str = "sync_interval_ms";
const NOTIFICATIONS_KEY: &str = "notifications_enabled";
const LIST_MODE_KEY: &str = "list_mode";
const FALLBACK_KEY: &str = "fallback_reminder_minutes";
const DEFAULT_CALENDAR_KEY: &str = "default_calendar_id";
const TIME_FORMAT_KEY: &str = "time_format";
const WEEK_START_KEY: &str = "week_start";

/// Whether a clock is drawn as `13:30` or as `1:30 PM`.
///
/// An enum rather than the `String` the table actually holds, so the only two
/// values that exist are the two the app can draw. That is what lets
/// [`set_time_format`] take this type directly and skip a refusal path
/// entirely: a third value cannot be sent, so there is no user-facing error
/// to name, pin with a test and allowlist in `errors.rs` for a case the
/// select element makes unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeFormat {
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

impl TimeFormat {
    /// The stored spelling. The same strings the wire uses, so a row read by
    /// eye in `sqlite3` says what the settings modal says.
    fn as_str(self) -> &'static str {
        match self {
            TimeFormat::H24 => "24h",
            TimeFormat::H12 => "12h",
        }
    }
}

/// The day a week begins on.
///
/// Three, not seven, and they are Google Calendar's own three — this is a
/// Google Calendar client, and a week starting on a Wednesday is a preference
/// no calendar this one syncs with can express. An enum for the same reason
/// [`TimeFormat`] is one: the set is closed, so [`set_week_start`] needs no
/// refusal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeekStart {
    #[serde(rename = "monday")]
    Monday,
    #[serde(rename = "sunday")]
    Sunday,
    #[serde(rename = "saturday")]
    Saturday,
}

impl WeekStart {
    /// The stored spelling, which is also the wire spelling.
    fn as_str(self) -> &'static str {
        match self {
            WeekStart::Monday => "monday",
            WeekStart::Sunday => "sunday",
            WeekStart::Saturday => "saturday",
        }
    }

    /// This day as jiff's own weekday, for the grid anchors that walk
    /// backwards to it.
    pub(crate) fn weekday(self) -> jiff::civil::Weekday {
        use jiff::civil::Weekday;
        match self {
            WeekStart::Monday => Weekday::Monday,
            WeekStart::Sunday => Weekday::Sunday,
            WeekStart::Saturday => Weekday::Saturday,
        }
    }

    /// How many blank cells precede a month whose 1st falls on `first` — the
    /// month grid's `lead_blanks`.
    ///
    /// Monday-zero throughout rather than jiff's two offset helpers chosen per
    /// variant: one origin, one subtraction, and the modulo does the wrapping.
    /// Mixing the two origins is how this arithmetic goes wrong.
    pub(crate) fn lead_blanks(self, first: jiff::civil::Weekday) -> usize {
        let day = first.to_monday_zero_offset() as usize;
        let start = self.weekday().to_monday_zero_offset() as usize;
        (day + 7 - start) % 7
    }

    /// Whether the column at `index` in a week-aligned row is a weekend day.
    ///
    /// Read off the *index*, never off the date the column carries — the
    /// property Big Year's 28-day rows exist to guarantee (see
    /// `every_row_puts_its_weekends_in_the_same_columns`). Note that only a
    /// Monday start puts Saturday and Sunday next to each other; the other two
    /// split the pair to the ends of the row, exactly as they do in every
    /// month grid those readers have ever used.
    ///
    /// Used by the Rust suite rather than by the app: the shading itself is
    /// drawn in the browser, from `weekstart.ts`'s own copy of this rule. That
    /// is exactly why this exists — `the_ribbons_weekend_stripes_stay_straight_under_every_start`
    /// asserts the two agree against real dates, so the copy cannot drift.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_weekend_column(self, index: usize) -> bool {
        use jiff::civil::Weekday;
        let start = self.weekday().to_monday_zero_offset() as usize;
        let weekday = (start + index) % 7;
        weekday == Weekday::Saturday.to_monday_zero_offset() as usize
            || weekday == Weekday::Sunday.to_monday_zero_offset() as usize
    }
}

/// What the General and Notifications tabs show.
///
/// `sync_interval_ms` is reported as **stored**, not as clamped. The clamp in
/// [`crate::sync_loop::interval_ms`] is a defence against a row somebody
/// edited by hand with `sqlite3` — which the platform guides documented as the
/// only way to change this until now — and reporting the clamped value here
/// would make the form silently disagree with the database it is editing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub sync_interval_ms: i64,
    pub notifications_enabled: bool,
    /// The floor, published rather than duplicated in the UI. The form has to
    /// say what the minimum is in order to refuse a smaller one with a reason,
    /// and a second copy of the number in TypeScript is one that drifts.
    pub min_sync_interval_ms: i64,
    /// Whether Day, Week and Month draw as a list rather than a grid — the
    /// filmstrip toggle (filmstrip spec §4).
    ///
    /// Here rather than in a table of its own because it is a preference and
    /// belongs beside the others, and because the alternative — remembering it
    /// only for the session — cannot survive the restart the spec asks it to.
    /// No tab in the settings modal shows it: the control that sets it is the
    /// `▦`/`☰` beside the view switcher, and a second control for the same
    /// value in a modal would be a second place for it to disagree.
    pub list_mode: bool,
    /// Minutes-before for the fallback reminders (fallback spec §3): what
    /// fires for a timed event that follows its calendar's defaults when the
    /// calendar has none. Minutes alone, because the fallback is popup by
    /// construction — omacal never sends email, so a method field here could
    /// only ever hold one value.
    pub fallback_reminder_minutes: Vec<i64>,
    /// The calendar a new event lands on unless the user picks another, or
    /// `None` for "the primary, else the first writable" — the rule that
    /// existed before this setting did. **Stored unvalidated on purpose**: a
    /// valid id goes stale the moment its calendar is removed or loses write
    /// access, so the use-site guard (`offerableCalendarId`, which replaces
    /// an id a create cannot land on) has to exist regardless — and a
    /// write-time check would only duplicate it with a second rule to drift.
    pub default_calendar_id: Option<i64>,
    /// Whether times are drawn as `13:30` or `1:30 PM`, everywhere the app
    /// prints one — event blocks, the filmstrip, the popover and the Week and
    /// Day hour gutter, which follows deliberately: a 12-hour reader given a
    /// 24-hour ruler has to convert in their head at exactly the moment the
    /// ruler exists to save them from it.
    pub time_format: TimeFormat,
    /// The day a week begins on, honoured by the Week grid's own anchor, the
    /// month grid's leading blanks, the Year view's twelve small grids, and
    /// Big Year's 392-day ribbon.
    pub week_start: WeekStart,
}

async fn read(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

async fn write(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// The settings as stored, with defaults for anything absent.
///
/// Absent is the ordinary case on a fresh install and is not an error:
/// nothing writes these until the user opens the modal.
pub async fn read_settings(pool: &SqlitePool) -> AppSettings {
    AppSettings {
        sync_interval_ms: read(pool, SYNC_INTERVAL_KEY)
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::sync_loop::DEFAULT_INTERVAL_MS),
        // **Reminders are on unless somebody turned them off.** The opposite
        // default would mean a fresh install silently firing nothing, which
        // looks exactly like the notification transport being broken — and on
        // macOS, where it may genuinely be, the two would be indistinguishable.
        notifications_enabled: read(pool, NOTIFICATIONS_KEY)
            .await
            .map(|v| v != "0")
            .unwrap_or(true),
        min_sync_interval_ms: crate::sync_loop::MIN_INTERVAL_MS,
        // **The grid is what a calendar looks like until somebody says
        // otherwise**, so the absent row reads as off — the opposite polarity
        // to `notifications_enabled` above, and for the opposite reason. A
        // reminder nobody sees is indistinguishable from a broken transport; a
        // grid nobody asked for is just the app as it has always looked.
        //
        // `== "1"` rather than `!= "0"`, so a value from a future version or a
        // hand-edited row lands on that same default rather than silently
        // turning the calendar into a list.
        list_mode: read(pool, LIST_MODE_KEY).await.map(|v| v == "1").unwrap_or(false),
        // **Shipped as 60 and 10, not empty** (fallback spec §3): the gap
        // this fills is real meetings going silent on receive-only shared
        // calendars, and an empty default would leave a fresh install with
        // exactly that surprise. `[]` stored is a real choice — the feature
        // off — and survives; only an absent or unparseable row lands here.
        fallback_reminder_minutes: read(pool, FALLBACK_KEY)
            .await
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_else(|| vec![60, 10]),
        // Absent, cleared ("" — see `set_default_calendar`) and garbage all
        // read as `None`: the old rule, never an error.
        default_calendar_id: read(pool, DEFAULT_CALENDAR_KEY)
            .await
            .and_then(|v| v.parse().ok()),
        // `== "12h"` rather than `!= "24h"`, the same polarity `list_mode`
        // takes and for the same reason: absent, garbage, and a value written
        // by some future version all land on the format the app has always
        // drawn, rather than on the one nobody asked for.
        time_format: read(pool, TIME_FORMAT_KEY)
            .await
            .map(|v| if v == "12h" { TimeFormat::H12 } else { TimeFormat::H24 })
            .unwrap_or(TimeFormat::H24),
        // Same polarity rule as its two neighbours: only the two spellings
        // this version writes move the setting, and everything else — absent,
        // hand-edited, or written by a version that learned a fourth day —
        // lands on the week omacal has always drawn.
        week_start: match read(pool, WEEK_START_KEY).await.as_deref() {
            Some("sunday") => WeekStart::Sunday,
            Some("saturday") => WeekStart::Saturday,
            _ => WeekStart::Monday,
        },
    }
}

/// What the user is told when a sync interval below the floor is refused.
///
/// A named constant for the same reason the other two are: it is pinned by a
/// test and allowlisted in `errors.rs`, and the two must not drift.
pub const INTERVAL_TOO_SHORT: &str =
    "omacal will not sync more often than once a minute — Google's quota is finite and a \
     desktop app has no business polling faster than that";

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(read_settings(&state.pool).await)
}

/// Stores a new sync interval, **refusing anything below the floor**.
///
/// Refused rather than clamped, and that is the whole of the decision. A value
/// accepted and then quietly changed is worse than one that is turned down: the
/// user types 10 seconds, the form says nothing, and the app polls every minute
/// while they believe otherwise. `sync_loop::interval_ms` still clamps on the
/// way *out*, because a row edited by hand with `sqlite3` — the only way to set
/// this until now, documented in both platform guides — never passed through
/// here at all.
#[tauri::command]
pub async fn set_sync_interval(
    state: tauri::State<'_, AppState>,
    ms: i64,
) -> Result<AppSettings, String> {
    set_sync_interval_impl(&state.pool, ms)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

async fn set_sync_interval_impl(pool: &SqlitePool, ms: i64) -> anyhow::Result<AppSettings> {
    if ms < crate::sync_loop::MIN_INTERVAL_MS {
        anyhow::bail!(INTERVAL_TOO_SHORT);
    }
    write(pool, SYNC_INTERVAL_KEY, &ms.to_string()).await?;
    Ok(read_settings(pool).await)
}

#[tauri::command]
pub async fn set_notifications_enabled(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Result<AppSettings, String> {
    write(&state.pool, NOTIFICATIONS_KEY, if on { "1" } else { "0" })
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

/// Stores the fallback reminder rows, through the same bounds the event
/// form's rows are held to — `write::validate_reminders`, so the two cannot
/// drift apart — and refused with the limit named, never clamped (spec §3).
/// `[]` is accepted and meaningful: it is the feature turned off.
#[tauri::command]
pub async fn set_fallback_reminders(
    state: tauri::State<'_, AppState>,
    minutes: Vec<i64>,
) -> Result<AppSettings, String> {
    set_fallback_reminders_impl(&state.pool, minutes)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

pub(crate) async fn set_fallback_reminders_impl(
    pool: &SqlitePool,
    minutes: Vec<i64>,
) -> anyhow::Result<AppSettings> {
    let as_input = crate::write::RemindersInput {
        use_default: false,
        overrides: minutes
            .iter()
            .map(|&m| crate::write::ReminderInput { method: "popup".into(), minutes: m })
            .collect(),
    };
    crate::write::validate_reminders(&as_input).map_err(|m| anyhow::anyhow!(m))?;
    write(pool, FALLBACK_KEY, &serde_json::to_string(&minutes)?).await?;
    Ok(read_settings(pool).await)
}

/// Stores the default calendar for new events. `None` clears the choice —
/// written as an empty value rather than a deleted row, so `write`'s upsert
/// is the only statement this module ever makes about the table.
#[tauri::command]
pub async fn set_default_calendar(
    state: tauri::State<'_, AppState>,
    id: Option<i64>,
) -> Result<AppSettings, String> {
    write(&state.pool, DEFAULT_CALENDAR_KEY, &id.map(|v| v.to_string()).unwrap_or_default())
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

/// Stores the filmstrip toggle. Nothing is refused and nothing is clamped —
/// unlike the sync interval, there is no value of a boolean the app has to
/// protect Google's quota from.
#[tauri::command]
pub async fn set_list_mode(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> Result<AppSettings, String> {
    write(&state.pool, LIST_MODE_KEY, if on { "1" } else { "0" })
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

/// Stores the clock format. Like [`set_list_mode`] nothing is refused, and
/// here the *type* is the reason rather than the triviality of a boolean:
/// [`TimeFormat`] has no third variant for a caller to send.
#[tauri::command]
pub async fn set_time_format(
    state: tauri::State<'_, AppState>,
    format: TimeFormat,
) -> Result<AppSettings, String> {
    write(&state.pool, TIME_FORMAT_KEY, format.as_str())
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

/// Stores the day a week begins on. Nothing to refuse: [`WeekStart`] has
/// three variants and the select offers all three.
#[tauri::command]
pub async fn set_week_start(
    state: tauri::State<'_, AppState>,
    start: WeekStart,
) -> Result<AppSettings, String> {
    write(&state.pool, WEEK_START_KEY, start.as_str())
        .await
        .map_err(|e| crate::errors::user_facing(&e))?;
    Ok(read_settings(&state.pool).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        omacal_store::connect_memory().await.unwrap()
    }

    /// A fresh install has written none of these, and that is the ordinary
    /// case rather than an error.
    #[tokio::test]
    async fn absent_settings_read_as_their_defaults() {
        let s = read_settings(&pool().await).await;
        assert_eq!(s.sync_interval_ms, crate::sync_loop::DEFAULT_INTERVAL_MS);
        assert!(s.notifications_enabled, "reminders must be on until turned off");
        assert_eq!(s.min_sync_interval_ms, crate::sync_loop::MIN_INTERVAL_MS);
        assert!(!s.list_mode, "a fresh install draws the grid, not a list");
        assert_eq!(
            s.fallback_reminder_minutes,
            vec![60, 10],
            "shipped as 60 and 10, not empty — an empty default is today's silence again"
        );
        assert_eq!(s.default_calendar_id, None, "no choice made is the old rule, not an id");
        assert_eq!(
            s.time_format,
            TimeFormat::H24,
            "the clock the app has always drawn, so no installed copy changes under its user"
        );
        assert_eq!(
            s.week_start,
            WeekStart::Monday,
            "the week omacal has always drawn"
        );
    }

    /// `None` must clear a previously stored id back to the old rule — a
    /// choice that could only ever be changed, never unmade, is a trap.
    #[tokio::test]
    async fn the_default_calendar_round_trips_and_clears() {
        let p = pool().await;

        // Through the command's own body: the Tauri wrapper only adds State.
        write(&p, DEFAULT_CALENDAR_KEY, "8").await.unwrap();
        assert_eq!(read_settings(&p).await.default_calendar_id, Some(8));

        write(&p, DEFAULT_CALENDAR_KEY, "").await.unwrap();
        assert_eq!(read_settings(&p).await.default_calendar_id, None);
    }

    /// `[]` stored is a real choice — the feature off — and must read back as
    /// itself, never as the shipped default (fallback spec §3).
    #[tokio::test]
    async fn fallback_reminders_round_trip_including_none() {
        let p = pool().await;

        let s = set_fallback_reminders_impl(&p, vec![15]).await.unwrap();
        assert_eq!(s.fallback_reminder_minutes, vec![15]);
        assert_eq!(read_settings(&p).await.fallback_reminder_minutes, vec![15]);

        let s = set_fallback_reminders_impl(&p, vec![]).await.unwrap();
        assert!(s.fallback_reminder_minutes.is_empty());
        assert!(read_settings(&p).await.fallback_reminder_minutes.is_empty());
    }

    /// The event form's own bounds, through the same function, refused with
    /// the limit named — and the stored value untouched by a refused write.
    #[tokio::test]
    async fn fallback_reminders_are_held_to_googles_bounds() {
        let p = pool().await;
        assert!(set_fallback_reminders_impl(&p, vec![40_321]).await.is_err());
        assert!(set_fallback_reminders_impl(&p, (0..6).collect()).await.is_err());
        assert!(set_fallback_reminders_impl(&p, vec![-1]).await.is_err());
        assert_eq!(
            read_settings(&p).await.fallback_reminder_minutes,
            vec![60, 10],
            "a refused write must leave the stored value alone"
        );
    }

    #[tokio::test]
    async fn an_interval_at_or_above_the_floor_is_stored_and_read_back() {
        let p = pool().await;
        let got = set_sync_interval_impl(&p, 120_000).await.unwrap();
        assert_eq!(got.sync_interval_ms, 120_000);
        assert_eq!(read_settings(&p).await.sync_interval_ms, 120_000);

        // Exactly the floor is allowed, so the refusal below cannot be
        // satisfied by a rule that refuses the boundary too.
        let at = set_sync_interval_impl(&p, crate::sync_loop::MIN_INTERVAL_MS).await.unwrap();
        assert_eq!(at.sync_interval_ms, crate::sync_loop::MIN_INTERVAL_MS);
    }

    /// **Refused, not clamped.** A value accepted and then quietly changed is
    /// worse than one turned down: the user believes they are polling every ten
    /// seconds and the app is not.
    #[tokio::test]
    async fn an_interval_below_the_floor_is_refused_and_nothing_is_stored() {
        let p = pool().await;
        set_sync_interval_impl(&p, 120_000).await.unwrap();

        let err = set_sync_interval_impl(&p, 10_000).await.unwrap_err();
        assert_eq!(err.to_string(), INTERVAL_TOO_SHORT);
        assert_eq!(
            read_settings(&p).await.sync_interval_ms,
            120_000,
            "a refused value must not half-land",
        );
        assert_eq!(crate::errors::user_facing(&err), INTERVAL_TOO_SHORT);
    }

    /// The three grids' shared arithmetic, as a table.
    ///
    /// August 2026 opens on a Saturday, which is the month that separates all
    /// three starts: five blanks under Monday, six under Sunday, none at all
    /// under Saturday. A month opening mid-week would agree under two of them
    /// and hide a wrong subtraction.
    #[test]
    fn lead_blanks_are_counted_from_the_chosen_first_day() {
        use jiff::civil::Weekday;
        assert_eq!(WeekStart::Monday.lead_blanks(Weekday::Saturday), 5);
        assert_eq!(WeekStart::Sunday.lead_blanks(Weekday::Saturday), 6);
        assert_eq!(WeekStart::Saturday.lead_blanks(Weekday::Saturday), 0);

        // The first day of the week is always zero blanks, and the day before
        // it is always six. Anything else means the modulo wrapped wrong.
        for (start, day_before) in [
            (WeekStart::Monday, Weekday::Sunday),
            (WeekStart::Sunday, Weekday::Saturday),
            (WeekStart::Saturday, Weekday::Friday),
        ] {
            assert_eq!(start.lead_blanks(start.weekday()), 0, "{start:?}");
            assert_eq!(start.lead_blanks(day_before), 6, "{start:?}");
        }
    }

    /// Weekends land where the reader expects, and — the load-bearing half —
    /// **exactly two columns of every seven** are weekend under all three.
    /// A formula that drifted would still satisfy a single hand-written row.
    #[test]
    fn weekend_columns_follow_the_first_day() {
        // Monday start: the pair sits together, columns 5 and 6.
        assert_eq!(
            (0..7).filter(|&c| WeekStart::Monday.is_weekend_column(c)).collect::<Vec<_>>(),
            vec![5, 6],
        );
        // Sunday start splits the pair to the ends — as every Sunday-start
        // month grid in the world does.
        assert_eq!(
            (0..7).filter(|&c| WeekStart::Sunday.is_weekend_column(c)).collect::<Vec<_>>(),
            vec![0, 6],
        );
        // Saturday start puts it back together, at the front.
        assert_eq!(
            (0..7).filter(|&c| WeekStart::Saturday.is_weekend_column(c)).collect::<Vec<_>>(),
            vec![0, 1],
        );

        // Across a full 28-day Big Year row, every start marks eight columns —
        // and marks them in the same place in each of the four blocks, which
        // is the property the 28-day row exists for.
        for start in [WeekStart::Monday, WeekStart::Sunday, WeekStart::Saturday] {
            let marked: Vec<usize> = (0..28).filter(|&c| start.is_weekend_column(c)).collect();
            assert_eq!(marked.len(), 8, "{start:?} marked the wrong number of days");
            for block in 1..4 {
                for i in 0..2 {
                    assert_eq!(
                        marked[block * 2 + i],
                        marked[i] + block * 7,
                        "{start:?} drifted in block {block}",
                    );
                }
            }
        }
    }

    /// Both directions, because a format that could be turned on and not off
    /// is half a setting.
    #[tokio::test]
    async fn the_time_format_round_trips_both_ways() {
        let p = pool().await;

        write(&p, TIME_FORMAT_KEY, TimeFormat::H12.as_str()).await.unwrap();
        assert_eq!(read_settings(&p).await.time_format, TimeFormat::H12);

        write(&p, TIME_FORMAT_KEY, TimeFormat::H24.as_str()).await.unwrap();
        assert_eq!(read_settings(&p).await.time_format, TimeFormat::H24);
    }

    /// All three round-trip, and an unrecognised row reads as Monday — the
    /// same polarity rule the two settings beside this one take.
    #[tokio::test]
    async fn the_week_start_round_trips_and_falls_back_to_monday() {
        let p = pool().await;
        for start in [WeekStart::Sunday, WeekStart::Saturday, WeekStart::Monday] {
            write(&p, WEEK_START_KEY, start.as_str()).await.unwrap();
            assert_eq!(read_settings(&p).await.week_start, start);
        }
        for stored in ["", "Sunday", "sun", "wednesday", "🗓"] {
            write(&p, WEEK_START_KEY, stored).await.unwrap();
            assert_eq!(
                read_settings(&p).await.week_start,
                WeekStart::Monday,
                "{stored:?} is not a day this version writes",
            );
        }
    }

    /// The polarity rule, witnessed by a value the app never writes. A row
    /// edited by hand — or written by a future version that learned a third
    /// format — must land on the clock the app has always drawn, not on the
    /// other one and not on a panic.
    #[tokio::test]
    async fn an_unrecognised_stored_format_reads_as_24h() {
        let p = pool().await;
        for stored in ["", "12", "H12", "twelve", "24h ", "🕐"] {
            write(&p, TIME_FORMAT_KEY, stored).await.unwrap();
            assert_eq!(
                read_settings(&p).await.time_format,
                TimeFormat::H24,
                "{stored:?} is not 12h and must not be read as it"
            );
        }
    }

    /// The stored spelling is what the wire uses, so the row reads in
    /// `sqlite3` as the modal says. Pinned because the two are written in
    /// different places — `as_str` and a serde rename — and nothing else
    /// would notice them drifting apart.
    #[test]
    fn the_week_starts_stored_spelling_is_its_wire_spelling() {
        for w in [WeekStart::Monday, WeekStart::Sunday, WeekStart::Saturday] {
            assert_eq!(serde_json::to_string(&w).unwrap(), format!("\"{}\"", w.as_str()));
        }
    }

    #[test]
    fn the_stored_spelling_is_the_wire_spelling() {
        for f in [TimeFormat::H24, TimeFormat::H12] {
            assert_eq!(
                serde_json::to_string(&f).unwrap(),
                format!("\"{}\"", f.as_str()),
            );
        }
    }

    /// The interval the *loop* uses still clamps, because a row edited by hand
    /// with `sqlite3` never passed through the command that refuses.
    #[tokio::test]
    async fn a_hand_edited_row_below_the_floor_is_still_clamped_on_the_way_out() {
        let p = pool().await;
        write(&p, SYNC_INTERVAL_KEY, "100").await.unwrap();

        assert_eq!(read_settings(&p).await.sync_interval_ms, 100, "reported as stored");
        assert_eq!(
            crate::sync_loop::interval_ms(&p).await,
            crate::sync_loop::MIN_INTERVAL_MS,
            "and clamped where it is actually used",
        );
    }

    #[tokio::test]
    async fn notifications_can_be_turned_off_and_back_on() {
        let p = pool().await;
        write(&p, NOTIFICATIONS_KEY, "0").await.unwrap();
        assert!(!read_settings(&p).await.notifications_enabled);
        write(&p, NOTIFICATIONS_KEY, "1").await.unwrap();
        assert!(read_settings(&p).await.notifications_enabled);
    }

    /// A value nobody here wrote — hand-edited, or from a future version —
    /// reads as *on* rather than crashing or silently disabling reminders.
    #[tokio::test]
    async fn an_unrecognised_notifications_value_leaves_reminders_on() {
        let p = pool().await;
        write(&p, NOTIFICATIONS_KEY, "yes").await.unwrap();
        assert!(read_settings(&p).await.notifications_enabled);
    }

    /// **The half a UI spec cannot witness.** Flipping the toggle in one
    /// session proves a variable changed; only reading the row back out of a
    /// pool that was never told anything proves it was *stored*.
    #[tokio::test]
    async fn list_mode_is_stored_and_read_back() {
        let p = pool().await;
        write(&p, LIST_MODE_KEY, "1").await.unwrap();
        assert!(read_settings(&p).await.list_mode);
        write(&p, LIST_MODE_KEY, "0").await.unwrap();
        assert!(!read_settings(&p).await.list_mode);
    }

    /// Turning it on must not disturb the preferences stored beside it — one
    /// `settings` table, and a write that replaced the row rather than
    /// upserting its own key would take the sync interval with it.
    #[tokio::test]
    async fn storing_list_mode_leaves_its_neighbours_alone() {
        let p = pool().await;
        set_sync_interval_impl(&p, 120_000).await.unwrap();
        write(&p, NOTIFICATIONS_KEY, "0").await.unwrap();

        write(&p, LIST_MODE_KEY, "1").await.unwrap();

        let s = read_settings(&p).await;
        assert!(s.list_mode);
        assert_eq!(s.sync_interval_ms, 120_000);
        assert!(!s.notifications_enabled);
    }

    /// A value nobody here wrote reads as **off** — the grid, which is what a
    /// calendar looks like until somebody says otherwise. The opposite
    /// polarity to reminders above, and deliberately: a hand-edited row must
    /// not be able to turn the whole calendar into a list.
    #[tokio::test]
    async fn an_unrecognised_list_mode_value_leaves_the_grid() {
        let p = pool().await;
        write(&p, LIST_MODE_KEY, "yes").await.unwrap();
        assert!(!read_settings(&p).await.list_mode);
    }
}
