//! The tray, and what closing the window means.
//!
//! Split the same way the transport is: the parts that are decisions —
//! what is on the menu, what each entry means, whether autostart may be
//! registered — are pure and tested here. Building the tray icon and moving
//! the window are OS integration, and they are the untested half.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// The tray menu, in order: id, label.
///
/// **Quit is not optional.** Closing the window only hides it (see
/// [`hide_instead_of_closing`]), so if this entry ever goes the app cannot be
/// quit from the UI at all — the tray is the only way out.
pub(crate) const MENU: [(&str, &str); 3] =
    [("open", "Open omacal"), ("sync", "Sync now"), ("quit", "Quit")];

/// What a tray menu id means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Open,
    /// Open a meeting's conference link — the one thing a menu bar does
    /// better than a window, and the reason the macOS section exists
    /// (spec 2026-08-29 §2). Carries the URL the feed already resolved.
    Join(String),
    /// Show the window anchored on one date — `omacal 2026-09-01`. Carries
    /// the ISO date re-spelled by the parser, so downstream never re-reads
    /// argv. This is what makes the bar widget's rows, a keybinding, or a
    /// script able to land the calendar *somewhere*, not merely open it.
    OpenAt(String),
    SyncNow,
    Quit,
}

/// What a second `omacal` invocation asks of the running instance, read off
/// its argv. This is the tray menu's vocabulary arriving over the
/// single-instance channel — it exists so a surface that is not this process
/// (the Omarchy bar widget, a script, a keybinding) can drive the app:
/// `omacal --quit`, `omacal --sync-now`, `omacal 2026-09-01` opening the
/// window on that date, and a bare `omacal` meaning what launching an
/// already-running app has always meant, show the window.
/// Unknown flags fall through to Open rather than erroring — a second
/// instance has no stderr anyone will ever read. The flags outrank a date:
/// `--quit` alongside one is still the stronger ask.
pub(crate) fn instance_action(argv: &[String]) -> TrayAction {
    if argv.iter().any(|a| a == "--quit") {
        TrayAction::Quit
    } else if argv.iter().any(|a| a == "--sync-now") {
        TrayAction::SyncNow
    } else if let Some(ymd) = argv.iter().skip(1).find_map(|a| parse_date(a)) {
        TrayAction::OpenAt(ymd)
    } else {
        TrayAction::Open
    }
}

/// A positional date argument: `YYYY-MM-DD`, one spelling, deliberately.
/// The shape gate in front of jiff is what makes the contract testable as
/// stated — whatever looser ISO forms jiff happens to accept, `2026-9-1`
/// must read as an unknown argument (and so as Open, per the rule above),
/// not as a date that works on some builds. jiff then rejects the shapes
/// that look right but name no day, `2026-13-40` and friends.
fn parse_date(arg: &str) -> Option<String> {
    let b = arg.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    arg.parse::<jiff::civil::Date>().ok().map(|d| d.to_string())
}

/// Maps a menu id to the thing it does. Separate from doing it, so the mapping
/// is testable without an `AppHandle` — an id that silently matched nothing
/// would be a menu entry that does nothing when clicked.
///
/// The prefix a dated row's menu id carries, and a joinable one's.
///
/// Ids carry their argument because a menu event hands back a string and
/// nothing else: the alternative is a side table of row → date that the
/// rebuild in [`apply`] would have to keep in step with the menu it just
/// replaced. Parsed here, so an id shape that stops round-tripping is a
/// failing test rather than a menu entry that clicks and does nothing.
const AT_PREFIX: &str = "at:";
const JOIN_PREFIX: &str = "join:";

pub(crate) fn action_for(id: &str) -> Option<TrayAction> {
    match id {
        "open" => Some(TrayAction::Open),
        "sync" => Some(TrayAction::SyncNow),
        "quit" => Some(TrayAction::Quit),
        _ => {
            if let Some(rest) = id.strip_prefix(AT_PREFIX) {
                // Through the same gate argv's date goes through: one
                // spelling, and jiff refusing the shapes that look right
                // but name no day.
                return parse_date(rest).map(TrayAction::OpenAt);
            }
            // Only http(s). The URL is our own — it came from the feed, not
            // from the menu — but the check costs a line and means no future
            // feed change can put a `file:` or `javascript:` string in front
            // of the opener.
            if let Some(url) = id.strip_prefix(JOIN_PREFIX) {
                if url.starts_with("https://") || url.starts_with("http://") {
                    return Some(TrayAction::Join(url.to_string()));
                }
            }
            None
        }
    }
}

/// Whether start-on-login may be registered.
///
/// **Never in demo mode.** A synthetic-data build that launches itself on
/// login is a nasty surprise on someone's machine, and demo mode's whole
/// promise is that it touches nothing real. Same shape and same reason as
/// [`crate::notify_loop::may_notify`].
pub(crate) fn may_autostart(demo: bool) -> bool {
    !demo
}

/// Whether a window-close should hide rather than quit.
///
/// Always, and it is a rule rather than a constant because it is the one thing
/// standing between this app and the bug §2.6 describes: a window someone
/// closed, an app that looks gone, and reminders that silently stopped firing.
/// Quit is explicit, from the tray.
pub(crate) fn hide_instead_of_closing() -> bool {
    true
}

/// The tray icon's id, shared by [`build`] and [`set_visible`].
const TRAY_ID: &str = "omacal-tray";

/// How much of a title the macOS menu bar gets before an ellipsis. The
/// scarcest space in the app: a long meeting name pushes every other menu
/// extra off the right-hand side, so this errs short.
const TITLE_CAP: usize = 24;
/// A dropdown is a glance, not the calendar.
const EVENT_ROWS: usize = 8;
const TASK_ROWS: usize = 4;

/// `s` cut to `cap` *characters* with an ellipsis, or unchanged.
///
/// Characters and not bytes: a Cyrillic meeting title (this calendar has
/// plenty) would otherwise be cut mid-codepoint, and `String` truncation on
/// a char boundary is a panic, not a mangled label.
fn ellipsize(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let kept: String = s.chars().take(cap.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// A title with no title, worded once.
const UNTITLED: &str = "(no title)";

/// Whether `ev` is happening at `now_ms`. Half-open, as every interval in
/// this codebase is: an event that ends exactly now has ended.
fn running(ev: &crate::upcoming::FeedEvent, now_ms: i64) -> bool {
    ev.start_ms <= now_ms && now_ms < ev.end_ms
}

/// The macOS menu bar's text, or `None` for the icon alone.
///
/// The event running now wins over the next one — knowing you are *in*
/// something beats knowing what follows it. All-day entries never claim the
/// title: a day-long "Trip" would sit in the menu bar all day saying nothing
/// about the next hour, and the width it costs is the width every other
/// menu extra loses. Nothing upcoming yields `None` rather than an empty
/// string, which AppKit treats differently, and a stale title after the
/// meeting is worse than no title.
///
/// Only macOS shows the result (see [`apply`]), but it is compiled, called
/// and tested on every platform: CI is Linux-only, so a decision that only a
/// Mac compiles is a decision nothing checks until a release build finds it.
pub(crate) fn menu_title(
    feed: &crate::upcoming::Feed,
    now_ms: i64,
    fmt: crate::settings::TimeFormat,
) -> Option<String> {
    let timed = || feed.events.iter().filter(|e| !e.all_day);
    let now = timed().find(|e| running(e, now_ms));
    let next = || timed().find(|e| e.start_ms > now_ms);
    let ev = now.or_else(next)?;
    let title = ellipsize(ev.title.as_deref().unwrap_or(UNTITLED), TITLE_CAP);
    Some(if now.is_some() {
        format!("▸ {title}")
    } else {
        let clock = crate::notify::time_in_zone_with_format(ev.start_ms, &ev.tz, fmt);
        format!("{clock}  {title}")
    })
}

/// One row of the tray's live section: the menu id that names its action,
/// and the label the user reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub id: String,
    pub label: String,
}

/// The day an instant falls on in `tz`, as the `YYYY-MM-DD` `OpenAt` speaks.
/// The *event's* zone rather than this machine's, matching what the
/// notification text already does with the same instants.
fn day_in_zone(ms: i64, tz: &str) -> String {
    let ts = jiff::Timestamp::from_millisecond(ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let z = ts.in_tz(tz).unwrap_or_else(|_| ts.in_tz("UTC").expect("UTC always resolves"));
    z.date().to_string()
}

/// The live rows, in the bar widget's own order: what is on, what is next,
/// a Join for the meeting at hand, then what is due.
///
/// Pure, and that is the point — everything this decides is decided here,
/// leaving [`apply`] with nothing but Tauri calls.
pub(crate) fn rows(
    feed: &crate::upcoming::Feed,
    now_ms: i64,
    fmt: crate::settings::TimeFormat,
) -> Vec<Row> {
    let mut out = Vec::new();
    for ev in feed.events.iter().take(EVENT_ROWS) {
        let title = ellipsize(ev.title.as_deref().unwrap_or(UNTITLED), TITLE_CAP);
        let when = if ev.all_day {
            "all day".to_string()
        } else {
            crate::notify::time_in_zone_with_format(ev.start_ms, &ev.tz, fmt)
        };
        let mark = if running(ev, now_ms) { "▸ " } else { "" };
        out.push(Row {
            id: format!("{AT_PREFIX}{}", day_in_zone(ev.start_ms, &ev.tz)),
            label: format!("{mark}{when}  {title}"),
        });
    }

    // One Join, for the meeting at hand — the running one, else the next.
    // Not one per row: a dropdown of Join buttons is a way to join the
    // wrong call, and the feed's later rows are hours away.
    let at_hand = feed
        .events
        .iter()
        .find(|e| running(e, now_ms))
        .or_else(|| feed.events.iter().find(|e| e.start_ms > now_ms));
    if let Some(url) = at_hand.and_then(|e| e.conference.as_deref()) {
        if url.starts_with("https://") || url.starts_with("http://") {
            out.push(Row { id: format!("{JOIN_PREFIX}{url}"), label: "Join meeting".into() });
        }
    }

    for task in feed.tasks.iter().take(TASK_ROWS) {
        let title = ellipsize(&task.title, TITLE_CAP);
        // Tasks have no row of their own to open — the app's task list is
        // one window away, so every task row simply opens omacal.
        out.push(Row {
            id: "open".into(),
            label: if task.overdue {
                format!("⚠  {title}")
            } else {
                format!("due  {title}")
            },
        });
    }
    out
}

/// Shows or hides the tray icon on a running app — the live half of the
/// `tray_icon` setting. A no-op when the tray never built (macOS refusals,
/// headless oddities): the setting still persists and applies next launch.
pub(crate) fn set_visible(app: &AppHandle, on: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_visible(on) {
            tracing::warn!(%e, on, "could not change tray icon visibility");
        }
    }
}

/// Builds the tray icon and wires its menu.
///
/// **Untested.** Everything it decides is decided by [`MENU`] and
/// [`action_for`] above, which are; what is left is Tauri and the OS, and this
/// project has no way to assert that an icon appeared in a system tray.
pub(crate) fn build(app: &AppHandle) -> tauri::Result<()> {
    let items: Vec<MenuItem<_>> = MENU
        .iter()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<_>> =
        items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<_>).collect();
    let menu = Menu::with_items(app, &refs)?;

    // Not the window icon: that is the mark on a dark tile, and at tray
    // sizes on a dark bar the tile swallows it. tray.png is the mark alone
    // (see icons/tray.svg), drawn to survive 22px.
    //
    // Built with an id so `set_visible` below can find it again: the tray
    // icon is now a *setting*, because on Omarchy 4 the bar widget carries
    // the same three actions and a second omacal icon is one too many.
    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .icon(tauri::include_image!("icons/tray.png"))
        .on_menu_event(|app, event| match action_for(event.id.as_ref()) {
            Some(TrayAction::Open) => show_main_window(app),
            // Unreachable from a menu — `action_for` never returns it, the
            // menu has no dated entry — but honoured rather than ignored,
            // because a match arm that discards an action is how a future
            // menu entry would click and do nothing.
            Some(TrayAction::OpenAt(ymd)) => open_at(app, &ymd),
            Some(TrayAction::Join(url)) => {
                if let Err(e) = crate::browser::open_external(&url) {
                    tracing::warn!(%e, "could not open the meeting link from the tray");
                }
            }
            Some(TrayAction::SyncNow) => crate::sync_loop::request_now(app),
            Some(TrayAction::Quit) => app.exit(0),
            // An id the menu did not put there. Nothing to do, and nothing
            // worth crashing the app over.
            None => tracing::warn!(id = %event.id.as_ref(), "unknown tray menu id"),
        })
        .build(app)?;

    Ok(())
}

/// Rebuilds the tray's menu and title from one snapshot.
///
/// **Untested, like [`build`]** — every decision it carries was made by
/// [`rows`] and [`menu_title`], which are; what is left is Tauri and AppKit.
fn apply(app: &AppHandle, feed: &crate::upcoming::Feed, now_ms: i64,
         fmt: crate::settings::TimeFormat) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(()); // Tray turned off, or never built. Nothing to dress.
    };

    let live = rows(feed, now_ms, fmt);
    let mut items: Vec<MenuItem<_>> = Vec::new();
    for r in &live {
        items.push(MenuItem::with_id(app, &r.id, &r.label, true, None::<&str>)?);
    }
    let fixed: Vec<MenuItem<_>> = MENU
        .iter()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;

    let sep = tauri::menu::PredefinedMenuItem::separator(app)?;
    let mut refs: Vec<&dyn tauri::menu::IsMenuItem<_>> = Vec::new();
    for i in &items {
        refs.push(i);
    }
    if !items.is_empty() {
        refs.push(&sep);
    }
    for i in &fixed {
        refs.push(i);
    }
    tray.set_menu(Some(Menu::with_items(app, &refs)?))?;

    // `cfg!` and not `#[cfg]`, deliberately. `set_title` is not
    // platform-gated in Tauri, so writing the decision as a runtime branch
    // means the Linux CI runner type-checks the very line macOS runs —
    // and CI is Linux-only, so an `#[cfg]` block here would be compiled by
    // nothing until a release build on a Mac discovered it.
    //
    // The decision itself: macOS shows the title, Linux does not. There the
    // same string costs panel width next to a bar widget already saying
    // more, and Tauri's own note says the title needs the icon shown anyway.
    let title = if cfg!(target_os = "macos") { menu_title(feed, now_ms, fmt) } else { None };
    tray.set_title(title)?;

    Ok(())
}

/// Recomputes the snapshot and dresses the tray with it.
///
/// Spawned rather than awaited: every caller is a place that has just
/// finished doing something else (a sync landing, a tick firing), and none
/// of them should wait on a menu.
pub(crate) fn refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (pool, demo) = {
            let state = app.state::<crate::AppState>();
            (state.pool.clone(), state.demo)
        };
        // Demo mode dresses nothing, for the reason the feed itself refuses
        // it: synthetic meetings must never be announced as real.
        if demo {
            return;
        }
        let now = crate::now_ms();
        let feed = match crate::upcoming::current(&pool, now).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(%e, "could not read the upcoming feed for the tray");
                return;
            }
        };
        let fmt = crate::settings::read_settings(&pool).await.time_format;
        if let Err(e) = apply(&app, &feed, now, fmt) {
            tracing::warn!(%e, "could not update the tray menu");
        }
    });
}

/// How often the tray re-reads the clock.
///
/// The answer changes with time alone — a meeting starting is not something
/// any other part of this app notifies us about — so a tick is the only way
/// the title stops lying. A minute is the resolution the title shows.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// Starts the minute tick that keeps the title honest.
pub(crate) fn spawn_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            refresh(&app);
        }
    });
}

/// Brings the window back from hidden. Untested for the same reason as
/// [`build`].
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// What a dated invocation emits once the window is up, carrying the ISO
/// date. The webview owns every date computation in its own zone, so the
/// string crosses whole rather than as a timestamp somebody here would have
/// to pick a zone for.
pub(crate) const OPEN_DATE_EVENT: &str = "open-date";

/// [`show_main_window`], then tell the webview where to land. Untested like
/// its first half; everything it decides was decided by `parse_date`.
pub(crate) fn open_at(app: &AppHandle, ymd: &str) {
    use tauri::Emitter;
    show_main_window(app);
    let _ = app.emit(OPEN_DATE_EVENT, ymd.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::TimeFormat;
    use crate::upcoming::{Feed, FeedEvent, FeedTask};

    /// 2026-08-29 09:00 UTC, and the times below are offsets from it.
    const T0: i64 = 1_787_994_000_000;

    fn ev(title: &str, start: i64, end: i64) -> FeedEvent {
        FeedEvent {
            title: Some(title.into()),
            start_ms: start,
            end_ms: end,
            all_day: false,
            tz: "UTC".into(),
            location: None,
            attendees: 0,
            response: None,
            conference: None,
            color: None,
            calendar: None,
        }
    }

    fn feed(events: Vec<FeedEvent>) -> Feed {
        Feed { version: 1, generated_ms: T0, events, tasks: Vec::new() }
    }

    /// The running meeting outranks the next one: being *in* something is
    /// the more useful fact, and the marker is what tells the two apart.
    #[test]
    fn the_title_prefers_the_meeting_you_are_in() {
        let f = feed(vec![
            ev("Standup", T0 - 600_000, T0 + 600_000),
            ev("Review", T0 + 3_600_000, T0 + 7_200_000),
        ]);
        assert_eq!(menu_title(&f, T0, TimeFormat::H24).as_deref(), Some("▸ Standup"));
    }

    /// With nothing running, the next one — and it carries the start time,
    /// which is the whole reason to look at the menu bar.
    #[test]
    fn the_title_falls_to_the_next_meeting_with_its_clock() {
        let f = feed(vec![ev("Review", T0 + 3_600_000, T0 + 7_200_000)]);
        assert_eq!(menu_title(&f, T0, TimeFormat::H24).as_deref(), Some("10:00  Review"));
    }

    /// An event that ended exactly now has ended — the half-open rule every
    /// interval in this codebase follows.
    #[test]
    fn a_meeting_ending_now_is_not_the_one_you_are_in() {
        let f = feed(vec![ev("Standup", T0 - 600_000, T0)]);
        assert_eq!(menu_title(&f, T0, TimeFormat::H24), None);
    }

    /// All-day entries never claim the width: a day-long trip would sit
    /// there all day saying nothing about the next hour.
    #[test]
    fn an_all_day_event_never_becomes_the_title() {
        let mut trip = ev("Trip to Sofia", T0 - 3_600_000, T0 + 80_000_000);
        trip.all_day = true;
        let f = feed(vec![trip]);
        assert_eq!(menu_title(&f, T0, TimeFormat::H24), None);
    }

    /// `None`, not `Some("")` — AppKit treats the two differently, and a
    /// stale title after the meeting is worse than no title at all.
    #[test]
    fn an_empty_calendar_gets_no_title_rather_than_an_empty_one() {
        assert_eq!(menu_title(&feed(vec![]), T0, TimeFormat::H24), None);
    }

    /// Cut by characters, never bytes: this calendar is full of Cyrillic,
    /// and slicing a `String` off a char boundary is a panic.
    #[test]
    fn a_long_cyrillic_title_is_cut_without_panicking() {
        let long = "Консулски услуги в посолството на Република България";
        let f = feed(vec![ev(long, T0 + 60_000, T0 + 600_000)]);
        let title = menu_title(&f, T0, TimeFormat::H24).expect("a next meeting");
        assert!(title.ends_with('…'), "{title}");
        assert!(title.chars().count() <= TITLE_CAP + 8, "{title}");
    }

    /// The row's id has to survive the trip out to AppKit and back through
    /// `action_for` as the same day, or the row opens the wrong one.
    #[test]
    fn a_row_id_round_trips_to_the_day_its_event_is_on() {
        let f = feed(vec![ev("Standup", T0, T0 + 600_000)]);
        let r = rows(&f, T0 - 60_000, TimeFormat::H24);
        assert_eq!(r[0].label, "09:00  Standup");
        assert_eq!(action_for(&r[0].id), Some(TrayAction::OpenAt("2026-08-29".into())));
    }

    /// One Join, for the meeting at hand — a dropdown of them is a way to
    /// join the wrong call.
    #[test]
    fn only_the_meeting_at_hand_offers_a_join() {
        let mut now = ev("Standup", T0 - 60_000, T0 + 600_000);
        now.conference = Some("https://meet.google.com/abc-defg-hij".into());
        let mut later = ev("Review", T0 + 3_600_000, T0 + 7_200_000);
        later.conference = Some("https://zoom.us/j/999".into());
        let r = rows(&feed(vec![now, later]), T0, TimeFormat::H24);
        let joins: Vec<_> = r.iter().filter(|x| x.label == "Join meeting").collect();
        assert_eq!(joins.len(), 1);
        assert_eq!(
            action_for(&joins[0].id),
            Some(TrayAction::Join("https://meet.google.com/abc-defg-hij".into()))
        );
    }

    /// Anything that is not http(s) is not a thing this menu hands to the
    /// opener, however it got into the feed.
    #[test]
    fn a_non_web_scheme_is_never_a_join() {
        assert_eq!(action_for("join:file:///etc/passwd"), None);
        assert_eq!(action_for("join:javascript:alert(1)"), None);
        assert_eq!(action_for("at:2026-13-40"), None, "jiff refuses the impossible day");
        assert_eq!(action_for("at:2026-9-1"), None, "one spelling only");
    }

    /// Overdue reads differently from due, because that is the difference
    /// worth glancing at.
    #[test]
    fn tasks_follow_the_meetings_and_overdue_ones_say_so() {
        let mut f = feed(vec![]);
        f.tasks = vec![
            FeedTask { title: "Pay Unicredit".into(), due_ms: T0 - 3_600_000, all_day: false,
                       overdue: true, list: None, color: None, priority: 0 },
            FeedTask { title: "Renew domain".into(), due_ms: T0 + 3_600_000, all_day: false,
                       overdue: false, list: None, color: None, priority: 0 },
        ];
        let r = rows(&f, T0, TimeFormat::H24);
        assert_eq!(r[0].label, "⚠  Pay Unicredit");
        assert_eq!(r[1].label, "due  Renew domain");
        assert_eq!(r[0].id, "open", "a task row has no day of its own to open");
    }

    /// Open, Sync now, Quit — in that order, and Quit present at all.
    #[test]
    fn the_tray_menu_offers_open_sync_and_quit() {
        assert_eq!(
            MENU.map(|(id, _)| id),
            ["open", "sync", "quit"],
            "the tray menu's contents and their order"
        );
        assert_eq!(MENU.map(|(_, label)| label), ["Open omacal", "Sync now", "Quit"]);
    }

    /// Stated on its own because losing it is not a cosmetic regression: with
    /// the close button only hiding the window, a tray with no Quit leaves no
    /// way to exit the app short of killing the process.
    ///
    /// **What the first assertion is and is not.** It pins a *constant*, not a
    /// behaviour: the window is actually hidden by the `CloseRequested` arm in
    /// `lib.rs`, inside a Tauri event closure this project cannot drive from a
    /// test. So this asserts that the flag that arm consults still says hide,
    /// and nothing more. If someone deletes the arm and leaves the constant,
    /// every test here still passes and closing the window quits the app.
    /// Recorded plainly rather than left to look like the others.
    #[test]
    fn quit_is_on_the_menu_because_closing_the_window_does_not_quit() {
        assert!(hide_instead_of_closing(), "fixture check: closing only hides");
        assert!(
            MENU.iter().any(|(id, _)| *id == "quit"),
            "closing the window only hides it, so the tray must offer a way out"
        );
    }

    /// Every id on the menu maps to something. An entry that mapped to nothing
    /// would render, be clickable, and do nothing at all.
    #[test]
    fn every_menu_entry_maps_to_an_action() {
        for (id, label) in MENU {
            assert!(action_for(id).is_some(), "menu entry {label:?} ({id}) does nothing");
        }
        assert_eq!(action_for("open"), Some(TrayAction::Open));
        assert_eq!(action_for("sync"), Some(TrayAction::SyncNow));
        assert_eq!(action_for("quit"), Some(TrayAction::Quit));
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// The whole contract of the second-invocation channel: the two flags,
    /// the bare-launch default, and — the case that matters most, because a
    /// stray flag must never quit someone's app — unknown arguments reading
    /// as Open.
    #[test]
    fn a_second_invocations_argv_maps_to_an_action() {
        assert_eq!(instance_action(&argv(&["omacal", "--quit"])), TrayAction::Quit);
        assert_eq!(instance_action(&argv(&["omacal", "--sync-now"])), TrayAction::SyncNow);
        assert_eq!(instance_action(&argv(&["omacal"])), TrayAction::Open);
        assert_eq!(instance_action(&argv(&["omacal", "--wat"])), TrayAction::Open);
        // Quit outranks sync when both are passed: the stronger ask wins,
        // and a sync on a quitting app is work thrown away.
        assert_eq!(
            instance_action(&argv(&["omacal", "--sync-now", "--quit"])),
            TrayAction::Quit
        );
        assert_eq!(action_for("nonsense"), None);
    }

    /// The dated invocation: one spelling in, the same spelling out, and
    /// everything that is not exactly a date falling back to the rule above —
    /// Open, never an error, because a second instance has no stderr.
    #[test]
    fn a_positional_date_opens_the_window_on_that_date() {
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01"])),
            TrayAction::OpenAt("2026-09-01".into())
        );
        // The shape gate: a date jiff might tolerate is still not the one
        // spelling this contract admits.
        assert_eq!(instance_action(&argv(&["omacal", "2026-9-1"])), TrayAction::Open);
        // The right shape naming no day at all.
        assert_eq!(instance_action(&argv(&["omacal", "2026-13-40"])), TrayAction::Open);
        // The flags outrank a date — `--quit` alongside one is the stronger
        // ask, and a quitting app has nowhere to land.
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01", "--quit"])),
            TrayAction::Quit
        );
        assert_eq!(
            instance_action(&argv(&["omacal", "2026-09-01", "--sync-now"])),
            TrayAction::SyncNow
        );
    }

    /// The other half of the demo promise. Demo mode never writes the real
    /// database, never reaches Google, posts no notifications — and does not
    /// register itself to launch on login either.
    #[test]
    fn demo_mode_never_registers_start_on_login() {
        assert!(may_autostart(false));
        assert!(!may_autostart(true), "a synthetic-data build must not launch itself");
    }
}
